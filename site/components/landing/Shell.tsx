import fs from 'node:fs'
import path from 'node:path'
import Link from 'next/link'
import Effects from './Effects'
import ConsoleNotice from '@/components/ConsoleNotice'
import Analytics from '@/components/Analytics'
import LanguageMenu from './LanguageMenu'
import WallpaperPicker, { type Wallpaper } from './WallpaperPicker'
import { LINKS } from '@/lib/site'
import { dictionary, localized } from '@/lib/i18n'
import { currentLanguage } from '@/lib/language'
import '@/app/landing.css'

// Every page sits in the same room: the fixed wallpaper, the fog, a quiet nav
// and the footer. Pages only supply what is between them.
/** The fog-and-cabin default, then every image in public/wallpapers. */
function wallpapers(): Wallpaper[] {
  const directory = path.join(process.cwd(), 'public', 'wallpapers')
  let files: string[] = []
  try {
    files = fs.readdirSync(directory)
  } catch {
    files = []
  }
  const extra = files
    .filter((file) => /\.(jpe?g|png|webp)$/i.test(file))
    .sort()
    .map((file) => ({ name: file.replace(/\.[^.]+$/, ''), src: `/wallpapers/${file}` }))
  return [{ name: 'fog', src: '/wallpaper.jpg' }, ...extra]
}

export default function Shell({ children }: { children: React.ReactNode }) {
  const language = currentLanguage()
  const t = dictionary(language)
  const to = (path: string) => localized(path, language)
  return (
    <div className="landing">
      <Effects />
      <ConsoleNotice />
      <Analytics language={language} />
      <div className="l-wallpaper" aria-hidden />
      <div className="l-wallpaper-overlay" aria-hidden />
      <div className="l-mist" aria-hidden />
      <div className="l-mist-2" aria-hidden />

      <div className="l-site">
        <nav className="l-nav" aria-label="primary">
          <Link href={to('/')} className="l-nav-logo">
            noah
          </Link>
          <ul className="l-nav-links">
            <li><Link href={`${to('/')}#features`}>{t.nav.features}</Link></li>
            <li><Link href={to('/shepherd')}>{t.nav.shepherd}</Link></li>
            <li><Link href={to('/faq')}>{t.nav.faq}</Link></li>
            <li><Link href={to('/founder')}>{t.nav.founder}</Link></li>
            <li><Link href={to('/download')} className="l-nav-download">{t.nav.download}</Link></li>
            <LanguageMenu language={language} label={t.nav.language} />
          </ul>
        </nav>

        {children}

        <footer className="l-footer">
          <div className="l-footer-brand">
            <span className="l-footer-logo">noah</span>
            <span className="l-footer-tags">{t.footer.by}</span>
          </div>
          <ul className="l-footer-links">
            <li><Link href={to('/download')}>{t.nav.download}</Link></li>
            <li><Link href={to('/shepherd')}>{t.nav.shepherd}</Link></li>
            <li><Link href={to('/faq')}>{t.nav.faq}</Link></li>
            <li><Link href={to('/founder')}>{t.nav.founder}</Link></li>
            <li><Link href={to('/shield')}>shield</Link></li>
            <li><Link href={to('/compare')}>compare</Link></li>
            <li><Link href={to('/local-models')}>local models</Link></li>
            <li><Link href={to('/security')}>{t.footer.security}</Link></li>
            <li><Link href={to('/terms')}>{t.footer.terms}</Link></li>
            <li><Link href={to('/stats')}>{t.footer.stats}</Link></li>
            <li><a href={LINKS.source} target="_blank" rel="noopener noreferrer">{t.footer.source}</a></li>
            <li><a href={LINKS.discord} target="_blank" rel="noopener noreferrer">discord</a></li>
            <li><a href={LINKS.asherin} target="_blank" rel="noopener noreferrer">asherin.com</a></li>
          </ul>
        </footer>
      </div>
      <WallpaperPicker wallpapers={wallpapers()} />
    </div>
  )
}
