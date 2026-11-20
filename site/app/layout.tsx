import type { Metadata, Viewport } from 'next'
import { headers } from 'next/headers'
import './globals.css'
import { lookFontVariables } from './fonts'
import JsonLd from '@/components/JsonLd'
import { DESCRIPTION, LINKS, LONG_DESCRIPTION, SITE_URL, THEMED_LOOKS, WALLPAPER_STORAGE_KEY } from '@/lib/site'
import { HOME_TITLE, OG_IMAGE, ORGANIZATION_NAME, SITE_NAME, TWITTER_HANDLE, siteGraph } from '@/lib/seo'
import { LANGUAGE_CODES, LANGUAGE_STORAGE_KEY, RIGHT_TO_LEFT } from '@/lib/i18n'
import { currentLanguage } from '@/lib/language'

// Every page is rendered per request so middleware can give its scripts a
// fresh CSP nonce; a prerendered page would carry no nonce and could only run
// under 'unsafe-inline'.
export const dynamic = 'force-dynamic'

export const metadata: Metadata = {
  metadataBase: new URL(SITE_URL),
  title: {
    default: HOME_TITLE,
    template: '%s · noah',
  },
  description: DESCRIPTION,
  applicationName: SITE_NAME,
  keywords: [
    'free ai code editor',
    'ai code editor',
    'ai ide',
    'ai coding agent',
    'bring your own key',
    'byok ai editor',
    'deepseek code editor',
    'qwen code editor',
    'local llm code editor',
    'ollama code editor',
    'cursor alternative',
    'zed fork',
    'private ai ide',
    'shepherd agent',
    'house of asher',
  ],
  authors: [{ name: ORGANIZATION_NAME, url: LINKS.asherin }],
  creator: ORGANIZATION_NAME,
  publisher: ORGANIZATION_NAME,
  category: 'technology',
  alternates: { canonical: '/' },
  openGraph: {
    type: 'website',
    locale: 'en_US',
    url: '/',
    siteName: SITE_NAME,
    title: HOME_TITLE,
    description: LONG_DESCRIPTION,
    images: [OG_IMAGE],
  },
  twitter: {
    card: 'summary_large_image',
    site: TWITTER_HANDLE,
    creator: TWITTER_HANDLE,
    title: HOME_TITLE,
    description: DESCRIPTION,
    images: [{ url: OG_IMAGE.url, alt: OG_IMAGE.alt }],
  },
  robots: {
    index: true,
    follow: true,
    googleBot: { index: true, follow: true, 'max-image-preview': 'large', 'max-snippet': -1 },
  },
  formatDetection: { telephone: false, email: false, address: false },
  icons: {
    icon: [
      { url: '/favicon.ico', sizes: 'any' },
      { url: '/icon-192.png', sizes: '192x192', type: 'image/png' },
      { url: '/icon.png', sizes: '512x512', type: 'image/png' },
    ],
    apple: '/apple-touch-icon.png',
  },
}

export const viewport: Viewport = {
  width: 'device-width',
  initialScale: 1,
  themeColor: '#070909',
  colorScheme: 'dark',
}

// Runs before the first paint so a visitor who chose a look never sees the
// default one flash first. The picker keeps the attribute in step afterwards.
// It also preloads only the chosen look's wallpaper: a fixed preload of the
// default one goes unused under every other look, and the browser warns.
const LOOK_SCRIPT = `var l=null;try{l=localStorage.getItem(${JSON.stringify(WALLPAPER_STORAGE_KEY)})}catch(e){}if(!l)l='fog';if(${JSON.stringify(THEMED_LOOKS)}.indexOf(l)>-1){if(l!=='fog')document.documentElement.setAttribute('data-look',l);var p=document.createElement('link');p.rel='preload';p.as='image';p.href=l==='fog'?'/wallpaper.jpg':'/wallpapers/'+l+'.jpg';p.setAttribute('fetchpriority','high');document.head.appendChild(p)}`

// A visitor on an English url who has chosen another language, or whose
// browser speaks one the site speaks, is taken to that language before the
// page paints. A remembered "en" is a choice too and stays. Crawlers run no
// script and keep the url they asked for.
const LANGUAGE_SCRIPT = `(function(){var L=${JSON.stringify(LANGUAGE_CODES)};var p=location.pathname;if(L.indexOf(p.split('/')[1])>-1)return;var s=null;try{s=localStorage.getItem(${JSON.stringify(LANGUAGE_STORAGE_KEY)})}catch(e){}var w=s||(navigator.language||'').split('-')[0].toLowerCase();if(w&&w!=='en'&&L.indexOf(w)>-1){location.replace('/'+w+(p==='/'?'':p)+location.search+location.hash)}})()`

export default function RootLayout({ children }: { children: React.ReactNode }) {
  const nonce = headers().get('x-nonce') ?? undefined
  const language = currentLanguage()
  return (
    // The look script sets data-look on this element before React hydrates.
    <html
      lang={language}
      dir={RIGHT_TO_LEFT.includes(language) ? 'rtl' : 'ltr'}
      className={lookFontVariables}
      suppressHydrationWarning
    >
      <head>
        <script nonce={nonce} dangerouslySetInnerHTML={{ __html: LANGUAGE_SCRIPT }} />
        <script nonce={nonce} dangerouslySetInnerHTML={{ __html: LOOK_SCRIPT }} />
        <JsonLd data={siteGraph()} />
      </head>
      <body>{children}</body>
    </html>
  )
}
