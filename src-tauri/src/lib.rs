// src/lib.rs
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]
#![allow(clippy::uninlined_format_args)]

// --- CẬP NHẬT IMPORT ĐÚNG CHO ENIGO 0.2 VÀ IMAGE ---
use screenshots::Screen;
use std::time::Instant;
// Enigo 0.2 dùng trait Mouse và Button, Coordinate, Direction
use enigo::{Enigo, Mouse, Button, Coordinate, Direction, Settings}; 
use std::thread;
use std::time::Duration;
use std::io::Cursor;
use image::ImageFormat; // Import để định dạng ảnh PNG
// ----------------------------------------------------

use tauri_plugin_shell::ShellExt;
use tauri_plugin_shell::process::{CommandChild, CommandEvent};
use tauri::{AppHandle, Emitter};
use std::sync::{Arc, Mutex};
use tauri::async_runtime;
use std::process::Command;
use encoding_rs::GBK;
use std::path::Path;
use std::fs;
use base64::Engine;
#[cfg(target_os = "android")]
use std::os::unix::fs::PermissionsExt;
use clipboard::{ClipboardContext, ClipboardProvider};

mod opening_book;
use opening_book::{JieqiOpeningBook, MoveData, OpeningBookStats, AddEntryRequest};

// -------------------------------------------------------------
// type definition for the engine process state
type EngineProcess = Arc<Mutex<Option<CommandChild>>>;
// -------------------------------------------------------------

// --- [NEW] HÀM CHỤP ẢNH MÀN HÌNH (ĐÃ FIX LỖI BUFFER) ---
#[tauri::command]
async fn capture_screen() -> Result<String, String> {
    let start = Instant::now();
    
    let screens = Screen::all().map_err(|e| e.to_string())?;
    let screen = screens.first().ok_or("No screen found")?;

    println!("[DEBUG] Capturing screen: {:?}", screen);

    let image = screen.capture().map_err(|e| e.to_string())?;

    // FIX: Chuyển RgbaImage sang DynamicImage để dùng được write_to
    let dyn_image = image::DynamicImage::ImageRgba8(image);
    
    let mut buffer = Vec::new();
    // FIX: Dùng write_to thay vì save, và chỉ định format Png
    dyn_image.write_to(&mut Cursor::new(&mut buffer), ImageFormat::Png)
             .map_err(|e| e.to_string())?;

    let base64_img = base64::engine::general_purpose::STANDARD.encode(&buffer);
    
    println!("[DEBUG] Screen captured in {:?}", start.elapsed());
    
    Ok(format!("data:image/png;base64,{}", base64_img))
}

// --- [NEW] HÀM AUTO CLICK CHUỘT (ĐÃ FIX CHO ENIGO 0.2) ---
#[tauri::command]
async fn perform_mouse_move(start_x: i32, start_y: i32, end_x: i32, end_y: i32) -> Result<(), String> {
    // FIX: Enigo::new() cần tham số Settings trong bản 0.2
    let mut enigo = Enigo::new(&Settings::default()).map_err(|e| format!("Init Enigo failed: {}", e))?;
    
    println!("[AUTO] Move: ({},{}) -> ({},{})", start_x, start_y, end_x, end_y);

    // 1. Di chuyển đến quân cờ (Dùng Coordinate::Abs cho tọa độ màn hình)
    enigo.move_mouse(start_x, start_y, Coordinate::Abs).map_err(|e| e.to_string())?;
    thread::sleep(Duration::from_millis(50));
    
    // 2. Nhấn giữ chuột trái (Button::Left, Direction::Press)
    enigo.button(Button::Left, Direction::Press).map_err(|e| e.to_string())?;
    thread::sleep(Duration::from_millis(50));
    
    // 3. Kéo đến ô đích
    enigo.move_mouse(end_x, end_y, Coordinate::Abs).map_err(|e| e.to_string())?;
    thread::sleep(Duration::from_millis(100)); 
    
    // 4. Thả chuột (Button::Left, Direction::Release)
    enigo.button(Button::Left, Direction::Release).map_err(|e| e.to_string())?;
    
    Ok(())
}

/// Check if the engine file exists and is a file on Android.
#[cfg(target_os = "android")]
fn check_android_engine_file(path: &str) -> Result<(), String> {
    let engine_path = Path::new(path);
    if !engine_path.exists() {
        return Err(format!("Engine file not found: {}", path));
    }
    if let Ok(metadata) = fs::metadata(engine_path) {
        if !metadata.is_file() {
            return Err(format!("Path is not a file: {}", path));
        }
    } else {
        return Err(format!("Cannot access engine file metadata: {}", path));
    }
    Ok(())
}

