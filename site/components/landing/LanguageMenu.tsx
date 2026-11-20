'use client'

import { useEffect, useRef, useState } from 'react'
import { usePathname } from 'next/navigation'
import { LANGUAGES, LANGUAGE_CODES, LANGUAGE_STORAGE_KEY, type Language, localized, splitLanguage } from '@/lib/i18n'

// A quiet list of the languages the site speaks. Picking one is an ordinary
// link to the same page in that language; the choice is remembered so the
// layout's first-paint script stops guessing from the browser.
export default function LanguageMenu({ language, label }: { language: Language; label: string }) {
  const [open, setOpen] = useState(false)
  const panel = useRef<HTMLLIElement>(null)
  const pathname = usePathname() || '/'
  const { path } = splitLanguage(pathname)

  useEffect(() => {
    if (!open) return
    const close = (event: MouseEvent | KeyboardEvent) => {
      if (event instanceof KeyboardEvent) {
        if (event.key === 'Escape') setOpen(false)
        return
      }
      if (panel.current && !panel.current.contains(event.target as Node)) setOpen(false)
    }
    window.addEventListener('mousedown', close)
    window.addEventListener('keydown', close)
    return () => {
      window.removeEventListener('mousedown', close)
      window.removeEventListener('keydown', close)
    }
  }, [open])

  const remember = (chosen: Language) => {
    try {
      window.localStorage.setItem(LANGUAGE_STORAGE_KEY, chosen)
    } catch {
      // Private windows can refuse storage; the link still navigates.
    }
  }

  return (
    <li className="l-lang" ref={panel}>
      <button
        type="button"
        className="l-lang-toggle"
        onClick={() => setOpen((value) => !value)}
        aria-expanded={open}
        aria-haspopup="menu"
        aria-label={label}
      >
        {LANGUAGES[language]}
      </button>
      {open && (
        <ul className="l-lang-panel" role="menu" aria-label={label}>
          {LANGUAGE_CODES.map((code) => (
            <li key={code} role="none">
              <a
                role="menuitem"
                href={localized(path, code)}
                hrefLang={code}
                lang={code}
                className={code === language ? 'is-current' : undefined}
                aria-current={code === language ? 'true' : undefined}
                onClick={() => remember(code)}
              >
                {LANGUAGES[code]}
              </a>
            </li>
          ))}
        </ul>
      )}
    </li>
  )
}
