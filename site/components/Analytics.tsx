'use client'

import { useEffect } from 'react'
import { usePathname } from 'next/navigation'

// First-party counting, without cookies, ip addresses or third parties. Each
// page view sends one small beacon to /api/hit, and each click on a download
// or an outside link sends another. What is counted is on /stats.
const SEEN_KEY = 'noah-seen'

type Beacon = Record<string, string | boolean>

function send(payload: Beacon) {
  const body = JSON.stringify(payload)
  try {
    if (navigator.sendBeacon && navigator.sendBeacon('/api/hit', new Blob([body], { type: 'application/json' }))) return
  } catch {
    // Some browsers refuse beacons with a typed blob; fetch below covers them.
  }
  fetch('/api/hit', { method: 'POST', body, keepalive: true, headers: { 'content-type': 'application/json' } }).catch(() => {})
}

export default function Analytics({ language }: { language: string }) {
  const pathname = usePathname() || '/'

  useEffect(() => {
    if (navigator.webdriver) return
    let returning = false
    try {
      returning = window.localStorage.getItem(SEEN_KEY) === '1'
      window.localStorage.setItem(SEEN_KEY, '1')
    } catch {
      returning = false
    }
    const params = new URLSearchParams(window.location.search)
    send({
      kind: 'view',
      path: pathname,
      referrer: document.referrer || '',
      source: params.get('utm_source') || params.get('ref') || '',
      language,
      returning,
      device: window.innerWidth < 700 ? 'phone' : 'desktop',
    })
  }, [pathname, language])

  useEffect(() => {
    const onClick = (event: MouseEvent) => {
      const target = event.target as Element | null
      const anchor = target?.closest?.('a[href]') as HTMLAnchorElement | null
      if (!anchor) return
      const href = anchor.getAttribute('href') || ''
      const outside = /^https?:\/\//i.test(href) && !href.startsWith(window.location.origin)
      const download = href.startsWith('/downloads/')
      if (!outside && !download) return
      send({ kind: 'click', path: pathname, target: href })
    }
    document.addEventListener('click', onClick, true)
    return () => document.removeEventListener('click', onClick, true)
  }, [pathname])

  return null
}