/// Copy a file from a user-accessible directory to the app's internal storage.
#[cfg(target_os = "android")]
fn copy_file_to_internal_storage(source_path_str: &str, app_handle: &AppHandle) -> Result<String, String> {
    let source_path = Path::new(source_path_str);
    if !source_path.exists() {
        let error_msg = format!("Source file not found: {}", source_path.display());
        let _ = app_handle.emit("engine-output", format!("[DEBUG] {}", error_msg));
        return Err(error_msg);
    }

    let bundle_identifier = &app_handle.config().identifier;
    let internal_dir = format!("/data/data/{}/files/engines", bundle_identifier);
    if let Err(e) = fs::create_dir_all(&internal_dir) {
        let error_msg = format!("Failed to create internal directory: {}", e);
        let _ = app_handle.emit("engine-output", format!("[DEBUG] {}", error_msg));
        return Err(error_msg);
    }

    let filename = source_path.file_name()
        .ok_or_else(|| "Invalid source path".to_string())?
        .to_str()
        .ok_or_else(|| "Invalid filename encoding".to_string())?;
    let dest_path_str = format!("{}/{}", internal_dir, filename);
    let dest_path = Path::new(&dest_path_str);

    let _ = app_handle.emit("engine-output", format!("[DEBUG] Copying file from {} to {}", source_path.display(), dest_path.display()));

    if let Err(e) = fs::copy(source_path, dest_path) {
        let error_msg = format!("Failed to copy file: {}", e);
        let _ = app_handle.emit("engine-output", format!("[DEBUG] {}", error_msg));
        return Err(error_msg);
    }

    let _ = app_handle.emit("engine-output", "[DEBUG] Setting executable permission...");
    
    match fs::metadata(dest_path) {
        Ok(metadata) => {
            let mut permissions = metadata.permissions();
            permissions.set_mode(0o755);   

            if let Err(e) = fs::set_permissions(dest_path, permissions) {
                let error_msg = format!("Failed to set executable permission: {}", e);
                let _ = app_handle.emit("engine-output", format!("[DEBUG] {}", error_msg));
                return Err(error_msg);
            }
        },
        Err(e) => {
            let error_msg = format!("Failed to get metadata for setting permissions: {}", e);
            let _ = app_handle.emit("engine-output", format!("[DEBUG] {}", error_msg));
            return Err(error_msg);
        }
    }
    
    let _ = app_handle.emit("engine-output", format!("[DEBUG] Successfully copied and made executable: {}", dest_path.display()));
    Ok(dest_path_str)
}

/// Save game notation to Android's external storage.
#[tauri::command]
async fn save_game_notation(content: String, filename: String, app: AppHandle) -> Result<String, String> {
    if !cfg!(target_os = "android") {
        return Err("This function is only available on Android".to_string());
    }

    let bundle_identifier = &app.config().identifier;
    let external_dir = format!("/storage/emulated/0/Android/data/{}/files/notations", bundle_identifier);
    
    if let Err(e) = fs::create_dir_all(&external_dir) {
        return Err(format!("Failed to create notations directory: {}", e));
    }

    let file_path_str = format!("{}/{}", external_dir, filename);
    let file_path = Path::new(&file_path_str);

    if let Err(e) = fs::write(file_path, content) {
        return Err(format!("Failed to write notation file: {}", e));
    }

    Ok(file_path_str)
}

/// Save chart image to Android's external storage.
#[tauri::command]
async fn save_chart_image(content: String, filename: String, app: AppHandle) -> Result<String, String> {
    if !cfg!(target_os = "android") {
        return Err("This function is only available on Android".to_string());
    }

    let bundle_identifier = &app.config().identifier;
    let external_dir = format!("/storage/emulated/0/Android/data/{}/files/charts", bundle_identifier);
    
    if let Err(e) = fs::create_dir_all(&external_dir) {
        return Err(format!("Failed to create charts directory: {}", e));
    }

    let cleaned_content = content.replace("data:image/png;base64,", "");
    let decoded_content = match base64::engine::general_purpose::STANDARD.decode(&cleaned_content) {
        Ok(data) => data,
        Err(e) => return Err(format!("Failed to decode image data: {}", e)),
    };

    let file_path_str = format!("{}/{}", external_dir, filename);
    let file_path = Path::new(&file_path_str);

    if let Err(e) = fs::write(file_path, decoded_content) {
        return Err(format!("Failed to write chart image file: {}", e));
    }

    Ok(file_path_str)
}

