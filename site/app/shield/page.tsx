import type { Metadata } from 'next'
import Link from 'next/link'
import JsonLd from '@/components/JsonLd'
import EnglishNote from '@/components/landing/EnglishNote'
import Shell from '@/components/landing/Shell'
import AddShield from '@/components/landing/AddShield'
import { LINKS } from '@/lib/site'
import { breadcrumbs, pageMetadata } from '@/lib/seo'

export const metadata: Metadata = pageMetadata({
  path: '/shield',
  title: 'noah shield, a browser extension',
  description:
    'ads and trackers stopped on every site, clean search results, a one-press vpn through noah that fails closed, a log of everything it did, phishing and scam pages broken, screenshots with your private details blurred, screen recording with your camera in a corner, cheaper prices and working coupons, and a hundred smaller mercies. for chrome, edge, brave, firefox, safari and every other browser.',
})

const DOWNLOADS = [
  { file: 'noah-shield-chromium.zip', label: 'Chrome, Edge, Brave, Opera, Vivaldi, Arc', note: 'every browser built on Chromium' },
  { file: 'noah-shield-firefox.zip', label: 'Firefox', note: 'and browsers built on it' },
  { file: 'noah-shield-safari.zip', label: 'Safari', note: 'converted with Xcode on a Mac' },
]

