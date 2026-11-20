const CANONICAL_ORIGIN = 'https://noah.asherin.com'

// VERCEL_ENV is fixed at build time: 'production' for the deployment behind
// the custom domain, 'preview' for branch and pull request deployments.
const vercelEnvironment = process.env.VERCEL_ENV

// Everything the site does not use is switched off, so nothing embedded or
// injected can ask for it either.
const PERMISSIONS_POLICY = [
  'accelerometer=()',
  'attribution-reporting=()',
  'autoplay=()',
  'browsing-topics=()',
  'camera=()',
  'compute-pressure=()',
  'display-capture=()',
  'encrypted-media=()',
  'fullscreen=()',
  'gamepad=()',
  'geolocation=()',
  'gyroscope=()',
  'hid=()',
  'identity-credentials-get=()',
  'idle-detection=()',
  'local-fonts=()',
  'magnetometer=()',
  'microphone=()',
  'midi=()',
  'otp-credentials=()',
  'payment=()',
  'picture-in-picture=()',
  'publickey-credentials-create=()',
  'publickey-credentials-get=()',
  'screen-wake-lock=()',
  'serial=()',
  'storage-access=()',
  'sync-xhr=()',
  'usb=()',
  'window-management=()',
  'xr-spatial-tracking=()',
].join(', ')

const SECURITY_HEADERS = [
  { key: 'Strict-Transport-Security', value: 'max-age=63072000; includeSubDomains; preload' },
  { key: 'X-Content-Type-Options', value: 'nosniff' },
  { key: 'X-Frame-Options', value: 'DENY' },
  // The legacy XSS auditor could itself be abused to leak data; 0 turns it off.
  { key: 'X-XSS-Protection', value: '0' },
  { key: 'Referrer-Policy', value: 'strict-origin-when-cross-origin' },
  { key: 'Permissions-Policy', value: PERMISSIONS_POLICY },
  { key: 'Cross-Origin-Opener-Policy', value: 'same-origin' },
  { key: 'Cross-Origin-Resource-Policy', value: 'same-origin' },
  { key: 'Origin-Agent-Cluster', value: '?1' },
  { key: 'X-Permitted-Cross-Domain-Policies', value: 'none' },
  { key: 'X-DNS-Prefetch-Control', value: 'off' },
]

// Files that are not pages carry a policy that lets them do nothing if someone
// opens one directly. Pages get their nonce policy from middleware.ts; the two
// must never overlap, because two policies on one response both apply.
const LOCKED_DOWN_CSP = "default-src 'none'; frame-ancestors 'none'; base-uri 'none'; form-action 'none'; sandbox"

/** @type {import('next').NextConfig} */
const nextConfig = {
  poweredByHeader: false,
  images: { unoptimized: false },
  experimental: {
    // Pages render per request (for the CSP nonce), so the files the founder
    // page and the wallpaper picker look for at render time must ship with
    // the server functions, not only with the CDN copy of public/.
    outputFileTracingIncludes: {
      '/**': ['./public/wallpapers/**/*', './public/founder.jpg', './public/downloads/latest.json'],
    },
  },

  async headers() {
    return [
      { source: '/:path*', headers: SECURITY_HEADERS },

      // Share cards and icons are fetched by other sites and apps.
      {
        source: '/:file(og-image\\.jpg|icon\\.png|icon-192\\.png|apple-touch-icon\\.png|favicon\\.ico)',
        headers: [{ key: 'Cross-Origin-Resource-Policy', value: 'cross-origin' }],
      },

      // Wallpapers and the founder photo stay out of image search, and the
      // site-wide same-origin CORP stops other sites hotlinking them.
      {
        source: '/:file(wallpapers/.+|wallpaper\\.jpg|founder\\.jpg)',
        headers: [{ key: 'X-Robots-Tag', value: 'noindex, noimageindex, noarchive' }],
      },

      // Installers are opened by top-level navigation, which CORP does not
      // govern; same-site still stops other origins embedding them as
      // subresources. The updater is not a browser and is unaffected.
      {
        source: '/downloads/:path*',
        headers: [
          { key: 'Cross-Origin-Resource-Policy', value: 'same-site' },
          { key: 'Content-Security-Policy', value: LOCKED_DOWN_CSP },
          { key: 'X-Robots-Tag', value: 'noindex' },
          // The file names stay the same from release to release, so caches
          // must check back often rather than hold on to an old build.
          { key: 'Cache-Control', value: 'public, max-age=300, must-revalidate' },
        ],
      },
      {
        source: '/downloads/:file(.+\\.(?:exe|deb|tar\\.xz|tar\\.gz))',
        headers: [{ key: 'Content-Disposition', value: 'attachment' }],
      },
      {
        source: '/downloads/:file(.+\\.sha256)',
        headers: [{ key: 'Content-Type', value: 'text/plain; charset=utf-8' }],
      },
      {
        source: '/downloads/latest.json',
        headers: [{ key: 'Cache-Control', value: 'public, max-age=60, must-revalidate' }],
      },

      // noah shield's packages and signed feeds. The extension fetches the
      // feeds with its host permission, so no CORS is needed; caches must
      // check back often because the file names never change.
      {
        source: '/shield/:file(.+\\.(?:zip|xpi|json|sig|sha256))',
        headers: [
          { key: 'Cross-Origin-Resource-Policy', value: 'cross-origin' },
          { key: 'Content-Security-Policy', value: LOCKED_DOWN_CSP },
          { key: 'X-Robots-Tag', value: 'noindex' },
          { key: 'Cache-Control', value: 'public, max-age=300, must-revalidate' },
        ],
      },
      {
        source: '/shield/:file(.+\\.(?:zip|xpi))',
        headers: [{ key: 'Content-Disposition', value: 'attachment' }],
      },
      {
        source: '/shield/:file(.+\\.(?:sig|sha256))',
        headers: [{ key: 'Content-Type', value: 'text/plain; charset=utf-8' }],
      },
      // Firefox installs an add-on straight from a page only when the file
      // arrives as application/x-xpinstall.
      {
        source: '/shield/:file(.+\\.xpi)',
        headers: [{ key: 'Content-Type', value: 'application/x-xpinstall' }],
      },

      {
        source: '/.well-known/:path*',
        headers: [
          { key: 'Content-Security-Policy', value: LOCKED_DOWN_CSP },
          { key: 'Cache-Control', value: 'public, max-age=3600' },
        ],
      },

      // Previews can be shared for review but must never be indexed.
      ...(vercelEnvironment === 'preview'
        ? [{ source: '/:path*', headers: [{ key: 'X-Robots-Tag', value: 'noindex, nofollow' }] }]
        : []),
    ]
  },

  async redirects() {
    // Only the production build sends *.vercel.app to the real domain.
    // Preview deployments live on *.vercel.app too and must stay reachable.
    if (vercelEnvironment !== 'production') return []
    return [
      {
        source: '/:path*',
        has: [{ type: 'host', value: '(?<vercelHost>.+)\\.vercel\\.app' }],
        destination: `${CANONICAL_ORIGIN}/:path*`,
        permanent: true,
      },
    ]
  },
}

export default nextConfig