fn get_config_file_path(app: &AppHandle) -> Result<String, String> {
    if cfg!(target_os = "android") {
        let bundle_identifier = &app.config().identifier;
        Ok(format!("/data/data/{}/files/config.ini", bundle_identifier))
    } else {
        Ok("config.ini".to_string())
    }
}

fn get_autosave_file_path(app: &AppHandle) -> Result<String, String> {
    if cfg!(target_os = "android") {
        let bundle_identifier = &app.config().identifier;
        Ok(format!("/data/data/{}/files/Autosave.json", bundle_identifier))
    } else {
        Ok("Autosave.json".to_string())
    }
}

fn get_opening_book_db_path(app: &AppHandle) -> Result<String, String> {
    if cfg!(target_os = "android") {
        let bundle_identifier = &app.config().identifier;
        Ok(format!("/data/data/{}/files/jieqi_openings.jb", bundle_identifier))
    } else {
        Ok("jieqi_openings.jb".to_string())
    }
}

#[tauri::command]
async fn load_config(app: AppHandle) -> Result<String, String> {
    let config_path = get_config_file_path(&app)?;
    let path = Path::new(&config_path);
    
    if !path.exists() {
        return Ok(String::new());
    }
    
    match fs::read_to_string(path) {
        Ok(content) => Ok(content),
        Err(e) => Err(format!("Failed to read config file: {}", e)),
    }
}

#[tauri::command]
async fn save_config(content: String, app: AppHandle) -> Result<(), String> {
    let config_path = get_config_file_path(&app)?;
    let path = Path::new(&config_path);
    
    if let Some(parent) = path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            return Err(format!("Failed to create config directory: {}", e));
        }
    }
    
    match fs::write(path, content) {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("Failed to write config file: {}", e)),
    }
}

#[tauri::command]
async fn clear_config(app: AppHandle) -> Result<(), String> {
    let config_path = get_config_file_path(&app)?;
    let path = Path::new(&config_path);
    
    if path.exists() {
        match fs::remove_file(path) {
            Ok(_) => Ok(()),
            Err(e) => Err(format!("Failed to delete config file: {}", e)),
        }
    } else {
        Ok(())
    }
}

#[tauri::command]
async fn save_autosave(content: String, app: AppHandle) -> Result<(), String> {
    let autosave_path = get_autosave_file_path(&app)?;
    let path = Path::new(&autosave_path);
    
    if let Some(parent) = path.parent() {
        if let Err(e) = fs::create_dir_all(parent) {
            return Err(format!("Failed to create autosave directory: {}", e));
        }
    }
    
    match fs::write(path, content) {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("Failed to write autosave file: {}", e)),
    }
}

#[tauri::command]
async fn load_autosave(app: AppHandle) -> Result<String, String> {
    let autosave_path = get_autosave_file_path(&app)?;
    let path = Path::new(&autosave_path);
    
    if !path.exists() {
        return Ok(String::new());
    }
    
    match fs::read_to_string(path) {
        Ok(content) => Ok(content),
        Err(e) => Err(format!("Failed to read autosave file: {}", e)),
    }
}

#[cfg(target_os = "android")]
fn get_user_engine_directory() -> String {
    "/storage/emulated/0/jieqibox/engines".to_string()
}

