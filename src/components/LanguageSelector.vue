<template>
  <div class="language-selector">
    <v-menu>
      <template v-slot:activator="{ props }">
        <v-btn
          v-bind="props"
          icon="mdi-translate"
          size="small"
          color="primary"
          variant="text"
          :title="$t('languages.current')"
        />
      </template>
      <v-list>
        <v-list-item
          v-for="(name, code) in availableLanguages"
          :key="code"
          @click="changeLanguage(code)"
          :active="currentLanguage === code"
          :class="`language-option-${code}`"
          :lang="code"
        >
          <v-list-item-title>{{ name }}</v-list-item-title>
        </v-list-item>
      </v-list>
    </v-menu>
  </div>
</template>

<script setup lang="ts">
  import { computed } from 'vue'
  import { useI18n } from 'vue-i18n'
  import { useConfigManager } from '../composables/useConfigManager'

  const { t, locale } = useI18n()
  const configManager = useConfigManager()

  // Current language
  const currentLanguage = computed(() => locale.value)

  // Available languages
  const availableLanguages = computed(() => ({
    zh_cn: t('languages.zh_cn'),
    zh_tw: t('languages.zh_tw'),
    en: t('languages.en'),
    vi: t('languages.vi'),
    ja: t('languages.ja'),
  }))

  // Change language
  const changeLanguage = async (langCode: string) => {
    locale.value = langCode
    // Save to config file
    try {
      await configManager.updateLocale(langCode)
    } catch (error) {
      console.error('Failed to save language setting:', error)
    }
  }
</script>

<style lang="scss" scoped>
  .language-selector {
    display: flex;
    align-items: center;
  }

  // Language-specific font styles for each option
  .language-option-zh_cn {
    font-family:
      'Noto Sans SC', 'PingFang SC', 'Hiragino Sans GB', 'Microsoft YaHei',
      sans-serif;
  }

  .language-option-zh_tw {
    font-family:
      'Noto Sans TC', 'PingFang TC', 'Hiragino Sans TC', 'Microsoft JhengHei',
      sans-serif;
  }

  .language-option-ja {
    font-family:
      'Noto Sans JP', 'Hiragino Sans', 'Hiragino Kaku Gothic ProN', 'Yu Gothic',
      'Meiryo', sans-serif;
  }

  .language-option-en {
    font-family:
      'Noto Sans', 'Segoe UI', 'Roboto', 'Helvetica Neue', Arial, sans-serif;
  }

  .language-option-vi {
    font-family:
      'Noto Sans', 'Segoe UI', 'Roboto', 'Helvetica Neue', Arial, sans-serif;
  }
</style>
