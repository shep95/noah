import type { MetadataRoute } from 'next'
import { AI_CRAWLERS } from '@/lib/bots'
import { SITE_URL } from '@/lib/site'

// Preview deployments keep crawlers out entirely; only production is indexed.
export default function robots(): MetadataRoute.Robots {
  if (process.env.VERCEL_ENV === 'preview') {
    return { rules: { userAgent: '*', disallow: '/' } }
  }
  return {
    rules: [
      // AI-training crawlers may not read or copy anything here.
      { userAgent: AI_CRAWLERS, disallow: '/' },
      // Search engines index the pages; files and images stay out of results.
      { userAgent: '*', allow: '/', disallow: ['/downloads/', '/wallpapers/', '/founder.jpg'] },
    ],
    sitemap: `${SITE_URL}/sitemap.xml`,
  }
}
