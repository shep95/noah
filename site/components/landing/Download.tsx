'use client'

import { useEffect, useState } from 'react'
import Link from 'next/link'
import { RELEASE_VERSION, type Installer } from '@/lib/site'
import { type Dictionary, type Language, fill, localized } from '@/lib/i18n'

// Installers are hosted on this site (public/downloads), so the button pulls
// the file straight from here. The button offers the visitor's own platform
// when there is a build for it; a platform without one is told so plainly,
// never shown a "coming soon" placeholder.
type Platform = Installer['platform']

const PLATFORM_NAMES: Record<Platform, string> = { windows: 'windows', linux: 'linux', mac: 'mac' }

function detectPlatform(): Platform | null {
  const agent = navigator.userAgent.toLowerCase()
  const platform = (navigator.platform || '').toLowerCase()
  if (/mac|iphone|ipad|ipod/.test(agent) || platform.startsWith('mac')) return 'mac'
  if (/win/.test(agent) || platform.startsWith('win')) return 'windows'
  if (/linux|x11/.test(agent) && !/android/.test(agent)) return 'linux'
  return null
}

function DownloadIcon() {
  return (
    <svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="2.2" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <path d="M21 15v4a2 2 0 01-2 2H5a2 2 0 01-2-2v-4" />
      <polyline points="7 10 12 15 17 10" />
      <line x1="12" y1="15" x2="12" y2="3" />
    </svg>
  )
}

export default function Download({
  language,
  t,
  installers,
}: {
  language: Language
  t: Dictionary['download']
  installers: Installer[]
}) {
  const [visitor, setVisitor] = useState<Platform | null>(null)
  useEffect(() => setVisitor(detectPlatform()), [])

  const forVisitor = installers.find((installer) => installer.platform === visitor)
  const primary = forVisitor ?? installers[0]

  return (
    <div>
      {visitor && !forVisitor && (
        <p className="l-download-note">{fill(t.none, { platform: PLATFORM_NAMES[visitor] })}</p>
      )}
      <div className="l-cta-row">
        <a href={`/downloads/${primary.file}`} download className="l-btn-primary">
          <DownloadIcon />
          {fill(t.forPlatform, { platform: PLATFORM_NAMES[primary.platform] })}
        </a>
        <Link href={localized('/download', language)} className="l-btn-ghost">
          {t.all}
        </Link>
      </div>
      <p className="l-trust">
        v{RELEASE_VERSION} · {primary.size.toLowerCase()} · {primary.requirement}
      </p>
    </div>
  )
}
