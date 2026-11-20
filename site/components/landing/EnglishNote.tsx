import { DEFAULT_LANGUAGE, dictionary } from '@/lib/i18n'
import { currentLanguage } from '@/lib/language'

/** One line, in the visitor's language, on a page whose body is still English. */
export default function EnglishNote() {
  const language = currentLanguage()
  if (language === DEFAULT_LANGUAGE) return null
  return <p className="l-english-note">{dictionary(language).english}</p>
}
