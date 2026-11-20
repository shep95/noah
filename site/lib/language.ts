import { headers } from 'next/headers'
import { DEFAULT_LANGUAGE, LANGUAGE_HEADER, type Language, isLanguage } from './i18n'

// Kept apart from lib/i18n.ts because next/headers exists only on the server,
// and the dictionaries are also read by client components.
/** The language middleware chose for this request. Server components only. */
export function currentLanguage(): Language {
  const chosen = headers().get(LANGUAGE_HEADER)
  return isLanguage(chosen) ? chosen : DEFAULT_LANGUAGE
}
