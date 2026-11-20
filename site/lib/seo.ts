import type { Metadata } from 'next'
import {
  FEATURES,
  LINKS,
  LONG_DESCRIPTION,
  RELEASE_DATE,
  RELEASE_VERSION,
  SITE_URL,
} from './site'
import { availableInstallers } from './installers'
import {
  DEFAULT_LANGUAGE,
  LANGUAGE_CODES,
  type Language,
  languageAlternates,
  localized,
  openGraphLocale,
} from './i18n'

/** HowTo structured data for a guide page. */
export function howTo(guide: { path: string; name: string; description: string; steps: { name: string; text: string }[] }) {
  return {
    '@context': 'https://schema.org',
    '@type': 'HowTo',
    name: guide.name,
    description: guide.description,
    url: absolute(guide.path),
    step: guide.steps.map((step, index) => ({
      '@type': 'HowToStep',
      position: index + 1,
      name: step.name,
      text: step.text,
    })),
  }
}

export const SITE_NAME = 'noah'
export const ORGANIZATION_NAME = 'house of asher'
export const FOUNDER_NAME = 'asher shepherd newton'
export const TWITTER_HANDLE = '@house_ofasher'
export const HOME_TITLE = 'noah — free ai code editor with shepherd, its agent'

export const OG_IMAGE = {
  url: '/og-image.jpg',
  width: 1200,
  height: 630,
  alt: "noah — a shepherd for your code. download for windows, over a cabin in misty green hills",
}

const ROBOTS: Metadata['robots'] = {
  index: true,
  follow: true,
  googleBot: { index: true, follow: true, 'max-image-preview': 'large', 'max-snippet': -1 },
}

type PageSeo = {
  path: string
  title: string
  description: string
  /** Use `title` as the whole document title instead of filling the template. */
  absoluteTitle?: boolean
  openGraphType?: 'website' | 'profile'
  /**
   * The language this page is served in. A page translated whole names its
   * own localized url as canonical and lists every language as an alternate;
   * a page whose body is still English stays canonical to the English url,
   * whatever frame it is shown in.
   */
  language?: Language
  translated?: boolean
}

// A page's openGraph and twitter objects replace the layout's rather than
// merging with them, so every page states its image and card type in full.
export function pageMetadata({
  path,
  title,
  description,
  absoluteTitle = false,
  openGraphType = 'website',
  language = DEFAULT_LANGUAGE,
  translated = false,
}: PageSeo): Metadata {
  const shareTitle = absoluteTitle ? title : `${title} · ${SITE_NAME}`
  const canonical = translated ? localized(path, language) : path
  return {
    title: absoluteTitle ? { absolute: title } : title,
    description,
    alternates: translated ? { canonical, languages: languageAlternates(path) } : { canonical },
    robots: ROBOTS,
    openGraph: {
      type: openGraphType,
      locale: openGraphLocale(language),
      alternateLocale: translated
        ? LANGUAGE_CODES.filter((other) => other !== language).map(openGraphLocale)
        : undefined,
      url: canonical,
      siteName: SITE_NAME,
      title: shareTitle,
      description,
      images: [OG_IMAGE],
    },
    twitter: {
      card: 'summary_large_image',
      site: TWITTER_HANDLE,
      creator: TWITTER_HANDLE,
      title: shareTitle,
      description,
      images: [{ url: OG_IMAGE.url, alt: OG_IMAGE.alt }],
    },
  }
}

const ORGANIZATION_ID = `${SITE_URL}/#organization`
const WEBSITE_ID = `${SITE_URL}/#website`
const SOFTWARE_ID = `${SITE_URL}/#software`
export const FOUNDER_ID = `${SITE_URL}/founder#person`

function absolute(path: string): string {
  return path === '/' ? `${SITE_URL}/` : `${SITE_URL}${path}`
}

/** Who makes noah, the site, and the app itself, for every page. */
export function siteGraph() {
  return {
    '@context': 'https://schema.org',
    '@graph': [
      {
        '@type': 'Organization',
        '@id': ORGANIZATION_ID,
        name: ORGANIZATION_NAME,
        alternateName: '#houseofasher',
        url: LINKS.asherin,
        logo: {
          '@type': 'ImageObject',
          url: `${SITE_URL}/icon.png`,
          width: 512,
          height: 512,
        },
        founder: { '@type': 'Person', '@id': FOUNDER_ID, name: FOUNDER_NAME },
        sameAs: [LINKS.twitter, LINKS.instagram, LINKS.discord, LINKS.source],
      },
      {
        '@type': 'WebSite',
        '@id': WEBSITE_ID,
        url: absolute('/'),
        name: SITE_NAME,
        description: LONG_DESCRIPTION,
        publisher: { '@id': ORGANIZATION_ID },
        inLanguage: 'en',
      },
      {
        '@type': 'SoftwareApplication',
        '@id': SOFTWARE_ID,
        name: SITE_NAME,
        url: absolute('/'),
        description: LONG_DESCRIPTION,
        applicationCategory: 'DeveloperApplication',
        applicationSubCategory: 'ai code editor',
        operatingSystem: 'Windows 10, Windows 11, Linux',
        softwareVersion: RELEASE_VERSION,
        dateModified: RELEASE_DATE.toISOString().slice(0, 10),
        downloadUrl: availableInstallers().map((installer) => `${SITE_URL}/downloads/${installer.file}`),
        installUrl: absolute('/download'),
        featureList: FEATURES,
        image: `${SITE_URL}${OG_IMAGE.url}`,
        license: 'https://www.gnu.org/licenses/gpl-3.0.html',
        isAccessibleForFree: true,
        offers: {
          '@type': 'Offer',
          price: '0',
          priceCurrency: 'USD',
          url: absolute('/download'),
        },
        author: { '@id': ORGANIZATION_ID },
        publisher: { '@id': ORGANIZATION_ID },
      },
    ],
  }
}

export function breadcrumbs(trail: { name: string; path: string }[]) {
  return {
    '@context': 'https://schema.org',
    '@type': 'BreadcrumbList',
    itemListElement: [{ name: SITE_NAME, path: '/' }, ...trail].map((crumb, index) => ({
      '@type': 'ListItem',
      position: index + 1,
      name: crumb.name,
      item: absolute(crumb.path),
    })),
  }
}

export function faqPage(questions: { question: string; answer: string }[]) {
  return {
    '@context': 'https://schema.org',
    '@type': 'FAQPage',
    url: absolute('/faq'),
    mainEntity: questions.map(({ question, answer }) => ({
      '@type': 'Question',
      name: question,
      acceptedAnswer: { '@type': 'Answer', text: answer },
    })),
  }
}

export function founderProfile({ photo }: { photo: string | null }) {
  return {
    '@context': 'https://schema.org',
    '@type': 'ProfilePage',
    url: absolute('/founder'),
    mainEntity: {
      '@type': 'Person',
      '@id': FOUNDER_ID,
      name: FOUNDER_NAME,
      jobTitle: 'prompt engineer',
      url: absolute('/founder'),
      ...(photo ? { image: `${SITE_URL}${photo}` } : {}),
      worksFor: { '@id': ORGANIZATION_ID },
      sameAs: [LINKS.asherin, LINKS.instagram, LINKS.twitter],
    },
  }
}