#[cfg(target_os = "android")]
fn sync_and_list_engines(app_handle: &AppHandle) -> Result<Vec<String>, String> {
    let bundle_identifier = &app_handle.config().identifier;
    let source_dirs = vec![
        get_user_engine_directory(),
        format!("/storage/emulated/0/Android/data/{}/files/engines", bundle_identifier),
    ];
    let internal_dir_str = format!("/data/data/{}/files/engines", bundle_identifier);
    
    let _ = app_handle.emit("engine-output", format!("[DEBUG] Syncing engines. Internal dir: {}. Source dirs: {:?}", internal_dir_str, source_dirs));
    
    if let Err(e) = fs::create_dir_all(&internal_dir_str) {
        let error_msg = format!("Failed to create internal directory '{}': {}", internal_dir_str, e);
        let _ = app_handle.emit("engine-output", format!("[DEBUG] {}", error_msg));
        return Err(error_msg);
    } else {
        let _ = app_handle.emit("engine-output", format!("[DEBUG] Internal directory created/exists: {}", internal_dir_str));
    }

    for user_dir in &source_dirs {
        let _ = app_handle.emit("engine-output", format!("[DEBUG] Checking source directory: {}", user_dir));
        let user_path = Path::new(user_dir);

        if !user_path.exists() {
            let _ = app_handle.emit("engine-output", format!("[DEBUG] Source directory does not exist, skipping: {}", user_dir));
            continue;
        }

        if let Ok(entries) = fs::read_dir(user_path) {
            for entry_result in entries {
                if let Ok(entry) = entry_result {
                    let path = entry.path();
                    if path.is_file() {
                        if let Err(e) = copy_file_to_internal_storage(path.to_str().unwrap_or(""), app_handle) {
                            let _ = app_handle.emit("engine-output", format!("[DEBUG] Failed to copy file {}: {}", path.display(), e));
                        }
                    }
                }
            }
        }
    }

    let mut available_engines = Vec::new();
    if let Ok(entries) = fs::read_dir(&internal_dir_str) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(path_str) = path.to_str() {
                    available_engines.push(path_str.to_string());
                }
            }
        }
    }
    
    let _ = app_handle.emit("engine-output", format!("[DEBUG] Available internal engines: {:?}", available_engines));
    Ok(available_engines)
}

#[tauri::command]
async fn kill_engine(process_state: tauri::State<'_, EngineProcess>) -> Result<(), String> {
    if let Some(child) = process_state.lock().unwrap().take() {
        let _ = child.kill();
    }
    Ok(())
}

#[tauri::command]
async fn spawn_engine(
    path: String,
    args: Vec<String>,
    app: AppHandle,
    process_state: tauri::State<'_, EngineProcess>,
) -> Result<(), String> {
    if cfg!(target_os = "android") {
        let _ = app.emit("engine-output", format!("[DEBUG] Spawning engine: Path={}, Args={:?}", path, args));
    }
    
    let final_path = path;

    #[cfg(target_os = "android")]
    {
        if let Err(e) = check_android_engine_file(&final_path) {
            let _ = app.emit("engine-output", format!("[DEBUG] Engine file validation failed: {}", e));
            return Err(e);
        }
        let _ = app.emit("engine-output", "[DEBUG] Engine file validation passed.");
    }
    
    kill_engine(process_state.clone()).await.ok();
    
    let engine_dir = Path::new(&final_path)
        .parent()
        .ok_or_else(|| "Failed to get engine directory".to_string())?
        .to_str()
        .ok_or_else(|| "Failed to convert engine directory to string".to_string())?;
    
    let (mut rx, child) = match app.shell().command(&final_path)
        .args(args)
        .current_dir(engine_dir)
        .spawn() 
    {
        Ok(result) => result,
        Err(e) => {
            let error_msg = format!("Failed to spawn engine: {}", e);
            if cfg!(target_os = "android") {
                let _ = app.emit("engine-output", format!("[DEBUG] {}", error_msg));
            }
            return Err(error_msg);
        }
    };

    *process_state.lock().unwrap() = Some(child);
    
    let app_clone = app.clone();
    async_runtime::spawn(async move {
        while let Some(event) = rx.recv().await {
            if let CommandEvent::Stdout(buf) | CommandEvent::Stderr(buf) = event {
                let text = if cfg!(target_os = "windows") {
                    let (cow, ..) = GBK.decode(&buf);
                    cow.into_owned()
                } else {
                    String::from_utf8_lossy(&buf).into_owned()
                };
                let _ = app_clone.emit("engine-output", text);
            }
        }
    });

    Ok(())
}

#[tauri::command]
async fn send_to_engine(
    command: String,
    process_state: tauri::State<'_, EngineProcess>,
) -> Result<(), String> {
    if let Some(child) = process_state.lock().unwrap().as_mut() {
        child
            .write(format!("{}\n", command).as_bytes())
            .map_err(|e| format!("Failed to write to engine: {}", e))?;
        Ok(())
    } else {
        Err("Engine not running.".into())
    }
}

#[cfg(target_os = "android")]
#[tauri::command]
async fn get_default_android_engine_path() -> Result<String, String> {
    Ok(get_user_engine_directory())
}

