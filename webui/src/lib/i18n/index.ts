import i18n, { type Resource } from 'i18next'
import { initReactI18next } from 'react-i18next'
import LanguageDetector from 'i18next-browser-languagedetector'

export const SUPPORTED_LANGUAGES = [
  { code: 'en', label: 'English' },
  { code: 'de', label: 'Deutsch' },
  { code: 'es', label: 'español' },
  { code: 'fr', label: 'français' },
  { code: 'it', label: 'italiano' },
  { code: 'ja', label: '日本語' },
  { code: 'ko', label: '한국어' },
  { code: 'ru', label: 'русский' },
  { code: 'zh-Hans', label: '简体中文' },
  { code: 'zh-Hant', label: '繁體中文' },
] as const

export type LanguageCode = (typeof SUPPORTED_LANGUAGES)[number]['code']

function resolveLanguage(lng: string): LanguageCode {
  const tag = lng.toLowerCase().replace(/_/g, '-')
  if (tag.startsWith('zh')) return /hant|-tw|-hk|-mo/.test(tag) ? 'zh-Hant' : 'zh-Hans'
  const exact = SUPPORTED_LANGUAGES.find((l) => l.code.toLowerCase() === tag)
  if (exact) return exact.code
  const primary = tag.split('-')[0]
  return SUPPORTED_LANGUAGES.find((l) => l.code.toLowerCase().split('-')[0] === primary)?.code ?? 'en'
}

// any locales/<lang>/<ns>.json is registered automatically, so new namespaces
// never need an edit here
const modules = import.meta.glob<{ default: Record<string, unknown> }>('./locales/*/*.json', { eager: true })
const resources: Resource = {}
for (const [path, mod] of Object.entries(modules)) {
  const match = /^\.\/locales\/([^/]+)\/([^/]+)\.json$/.exec(path)
  if (!match) continue
  const [, lng, ns] = match
  ;(resources[lng] ??= {})[ns] = mod.default
}

void i18n
  .use(LanguageDetector)
  .use(initReactI18next)
  .init({
    resources,
    fallbackLng: 'en',
    supportedLngs: SUPPORTED_LANGUAGES.map((l) => l.code),
    defaultNS: 'common',
    interpolation: { escapeValue: false },
    returnEmptyString: false,
    detection: {
      // querystring enables shareable ?lng= links; it wins and is then cached
      order: ['querystring', 'localStorage', 'navigator'],
      caches: ['localStorage'],
      lookupLocalStorage: 'kmweb.language',
      convertDetectedLanguage: resolveLanguage,
    },
  })

// the browser picks CJK fallback fonts and Han glyph variants from <html lang>
document.documentElement.lang = i18n.language
i18n.on('languageChanged', (lng) => {
  document.documentElement.lang = lng
})

export default i18n