const GROUPS: { title: string; items: string[] }[] = [
  {
    title: 'vpn',
    items: [
      'one press: with noah on your computer the shield fetches Tor from the Tor Project once, starts it, and takes you out through the country you pick from the list (or the fastest exit); you watch it connect, percent by percent',
      'a list of locations by country; everything the browser does goes out through the place you chose',
      'an encrypted tunnel (HTTPS or SOCKS5) that fails closed: when the location is down, nothing goes out on the plain connection',
      'names resolved by the server, never on your machine; WebRTC held to proxied routes',
      'a server per site, so one site goes out through the US and another through the UK, or straight past the tunnel',
      'speed test across your servers and a one-press pick of the fastest',
      'exit check after connecting, a leak test page (exit address, WebRTC candidates, language and time zone), connect at startup',
      'Tor on your computer built in as the anonymous route; your own servers; a signed list of vetted locations that starts empty on purpose',
    ],
  },
  {
    title: 'ads',
    items: [
      'EasyList’s ad rules in the browser’s own blocking engine, on every site: ad servers, ad scripts, pop-ups, third-party ad frames',
      'the boxes ads leave behind hidden too, with EasyList’s element rules: 13,600 generic ones on every page and the site’s own on top',
      'a second set of 30,000 more rules when your browser has room for it; Google’s ad networks and Google Analytics blocked by the shield’s own rules',
      'a trusted site keeps its layout; the switch turns ads and trackers off together',
    ],
  },
  {
    title: 'light, frequency, the look',
    items: [
      'light by scene: bright and sunny, daytime, restaurant, in a dark room, night; the screen’s real brightness, turned through noah on your computer; nothing laid over pages except a warm tint you ask for',
      'a frequency under whatever plays: a tone from 20 to 963 Hz, or a binaural beat below that (2 Hz, 4 Hz, 7.83 Hz, 10 Hz), with a volume',
      'the look: one of noah’s pictures or your own becomes the palette of the shield’s pages and the new tab page, and on Firefox the browser’s own frame and toolbars; blacks that carry the picture’s temperature, words that always read, one accent from the picture',
    ],
  },
  {
    title: 'search and the log',
    items: [
      'clean results on Google, Bing, DuckDuckGo, Brave, Yahoo, Startpage and Ecosia: paid results gone, known content farms and scraper sites gone, pages written for the engine rather than for you shown faded; a count on the page and three switches',
      'search pages wear the look you chose outright: its picture behind the results, its palette for every word whatever mode the engine was in, the shield’s type and spacing (serif titles, cards with the same radius and rhythm)',
      'a “peek” under each result reads the page right there: fetched once, without cookies or your address in a referrer, its words and picture shown in place, so you open only what earned it',
      'the log: everything the shield stopped, warned about or did, in plain words with the site and the moment; “what happened here” in the popup lists the requests stopped on this page and who was on the other end; every notification opens the log',
    ],
  },
  {
    title: 'capture',
    items: [
      'a screenshot of what is on screen, the whole page stitched top to bottom, or an area you drag out; saved straight to Downloads/noah-shield as PNG, on any tab, even one open since before the shield was installed',
      'private details blurred in the page before the shot: emails, phone and card numbers, IBANs, keys, wallet addresses, street addresses, filled-in personal fields',
      'screen, window or tab recording with your camera in a rounded rectangle in the corner you choose, microphone and system sound mixed; written to your downloads as WebM with no name, date or program tag inside',
    ],
  },
  {
    title: 'data protection',
    items: [
      'EasyPrivacy’s 9,500 tracker rules next to EasyList’s 20,000 ad rules in the browser’s own blocking engine, plus session replay, crypto mining, malvertising and social pixel rules',
      'third-party cookies off, tracker cookies deleted twice a day, a site’s cookies burned when its last tab closes (with a keep list), one-press burn of a site’s cookies, cache and storage',
      'location, notifications, camera and microphone denied or asked per site; hidden autofill fields disarmed; hidden third-party frames removed',
      'tracking parameters stripped from links, redirect wrappers unwrapped (google, facebook, youtube, outlook safelinks, proofpoint), the Referer hidden from other sites, Global Privacy Control sent',
      'tracking pixels blocked in Gmail, Outlook, Yahoo, Proton and Fastmail; sign-in-with-Google/Facebook widgets optional',
      'a privacy score and letter grade for every site, a toolbar meter, and a who’s-watching list with each company, what it does and where it is based',
      'form data leak guard: what you type cannot leave for a third party before you submit; what you typed and deleted neither',
      'typing guard: scripts from other sites that listen to every key you press are named, keystroke streams are stopped, and strict mode refuses them the listener',
      'site trust under the grade: lookalike, plain http, and whether the site has leaked its users’ data before (Have I Been Pwned’s public breach list by domain)',
    ],
  },
  {
    title: 'fingerprint and location',
    items: [
      'canvas, WebGL and audio noise, seeded per site and per rotation period, so two sites cannot match you and one site sees a steady visitor',
      'rotation every session, day or hour, or on a button; cores, memory and battery in round numbers',
      'blend-in mode: Windows, Chrome, 1920×1080, common fonts, matching User-Agent and client hints on every request',
      'fake location: pick a city and every site gets its coordinates, time zone and language, on the page and in the Accept-Language header; or follow the tunnel’s exit country',
      'stealth: the shield’s own page elements carry names a site cannot look for',
    ],
  },
  {
    title: 'security and account safety',
    items: [
      'every site upgraded to https (local networks excepted)',
      'lookalike domains stopped before they load: paypa1.com, amazon-verify.net, login.paypal.com.evil.net, with the real address one press away',
      'a password you used elsewhere typed into a new site is caught; a password on plain http is caught; passwords checked against known leaks with five hex characters of a hash (Have I Been Pwned); emails checked with your own key',
      'risky downloads held until you have read why, with an optional VirusTotal check under your key',
      'the clipboard read only with your yes; the clipboard wiped after copying from a password field; crypto address swaps on copy caught',
      'wallet guard: approvals, signatures and unlimited allowances wait for you, with what they ask for shown in plain words',
      'fake support pages broken: alert storms silenced, fullscreen refused, the tab closed from a notice the page cannot press',
      'the other extensions audited for capture permissions and reach; sideloaded ones weigh more; disable from the shield',
    ],
  },
  {
    title: 'shopping',
    items: [
      'the product on the page looked up at eBay, Walmart, Best Buy, Newegg and Amazon in the background, shipping folded in where the store shows it, a direct search link for the rest',
      'coupon codes tried at checkout in a sensible order: what worked at that store before, what the store itself advertised, your own list, the signed feed; the total watched, the best kept, the order never placed',
      'Amazon’s on-page coupon clipped; price history per product with a fake-sale flag when the “was” price never existed, and a wait hint when it sits above its usual price',
      'a watchlist with drop alerts for products and cart pages; review health and seller trust on Amazon and eBay; what reddit says about the product',
      'store check: domain age from the registries’ own records, lookalike names, prices far below everyone else',
      'hidden fees added up before you pay; pre-ticked add-ons pointed out and unticked in one press; subscription traps named with a reminder before they charge; a 24-hour pause',
      'receipts saved from confirmation pages, spending per store per month, warranty reminders, a price-match proof card, discounted gift card links, the return policy in one line',
    ],
  },
  {
    title: 'annoyances, modes and tools',
    items: [
      'cookie banners answered with “reject all”; newsletter, paywall and app-nag overlays removed and scrolling restored; autoplay stopped; urgency claims dimmed and restarting countdowns struck out',
      'copy and right-click unblocked, dark mode for light sites, private search instead of Google, Bing or Yahoo, focus hours, low data, battery saver',
      'profiles (browsing, shopping, banking) and a mode per site; banking can pause every other extension while your bank is in front',
      'a paste guard for card numbers, government ids, keys and seed phrases, sharper on AI chat sites; photos stripped of location and camera metadata before they upload',
      'panic button and shortcut (every tab closed, history cleared, tunnel down), tab lock with a PIN, downloads that expire, parental mode with forced safe search',
      'tools: where a short link goes, the redirect chain of the current tab, a QR code check, a policy reader that grades terms in five lines, GDPR and CCPA deletion letters with a list of people-search opt-outs, an encrypted notes vault, settings sync encrypted end to end',
      'a dashboard of what was stopped today, a weekly report, camera and microphone indicator per tab, permission history, email aliases and decoy sign-up details',
    ],
  },
]

