import type { MetadataRoute } from 'next'
import { RELEASE_DATE, SITE_URL } from '@/lib/site'
import { LANGUAGE_CODES, localized } from '@/lib/i18n'

// Dates are when each page's content last changed, not the build time, so a
// redeploy does not tell crawlers that every page is new.
const PAGES: (MetadataRoute.Sitemap[number] & { translated?: boolean })[] = [
  { url: '/', lastModified: RELEASE_DATE, changeFrequency: 'weekly', priority: 1, translated: true },
  { url: '/download', lastModified: RELEASE_DATE, changeFrequency: 'weekly', priority: 0.9, translated: true },
  { url: '/shepherd', lastModified: new Date(Date.UTC(2026, 8, 26)), changeFrequency: 'monthly', priority: 0.8 },
  { url: '/faq', lastModified: new Date(Date.UTC(2026, 8, 26)), changeFrequency: 'monthly', priority: 0.7 },
  { url: '/shield', lastModified: new Date(Date.UTC(2026, 8, 27)), changeFrequency: 'monthly', priority: 0.7 },
  { url: '/compare', lastModified: new Date(Date.UTC(2026, 8, 26)), changeFrequency: 'monthly', priority: 0.6 },
  { url: '/local-models', lastModified: new Date(Date.UTC(2026, 8, 26)), changeFrequency: 'monthly', priority: 0.6 },
  { url: '/founder', lastModified: new Date(Date.UTC(2026, 8, 25)), changeFrequency: 'yearly', priority: 0.5 },
  { url: '/security', lastModified: new Date(Date.UTC(2026, 8, 25)), changeFrequency: 'yearly', priority: 0.4 },
  { url: '/stats', lastModified: new Date(Date.UTC(2026, 8, 26)), changeFrequency: 'daily', priority: 0.3 },
  { url: '/terms', lastModified: new Date(Date.UTC(2026, 8, 24)), changeFrequency: 'yearly', priority: 0.3 },
]

function absolute(path: string): string {
  return path === '/' ? `${SITE_URL}/` : `${SITE_URL}${path}`
}

export default function sitemap(): MetadataRoute.Sitemap {
  const entries: MetadataRoute.Sitemap = []
  for (const { translated, ...page } of PAGES) {
    if (!translated) {
      entries.push({ ...page, url: absolute(page.url) })
      continue
    }
    // Every translation of a page lists every other, so a crawler finding one
    // finds them all and knows they are the same page.
    const languages: Record<string, string> = { 'x-default': absolute(page.url) }
    for (const language of LANGUAGE_CODES) languages[language] = absolute(localized(page.url, language))
    for (const language of LANGUAGE_CODES) {
      entries.push({ ...page, url: absolute(localized(page.url, language)), alternates: { languages } })
    }
  }
  return entries
}