#[cfg(target_os = "android")]
#[tauri::command]
async fn check_android_file_permissions(path: String) -> Result<bool, String> {
    if let Ok(metadata) = fs::metadata(Path::new(&path)) {
        Ok(metadata.is_file())
    } else {
        Ok(false)
    }
}

#[cfg(target_os = "android")]
#[tauri::command]
async fn get_bundle_identifier(app: AppHandle) -> Result<String, String> {
    Ok(app.config().identifier.clone())
}

#[cfg(target_os = "android")]
#[tauri::command]
async fn scan_android_engines(app: AppHandle) -> Result<Vec<String>, String> {
    sync_and_list_engines(&app)
}

#[cfg(target_os = "android")]
#[tauri::command]
async fn request_saf_file_selection(name: String, args: String, has_nnue: bool, app: AppHandle) -> Result<(), String> {
    let _ = app.emit("request-saf-file-selection", serde_json::json!({
        "name": name,
        "args": args,
        "has_nnue": has_nnue
    }));
    Ok(())
}

#[cfg(target_os = "android")]
#[tauri::command]
async fn handle_saf_file_result(
    temp_file_path: String,
    filename: String,
    name: String,
    args: String,
    has_nnue: bool,
    app: AppHandle,
) -> Result<(), String> {
    let _ = app.emit("engine-output", format!("[DEBUG] SAF result for engine '{}': TempPath={}, Filename={}", name, temp_file_path, filename));

    if temp_file_path.is_empty() {
        return Err("SAF file processing failed: temporary path is empty.".to_string());
    }

    let engine_instance_id = format!("{}_{}", name, chrono::Utc::now().timestamp_millis());
    let bundle_identifier = &app.config().identifier;
    let engine_base_dir = format!("/data/data/{}/files/engines/{}", bundle_identifier, &engine_instance_id);

    if let Err(e) = fs::create_dir_all(&engine_base_dir) {
        let error_msg = format!("Failed to create final engine directory: {}", e);
        let _ = app.emit("engine-output", format!("[DEBUG] {}", error_msg));
        return Err(error_msg);
    }
    
    let final_path_str = format!("{}/{}", engine_base_dir, &filename);

    if let Err(e) = fs::rename(&temp_file_path, &final_path_str) {
        let error_msg = format!("Failed to move engine file from temp to final destination: {}", e);
        let _ = app.emit("engine-output", format!("[DEBUG] {}", error_msg));
        if let Err(copy_err) = fs::copy(&temp_file_path, &final_path_str) {
             let copy_error_msg = format!("Fallback copy also failed: {}", copy_err);
             let _ = app.emit("engine-output", format!("[DEBUG] {}", copy_error_msg));
             return Err(copy_error_msg);
        } else {
            let _ = fs::remove_file(&temp_file_path);
        }
    }

    let final_path = Path::new(&final_path_str);
    let mut perms = fs::metadata(final_path).map_err(|e| e.to_string())?.permissions();
    perms.set_mode(0o755);
    fs::set_permissions(final_path, perms).map_err(|e| e.to_string())?;

    if has_nnue {
        let _ = app.emit("engine-output", "[DEBUG] Engine requires NNUE file, requesting file selection...");
        let nnue_request_data = serde_json::json!({
            "engine_name": name,
            "engine_path": final_path_str,
            "args": args,
            "engine_instance_id": engine_instance_id
        });
        app.emit("request-nnue-file", nnue_request_data).map_err(|e| e.to_string())?;
        return Ok(());
    }

    let new_engine_data = serde_json::json!({
        "id": format!("engine_{}", chrono::Utc::now().timestamp_millis()),
        "name": name,
        "path": final_path_str,
        "args": args
    });

    app.emit("android-engine-added", new_engine_data).map_err(|e| e.to_string())?;
    
    Ok(())
}

