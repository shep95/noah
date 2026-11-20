'use client'

import { useEffect, useState } from 'react'
import { SHIELD_STORES } from '@/lib/site'

// One button for the visitor's own browser. Browsers only allow a one-click
// install from their own stores, so the button goes to the store listing as
// soon as one exists (lib/site.ts SHIELD_STORES); until then it downloads
// the package and opens the shortest honest path to a working install.
type Browser = 'chrome' | 'edge' | 'brave' | 'opera' | 'vivaldi' | 'arc' | 'chromium' | 'firefox' | 'safari' | 'other'

const NAMES: Record<Browser, string> = {
  chrome: 'Chrome',
  edge: 'Edge',
  brave: 'Brave',
  opera: 'Opera',
  vivaldi: 'Vivaldi',
  arc: 'Arc',
  chromium: 'your browser',
  firefox: 'Firefox',
  safari: 'Safari',
  other: 'your browser',
}

const EXTENSIONS_PAGE: Partial<Record<Browser, string>> = {
  chrome: 'chrome://extensions',
  edge: 'edge://extensions',
  brave: 'brave://extensions',
  opera: 'opera://extensions',
  vivaldi: 'vivaldi://extensions',
  arc: 'arc://extensions',
  chromium: 'chrome://extensions',
}

function detectBrowser(): Browser {
  const agent = navigator.userAgent
  const brands = ((navigator as Navigator & { userAgentData?: { brands?: { brand: string }[] } }).userAgentData?.brands || []).map((entry) => entry.brand.toLowerCase())
  const nav = navigator as Navigator & { brave?: { isBrave?: () => Promise<boolean> } }
  if (/Firefox\//.test(agent)) return 'firefox'
  if (/Safari\//.test(agent) && !/Chrom(e|ium)\//.test(agent)) return 'safari'
  if (nav.brave || brands.includes('brave')) return 'brave'
  if (/Edg\//.test(agent) || brands.some((brand) => brand.includes('edge'))) return 'edge'
  if (/OPR\//.test(agent) || brands.includes('opera')) return 'opera'
  if (/Vivaldi\//.test(agent)) return 'vivaldi'
  if (/Chrome\//.test(agent) && !/Chromium\//.test(agent) && brands.includes('google chrome')) return 'chrome'
  if (/Chrome\//.test(agent)) return 'chromium'
  return 'other'
}

function isChromium(browser: Browser) {
  return ['chrome', 'edge', 'brave', 'opera', 'vivaldi', 'arc', 'chromium'].includes(browser)
}

export default function AddShield() {
  const [browser, setBrowser] = useState<Browser>('other')
  const [open, setOpen] = useState(false)
  const [copied, setCopied] = useState(false)
  useEffect(() => setBrowser(detectBrowser()), [])

  const name = NAMES[browser]
  const store = isChromium(browser)
    ? browser === 'edge' && SHIELD_STORES.edge
      ? SHIELD_STORES.edge
      : SHIELD_STORES.chrome
    : browser === 'firefox'
      ? SHIELD_STORES.firefox
      : browser === 'safari'
        ? SHIELD_STORES.safari
        : ''
  const file = browser === 'firefox' ? (SHIELD_STORES.firefoxSigned ? 'noah-shield-firefox.xpi' : 'noah-shield-firefox.zip') : browser === 'safari' ? 'noah-shield-safari.zip' : 'noah-shield-chromium.zip'
  const instant = Boolean(store) || (browser === 'firefox' && SHIELD_STORES.firefoxSigned)

  const copyAddress = async () => {
    const address = EXTENSIONS_PAGE[browser] || 'chrome://extensions'
    try {
      await navigator.clipboard.writeText(address)
      setCopied(true)
      setTimeout(() => setCopied(false), 2500)
    } catch {
      setCopied(false)
    }
  }

  return (
    <div className="l-shield-add">
      <div className="l-cta-row">
        {instant ? (
          <a href={store || `/shield/${file}`} className="l-btn-primary" rel="noopener noreferrer">
            Add to {name}
          </a>
        ) : (
          <a
            href={`/shield/${file}`}
            download
            className="l-btn-primary"
            onClick={() => setOpen(true)}
          >
            Add to {name}
          </a>
        )}
        <a href="#every-browser" className="l-btn-ghost">
          every browser
        </a>
      </div>
      {instant ? (
        <p className="l-trust">one click: the store installs it and keeps it updated</p>
      ) : (
        <p className="l-trust">
          {browser === 'firefox'
            ? 'the store listing is under review; until it is live, Firefox takes the package as a temporary add-on'
            : browser === 'safari'
              ? 'Safari needs the App Store listing or an Xcode build; both are in progress'
              : 'the store listing is under review; until it is live, three steps put it in place'}
        </p>
      )}

      {open && !instant && isChromium(browser) && (
        <ol className="l-shield-steps">
          <li>
            The package is downloading. Unzip it somewhere you will keep it (the browser reads it from there).
          </li>
          <li>
            Open a new tab and paste{' '}
            <code>{EXTENSIONS_PAGE[browser] || 'chrome://extensions'}</code>{' '}
            <button type="button" className="l-btn-ghost" onClick={copyAddress}>
              {copied ? 'copied' : 'copy the address'}
            </button>
            . Browsers do not let a website open that page for you.
          </li>
          <li>
            Turn on <em>Developer mode</em> (top right), press <em>Load unpacked</em>, and pick the unzipped folder.
            The shield opens this page again and starts working at once.
          </li>
        </ol>
      )}
      {open && !instant && browser === 'firefox' && (
        <ol className="l-shield-steps">
          <li>The package is downloading.</li>
          <li>
            Open a new tab, paste <code>about:debugging#/runtime/this-firefox</code>, press <em>Load Temporary Add-on</em> and pick the zip.
          </li>
          <li>
            Firefox forgets temporary add-ons when it closes. The signed copy from addons.mozilla.org installs in one click and stays; it is under review.
          </li>
        </ol>
      )}
      {open && !instant && browser === 'safari' && (
        <ol className="l-shield-steps">
          <li>The package is downloading.</li>
          <li>
            On a Mac with Xcode, unzip it and run <code>xcrun safari-web-extension-converter noah-shield-safari</code>.
          </li>
          <li>Run the project once, then turn the shield on under Safari → Settings → Extensions.</li>
        </ol>
      )}
    </div>
  )
}