const NOT_POSSIBLE: [string, string][] = [
  ['A real VPN for the whole computer', 'An extension can only proxy the browser. The tunnel says so; noah’s device room covers the machine.'],
  ['The screen’s brightness and a Tor of its own', 'Not from an extension. With noah installed, the shield asks noah to do both; without it, the light card and the VPN card say so.'],
  ['Chrome’s bar under the new tab page', '“noah shield · Customize Chrome” is Chrome’s own footer for any extension that provides the new tab page. Customize Chrome, then the footer switch, turns it off; no extension can remove it.'],
  ['Free servers we have not checked', 'A stranger’s free proxy sees every site you visit. The vetted list is signed and starts empty; Tor and your own servers are there from day one.'],
  ['Wi-Fi security, auto-connect on public Wi-Fi, session hijack from another IP', 'No browser API sees the network or the server side. The device room reports Wi-Fi; the tunnel can connect at startup instead.'],
  ['Programs outside the browser recording the screen', 'Invisible to an extension; the device room lists them.'],
  ['A DNS leak test', 'Needs a resolver we control; with the tunnel on, names are resolved by the server by construction, and the leak page shows the exit.'],
  ['Deepfake and voice-clone detection, a data exposure score', 'Not achievable honestly with local heuristics; urgency scams are flagged by their words instead.'],
  ['Temporary phone numbers, burner cards, scam number lookups', 'These need paid third-party services with accounts; nothing free and trustworthy exists to build on.'],
  ['YouTube ads', 'An arms race we will not claim to win; YouTube’s trackers are blocked.'],
  ['Metadata on downloads', 'Extensions cannot rewrite downloaded files; uploads are stripped instead, and noah itself can clean local files.'],
]

