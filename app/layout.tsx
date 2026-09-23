import type { Metadata, Viewport } from 'next'
import './globals.css'

const BASE = process.env.NEXT_PUBLIC_APP_URL || 'https://unlocket.vercel.app'

export const metadata: Metadata = {
  title: {
    default: 'unlocket — ai code editor by #houseofasher',
    template: '%s | unlocket',
  },
  description:
    'an open, free ai-powered code editor. powered by shepherd — a pattern-reading intelligence that learns your codebase, matches your aesthetic, and codes with you. no sign-up. no paywall.',
  keywords: [
    'ai code editor', 'free ide', 'ai coding assistant', 'shepherd ai',
    'houseofasher', 'venice ai', 'online ide', 'browser ide', 'code editor',
    'ai pair programmer', 'github import', 'free coding tool',
  ],
  authors: [{ name: '#houseofasher', url: 'https://asherin.com' }],
  creator: '#houseofasher',
  publisher: '#houseofasher',
  metadataBase: new URL(BASE),
  openGraph: {
    type: 'website',
    locale: 'en_US',
    url: '/',
    siteName: 'unlocket',
    title: 'unlocket — ai code editor',
    description:
      'a free, open ai code editor powered by shepherd intelligence. no sign-up. no paywall.',
    images: [
      {
        url: '/og-image.jpg',
        width: 1674,
        height: 939,
        alt: 'unlocket — ai code editor by #houseofasher',
      },
    ],
  },
  twitter: {
    card: 'summary_large_image',
    title: 'unlocket — ai code editor',
    description:
      'a free, open ai code editor powered by shepherd intelligence. no sign-up. no paywall.',
    creator: '@houseofasher',
    images: ['/og-image.jpg'],
  },
  robots: {
    index: true,
    follow: true,
    googleBot: { index: true, follow: true, 'max-image-preview': 'large' },
  },
  icons: {
    icon: [
      { url: '/icon-192.png', sizes: '192x192', type: 'image/png' },
      { url: '/icon.png', sizes: '512x512', type: 'image/png' },
    ],
    apple: '/apple-touch-icon.png',
    shortcut: '/icon-192.png',
  },
}

export const viewport: Viewport = {
  width: 'device-width',
  initialScale: 1,
  themeColor: '#080c08',
}

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="en" suppressHydrationWarning>
      <head>
        <link rel="preconnect" href="https://fonts.googleapis.com" />
        <link rel="preconnect" href="https://fonts.gstatic.com" crossOrigin="anonymous" />
        <link
          href="https://fonts.googleapis.com/css2?family=Inter:wght@300;400;500&family=JetBrains+Mono:wght@300;400;500&display=swap"
          rel="stylesheet"
        />
        <link rel="canonical" href={BASE} />
        <script
          type="application/ld+json"
          dangerouslySetInnerHTML={{
            __html: JSON.stringify({
              '@context': 'https://schema.org',
              '@type': 'SoftwareApplication',
              name: 'unlocket',
              applicationCategory: 'DeveloperApplication',
              offers: { '@type': 'Offer', price: '0', priceCurrency: 'USD' },
              operatingSystem: 'Web',
              description: 'free ai-powered code editor with shepherd intelligence',
              author: {
                '@type': 'Organization',
                name: '#houseofasher',
                url: 'https://asherin.com',
              },
            }),
          }}
        />
      </head>
      <body>{children}</body>
    </html>
  )
}
