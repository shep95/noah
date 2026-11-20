import type { MetadataRoute } from 'next'

export default function manifest(): MetadataRoute.Manifest {
  return {
    id: '/',
    name: 'noah — free ai code editor',
    short_name: 'noah',
    description: 'a free ai code editor for windows and linux, with shepherd, its agent. no account, no paywall.',
    lang: 'en',
    dir: 'ltr',
    start_url: '/',
    scope: '/',
    display: 'standalone',
    background_color: '#070909',
    theme_color: '#070909',
    categories: ['developer', 'productivity', 'utilities'],
    icons: [
      { src: '/icon-192.png', sizes: '192x192', type: 'image/png', purpose: 'any' },
      { src: '/icon.png', sizes: '512x512', type: 'image/png', purpose: 'any' },
    ],
  }
}