#[cfg(target_os = "android")]
#[tauri::command]
async fn handle_nnue_file_result(
    temp_file_path: String,
    filename: String,
    engine_name: String,
    engine_path: String,
    args: String,
    engine_instance_id: String,
    app: AppHandle,
) -> Result<(), String> {
    let _ = app.emit("engine-output", format!("[DEBUG] NNUE file result for engine '{}': TempPath={}, Filename={}", engine_name, temp_file_path, filename));

    if temp_file_path.is_empty() {
        return Err("NNUE file processing failed: temporary path is empty.".to_string());
    }

    let bundle_identifier = &app.config().identifier;
    let engine_base_dir = format!("/data/data/{}/files/engines/{}", bundle_identifier, &engine_instance_id);
    let final_nnue_path_str = format!("{}/{}", engine_base_dir, &filename);

    if let Err(e) = fs::rename(&temp_file_path, &final_nnue_path_str) {
        let error_msg = format!("Failed to move NNUE file from temp to final destination: {}", e);
        let _ = app.emit("engine-output", format!("[DEBUG] {}", error_msg));
        if let Err(copy_err) = fs::copy(&temp_file_path, &final_nnue_path_str) {
             let copy_error_msg = format!("Fallback copy also failed: {}", copy_err);
             let _ = app.emit("engine-output", format!("[DEBUG] {}", copy_error_msg));
             return Err(copy_error_msg);
        } else {
            let _ = fs::remove_file(&temp_file_path);
        }
    }

    let _ = app.emit("engine-output", format!("[DEBUG] NNUE file successfully copied to: {}", final_nnue_path_str));

    let new_engine_data = serde_json::json!({
        "id": format!("engine_{}", chrono::Utc::now().timestamp_millis()),
        "name": engine_name,
        "path": engine_path,
        "args": args
    });

    app.emit("android-engine-added", new_engine_data).map_err(|e| e.to_string())?;
    
    Ok(())
}

#[tauri::command]
async fn open_external_url(url: String, app: AppHandle) -> Result<(), String> {
    let result = if cfg!(target_os = "windows") {
        Command::new("cmd").args(["/C", "start", &url]).spawn()
    } else if cfg!(target_os = "macos") {
        Command::new("open").arg(&url).spawn()
    } else if cfg!(target_os = "android") {
        let _ = app.emit("open-external-url", url);
        return Ok(());
    } else {
        Command::new("xdg-open").arg(&url).spawn()
    };

    match result {
        Ok(_) => Ok(()),
        Err(e) => Err(format!("Failed to open URL: {}", e))
    }
}

// Opening Book Commands