export default function ShieldPage() {
  return (
    <Shell>
      <JsonLd data={breadcrumbs([{ name: 'shield', path: '/shield' }])} />
      <main className="l-doc">
        <div className="l-doc-card">
          <p className="l-doc-meta">shield · #houseofasher</p>
          <h1>noah shield</h1>
          <EnglishNote />
          <p className="l-doc-lede">
            A browser extension for every browser. It says plainly what each part can and cannot do. Free, open
            source, no account, no telemetry; nothing about you goes anywhere.
          </p>

          <AddShield />

          <section id="every-browser">
            <h2>Every browser</h2>
            <ul>
              {DOWNLOADS.map((download) => (
                <li key={download.file}>
                  <a href={`/shield/${download.file}`}>{download.label}</a> · {download.note} ·{' '}
                  <a href={`/shield/${download.file}.sha256`}>sha-256</a>
                </li>
              ))}
            </ul>
            <p>
              <strong>Chromium browsers</strong> (Chrome, Edge, Brave, Opera, Vivaldi, Arc): unzip the file, open{' '}
              <code>chrome://extensions</code> (or <code>edge://extensions</code>, <code>brave://extensions</code>), turn on{' '}
              <em>Developer mode</em>, choose <em>Load unpacked</em> and pick the unzipped folder. The shield opens this
              page and starts working. Browsers allow a one-click install only from their own stores; the listings are
              submitted, and the button above becomes that one click the day each is approved. A copy loaded by hand
              tells you inside the extension when a newer version is out.
            </p>
            <p>
              <strong>Firefox</strong>: open <code>about:debugging#/runtime/this-firefox</code>, choose{' '}
              <em>Load Temporary Add-on</em> and pick the zip. Firefox forgets temporary add-ons when it closes; the
              signed, permanent copy from addons.mozilla.org is on its way and will update itself from{' '}
              <code>/shield/updates.json</code>.
            </p>
            <p>
              <strong>Safari</strong>: on a Mac with Xcode, unzip the Safari file and run{' '}
              <code>xcrun safari-web-extension-converter noah-shield-safari</code>, open the project it makes, run it once,
              then turn the shield on in Safari → Settings → Extensions.
            </p>
          </section>

          {GROUPS.map((group) => (
            <section key={group.title}>
              <h2>{group.title}</h2>
              <ul>
                {group.items.map((item) => (
                  <li key={item}>{item}</li>
                ))}
              </ul>
            </section>
          ))}

          <section>
            <h2>Built with the bad people in mind</h2>
            <ul>
              <li>No remote code: everything the shield runs is in the package you installed.</li>
              <li>
                Its data feeds (servers, coupons, the latest version) are signed with noah&rsquo;s ed25519 release key
                and refused when the signature does not match or the feed has expired. This website alone cannot change
                what the shield does.
              </li>
              <li>
                Pages cannot answer the shield&rsquo;s questions for you: the page-side guard and the extension agree on a
                secret event name before any page script exists, and its bars live in closed shadow roots.
              </li>
              <li>Content scripts may ask the background only for what a page needs; settings change only from the shield&rsquo;s own pages.</li>
              <li>
                Passwords never leave the page in clear: reuse is tracked by a per-install keyed hash, leak checks send five
                hex characters, the PIN is stored as a keyed hash, the vault and sync are AES-GCM under a key from your
                passphrase.
              </li>
              <li>Price lookups omit cookies and carry only the product&rsquo;s name. Coupon tries never submit an order.</li>
              <li>Local-only mode turns off every request the shield would make on its own, feeds included.</li>
              <li>
                Source is in the noah repository under <code>shield/</code>:{' '}
                <a href={`${LINKS.source}/tree/main/shield`} target="_blank" rel="noopener noreferrer">
                  shep95/noah
                </a>
                . Report a weakness the way the <Link href="/security">security page</Link> describes.
              </li>
            </ul>
          </section>

          <section>
            <h2>What an extension cannot do, and what we do instead</h2>
            <ul>
              {NOT_POSSIBLE.map(([what, why]) => (
                <li key={what}>
                  <strong>{what}.</strong> {why}
                </li>
              ))}
            </ul>
          </section>
        </div>
      </main>
    </Shell>
  )
}
