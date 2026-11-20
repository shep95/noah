/** The one origin search engines should know noah's site by. */
export const CANONICAL_ORIGIN = 'https://noah.asherin.com'

// Canonical links, the sitemap and social cards always name the production
// domain, even on preview deployments, so a preview URL can never become the
// canonical. NEXT_PUBLIC_APP_URL is honoured only by `next dev`, for trying
// the metadata against a local origin.
function resolveSiteUrl(): string {
  const configured = process.env.NEXT_PUBLIC_APP_URL
  if (configured && process.env.NODE_ENV === 'development') return configured.replace(/\/$/, '')
  return CANONICAL_ORIGIN
}

export const SITE_URL = resolveSiteUrl()

export const LINKS = {
  discord: 'https://discord.gg/M9hnebRwvk',
  asherin: 'https://asherin.com/',
  source: 'https://github.com/shep95/noah',
  instagram: 'https://www.instagram.com/asher_united/',
  twitter: 'https://x.com/house_ofasher',
}

export type Installer = {
  platform: 'windows' | 'linux' | 'mac'
  label: string
  file: string
  format: string
  size: string
  /** What the installer runs on, shown under the download button. */
  requirement: string
  sha256: string
}

/** The published release, matching `version` in public/downloads/latest.json. */
export const RELEASE_VERSION = '2026.10.2'

// Every installer the release pipeline can produce; lib/installers.ts keeps
// the pages to the ones published so far.
export const INSTALLERS: Installer[] = [
  {
    platform: 'windows',
    label: 'Windows 10 and 11 (64-bit)',
    file: 'noah-windows-x86_64.exe',
    format: 'installer (.exe)',
    size: '98 MB',
    requirement: 'windows 10+',
    sha256: '78bfe3f524015fbbc0706b961be2f52991b9fd80f0cb40e81d140f715f511b02',
  },
  {
    platform: 'linux',
    label: 'Debian, Ubuntu, Mint, Pop!_OS (64-bit)',
    file: 'noah-linux-amd64.deb',
    format: 'package (.deb)',
    size: '96 MB',
    requirement: 'ubuntu 22.04+ / debian 12+',
    sha256: '0150e9ce80f4851402997b4c31913a9cb57744b89583009c8eb81a72d54ff50c',
  },
  {
    platform: 'linux',
    label: 'Any other Linux (x86_64)',
    file: 'noah-linux-x86_64.tar.xz',
    format: 'archive (.tar.xz)',
    size: '96 MB',
    requirement: 'x86_64 linux',
    sha256: 'a61ff02e074a0e2600c41021f5afff531807eb82bb6d3e3292fef7a7330aae4a',
  },
  // The macOS disk images are built by the release workflow on GitHub's Mac
  // runners. Until the first one is published they are absent from
  // latest.json and lib/installers.ts leaves them off the pages.
  {
    platform: 'mac',
    label: 'Mac with Apple Silicon (M1 and later)',
    file: 'noah-mac-aarch64.dmg',
    format: 'disk image (.dmg)',
    size: '0 MB',
    requirement: 'macos 10.15+',
    sha256: '0000000000000000000000000000000000000000000000000000000000000000',
  },
  {
    platform: 'mac',
    label: 'Mac with an Intel processor',
    file: 'noah-mac-x86_64.dmg',
    format: 'disk image (.dmg)',
    size: '0 MB',
    requirement: 'macos 10.15+',
    sha256: '0000000000000000000000000000000000000000000000000000000000000000',
  },
]

/** The search-result description: kept under ~160 characters so it isn't cut. */
export const DESCRIPTION =
  'noah is a free ai code editor for windows and linux. shepherd, its agent, reads your whole project first. nearly fifty providers on your own key, or local models.'

/** The longer account, for structured data and share cards. */
export const LONG_DESCRIPTION =
  'noah is a free ai code editor for windows and linux, built on zed. shepherd, its agent, reads your whole project before it writes, searches the web when it has to, and reasons as deeply as you ask. bring your own key for nearly fifty providers, western and chinese, or run models locally. no account, no paywall, no telemetry.'

export const RELEASE_DATE = releaseDate(RELEASE_VERSION)

/** `2026.9.25` names the day it shipped. */
function releaseDate(version: string): Date {
  const [year, month, day] = version.split('.').map(Number)
  return new Date(Date.UTC(year, month - 1, day))
}

/** What noah does, for the SoftwareApplication feature list. */
export const FEATURES = [
  'shepherd, an agent that reads your whole project before it writes',
  'web search and adjustable reasoning levels',
  'nearly fifty bring-your-own-key ai providers, western and chinese, plus local models',
  'api keys kept in the system keychain',
  'asherin.chat, for thinking with shepherd outside a project',
  'asherin.pages, for pdfs, digital books and slideshows',
  'asherin.eye, a live 3d globe intelligence console you can extend',
  'a device security room: security checks, health, a system-wide ad blocker and a duplicate-file cleaner',
  'a browser room shared with shepherd',
  'chat history with pinning',
  'ed25519-signed automatic updates',
  'a trust layer: evidence, provenance, secret redaction and prompt-injection screening',
  'a sandbox for the commands shepherd runs',
  'wallpapers the interface takes its palette from',
]

/** Where the wallpaper picker remembers the visitor's choice. */
export const WALLPAPER_STORAGE_KEY = 'noah-wallpaper'

// Wallpapers with a hand-made look in app/landing.css: its own palette, type
// and layout. Any other image in public/wallpapers gets a palette sampled
// from the image and keeps the fog look's type and layout.
export const THEMED_LOOKS = ['fog', 'glitch', 'halo', 'lights', 'meteor', 'rings', 'sea'] as const

// noah shield's store listings. A non-empty address turns the "Add to …"
// button on /shield into a one-click install from that store; empty means
// the listing is not live yet and the page offers the package instead.
// firefoxSigned: true once public/shield/noah-shield-firefox.xpi carries an
// addons.mozilla.org signature, which lets Firefox install it from here.
export const SHIELD_STORES = {
  chrome: '',
  edge: '',
  firefox: '',
  safari: '',
  firefoxSigned: false,
}