#[tauri::command]
async fn opening_book_add_entry(
    request: AddEntryRequest,
    app: AppHandle,
) -> Result<bool, String> {
    let db_path = get_opening_book_db_path(&app)?;
    let book = JieqiOpeningBook::new(db_path).map_err(|e| e.to_string())?;
    book.add_entry(&request)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn opening_book_delete_entry(
    fen: String,
    uci_move: String,
    app: AppHandle,
) -> Result<bool, String> {
    let db_path = get_opening_book_db_path(&app)?;
    let book = JieqiOpeningBook::new(db_path).map_err(|e| e.to_string())?;
    book.delete_entry(&fen, &uci_move)
        .map_err(|e| e.to_string())
}

#[tauri::command]
async fn opening_book_query_moves(fen: String, app: AppHandle) -> Result<Vec<MoveData>, String> {
    let db_path = get_opening_book_db_path(&app)?;
    let book = JieqiOpeningBook::new(db_path).map_err(|e| e.to_string())?;
    book.query_moves(&fen).map_err(|e| e.to_string())
}

#[tauri::command]
async fn opening_book_get_stats(app: AppHandle) -> Result<OpeningBookStats, String> {
    let db_path = get_opening_book_db_path(&app)?;
    let book = JieqiOpeningBook::new(db_path).map_err(|e| e.to_string())?;
    book.get_stats().map_err(|e| e.to_string())
}

#[tauri::command]
async fn opening_book_clear_all(app: AppHandle) -> Result<(), String> {
    let db_path = get_opening_book_db_path(&app)?;
    let book = JieqiOpeningBook::new(db_path).map_err(|e| e.to_string())?;
    book.clear_all().map_err(|e| e.to_string())
}

#[tauri::command]
async fn opening_book_export_all(app: AppHandle) -> Result<String, String> {
    let db_path = get_opening_book_db_path(&app)?;
    let book = JieqiOpeningBook::new(db_path).map_err(|e| e.to_string())?;
    let entries = book.export_all().map_err(|e| e.to_string())?;
    serde_json::to_string(&entries).map_err(|e| e.to_string())
}

#[tauri::command]
async fn opening_book_import_entries(
    json_data: String,
    app: AppHandle,
) -> Result<(i32, Vec<String>), String> {
    let db_path = get_opening_book_db_path(&app)?;
    let book = JieqiOpeningBook::new(db_path).map_err(|e| e.to_string())?;

    let entries: Vec<opening_book::OpeningBookEntry> =
        serde_json::from_str(&json_data).map_err(|e| e.to_string())?;

    let mut imported = 0;
    let mut errors = Vec::new();

    for entry in entries {
        for move_data in entry.moves {
            let request = AddEntryRequest {
                fen: entry.fen.clone(),
                uci_move: move_data.uci_move.clone(),
                priority: move_data.priority,
                wins: move_data.wins,
                draws: move_data.draws,
                losses: move_data.losses,
                allowed: move_data.allowed,
                comment: move_data.comment.clone(),
            };
            match book.add_entry(&request) {
                Ok(_) => imported += 1,
                Err(e) => errors.push(format!("Failed to import move {}: {}", move_data.uci_move, e)),
            }
        }
    }

    Ok((imported, errors))
}

#[tauri::command]
async fn opening_book_export_db(destination_path: String, app: AppHandle) -> Result<(), String> {
    let source_path = get_opening_book_db_path(&app)?;
    fs::copy(source_path, destination_path).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn opening_book_import_db(source_path: String, app: AppHandle) -> Result<(), String> {
    let dest_path = get_opening_book_db_path(&app)?;
    fs::copy(source_path, dest_path).map_err(|e| e.to_string())?;
    Ok(())
}

#[tauri::command]
async fn save_game_notation_with_dialog(content: String, default_filename: String, app: AppHandle) -> Result<String, String> {
    #[cfg(target_os = "android")]
    {
        return save_game_notation(content, default_filename, app).await;
    }

    #[cfg(not(target_os = "android"))]
    {
        use tauri_plugin_dialog::{DialogExt, FilePath};
        
        let file_path = app.dialog()
            .file()
            .set_file_name(&default_filename)
            .add_filter("JSON files", &["json"])
            .add_filter("All files", &["*"])
            .blocking_save_file();

        match file_path {
            Some(FilePath::Path(path)) => {
                fs::write(&path, content)
                    .map_err(|e| format!("Failed to write file: {}", e))?;
                
                Ok(path.to_string_lossy().to_string())
            },
            Some(FilePath::Url(_)) => {
                Err("URL paths are not supported".to_string())
            },
            None => {
                Err("Save dialog was cancelled".to_string())
            }
        }
    }
}

#[tauri::command]
async fn copy_to_clipboard(text: String, _app: AppHandle) -> Result<(), String> {
    let mut ctx: ClipboardContext = ClipboardProvider::new()
        .map_err(|e| format!("Failed to access clipboard: {}", e))?;
    ctx.set_contents(text)
        .map_err(|e| format!("Failed to copy to clipboard: {}", e))
}

#[tauri::command]
async fn paste_from_clipboard(_app: AppHandle) -> Result<String, String> {
    let mut ctx: ClipboardContext = ClipboardProvider::new()
        .map_err(|e| format!("Failed to access clipboard: {}", e))?;
    ctx.get_contents()
        .map_err(|e| format!("Failed to paste from clipboard: {}", e))
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .manage(Arc::new(Mutex::new(None)) as EngineProcess)
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            spawn_engine, 
            kill_engine,
            send_to_engine, 
            open_external_url,
            save_game_notation,
            save_chart_image,
            load_config,
            save_config,
            clear_config,
            save_autosave,
            load_autosave,
            save_game_notation_with_dialog,
            copy_to_clipboard,
            paste_from_clipboard,
            opening_book_add_entry,
            opening_book_delete_entry,
            opening_book_query_moves,
            opening_book_get_stats,
            opening_book_clear_all,
            opening_book_export_all,
            opening_book_import_entries,
            opening_book_export_db,
            opening_book_import_db,
            // COMMANDS MỚI ĐÃ ĐƯỢC THÊM
            capture_screen, 
            perform_mouse_move, 
            // Android
            #[cfg(target_os = "android")]
            get_bundle_identifier,
            #[cfg(target_os = "android")]
            get_default_android_engine_path,
            #[cfg(target_os = "android")]
            check_android_file_permissions,
            #[cfg(target_os = "android")]
            scan_android_engines,
            #[cfg(target_os = "android")]
            request_saf_file_selection,
            #[cfg(target_os = "android")]
            handle_saf_file_result,
            #[cfg(target_os = "android")]
            handle_nnue_file_result
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}