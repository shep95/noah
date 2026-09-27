# noah shield: stages

The shield is built in stages, each tested in Chromium with the harness in the
session scratchpad (`shield_test/run.cjs`) before the next begins. A feature is
listed under the stage that ships it; the last section lists what a browser
extension cannot honestly do and what we do instead.

## Stage 1: the four pillars (shipped)

tunnel (fail-closed proxy, remote DNS, WebRTC guard, exit check, Tor built in,
signed server feed, your own servers) · trackers (EasyPrivacy as DNR rules,
badge counts) · third-party cookies off, tracker cookie purge, per-site trust
and per-site no-cookies · location denied, prompts denied, camera/mic ask ·
canvas/hardware/battery blur · tracking parameters stripped · Global Privacy
Control · shopping (product detection, other-store lookups, links, coupon
codes from the store's own pages, your list and the signed feed, checkout
auto-apply that never orders, Amazon coupon clipping) · capture guard (screen
and camera behind a bar the page cannot press) · extension audit · signed
feeds, update notice, Firefox update manifest.

## Stage 2: security and account safety (shipped)

https upgrade with local-network exceptions · lookalike and phishing domain
interstitial (brand table, homoglyphs, edit distance, brand-in-subdomain) ·
password reuse on the wrong site (per-install HMAC, never the password) ·
password over http · breach check for passwords (Have I Been Pwned range API,
five hex characters leave the browser) and for emails with your own HIBP key ·
sketchy download interstitial (risky types, untrusted source) with optional
VirusTotal URL check under your key · clipboard read behind a bar; clipboard
wiped 30 s after copying from a password field · hidden autofill fields
disarmed · form data leak guard (typed values sent to third parties before
submit are stopped) · crypto wallet guard (approvals, signatures, address
swaps on copy) · fake support popup breaker (alert loops, fullscreen,
"call this number") · hidden third-party iframes removed · session replay,
crypto mining, malvertising network and social pixel rules · social login
(Google One Tap, Facebook) blocker (off by default).

## Stage 3: privacy, fingerprint and anti-tracking (shipped)

burn this site · cookies burned when a site's last tab closes, with a keep
list · privacy score per site and a toolbar meter · who's watching (owners,
countries) · referrer hidden from third parties · link cleaner and redirect
unwrapper (google, facebook, youtube, outlook safelinks, email click wrappers)
· webmail tracking pixels · fingerprint suite: canvas, WebGL, audio, screen,
timezone and language, user agent header, fonts, cores and memory, battery,
seeded per site, rotated on a timer, a click or each session, with a blend-in
mode · fake location matched to the tunnel's country or a chosen city · email
plus-alias generator (and SimpleLogin with your key) · decoy signup data ·
anti-dox blur of your own name · permission history · auto-logout after idle ·
auto-clear history per site · settings sync, end-to-end encrypted through the
browser's own sync · local-only mode · stealth element names.

## Stage 4: tunnel extras and shopping extras (shipped)

a server per site (country per site) · speed test and fastest pick · connect
at startup · IP and WebRTC leak test · camera and microphone indicator per
tab · shipping folded into the comparison · local price history, fake-sale
flag, buy-later hint · watchlist with drop alerts (product and cart pages) ·
review health and seller trust on Amazon and eBay · reddit threads about the
product · store check (domain age by RDAP, lookalike, too-good price) ·
hidden fees revealed · subscription traps and pre-ticked add-ons · wait 24 h ·
receipts, spending per store, warranty reminders · price-match proof card ·
gift card discount links · return policy finder.

## Stage 5: annoyance killers, smart modes, tools (shipped)

cookie banner auto-reject · overlay, paywall and newsletter remover ·
autoplay stop · fake countdown flag · copy and right-click unblocker · auto
dark mode · focus hours · low-data mode · battery saver · private search
switcher and google result cleaner · profiles (shopping, banking, browsing) ·
weekly report · dashboard · panic button and shortcut · tab lock · auto-expire
downloads · upload metadata stripper (JPEG, PNG, WebP) · sensitive paste and
AI-chat leak warning · QR scanner · URL unshortener and redirect chain ·
data broker opt-out and forget-me request generator · policy and terms red
flags · encrypted notes vault · parental mode · throwaway window · Firefox
containers per site.

## Stage 6: look, tests, publish (shipped)

popup and options in the noah landing look; dashboard; the harness covers
every stage; the site's /shield page; zips and signed feeds; both repos pushed.

## Stage 7: hardening and quiet motion (shipped)

Reviewed the way an attacker reads an extension: a page's content script is
bound to its own tab and site in the worker; lookups a page can trigger are
rate-limited per tab; settings blobs cannot touch prototypes; older signed
feeds are refused; store lookups, watched pages and the unshortener never
reach local addresses, even through a redirect; the lookalike page only
names brands from its own table; beacons by other names (image and script
sources, sockets, event streams, cross-site forms, Request bodies) are held
by the form-leak guard; hooks read as native and the shield's elements can
carry a per-session name; the parental pin is PBKDF2; web-accessible
resources are the warning page alone. The pages got pill switches, staged
arrival, focus rings and one easing, all off under reduced motion.

## Not possible from an extension, and what we do instead

* **A real VPN for the whole computer.** An extension can only proxy the
  browser. The tunnel says so, and noah's device room covers the machine.
* **Free servers we have not checked.** A stranger's free proxy sees every
  site you visit. The vetted list is signed and starts empty; Tor and your
  own servers are there from day one.
* **Wi-Fi security, auto-connect on public Wi-Fi, session hijack from another
  IP.** No browser API sees the network or the server side. The device room
  reports Wi-Fi; the tunnel can connect at startup instead.
* **Programs outside the browser recording the screen.** Invisible to an
  extension; the device room lists them.
* **DNS leak test.** Needs a resolver we control; with the tunnel on, names
  are resolved by the server by construction, and the IP test shows the exit.
* **Deepfake detection, voice clone detection, data exposure score.** Not
  achievable honestly with local heuristics; we flag urgency scams by their
  words instead.
* **Temporary phone numbers, burner cards, scam number lookup.** Need paid
  third-party services with accounts; nothing free and trustworthy exists to
  build on, so we do not pretend.
* **YouTube ads.** An arms race we will not claim to win; YouTube's trackers
  are blocked.
* **Metadata on downloads.** Extensions cannot rewrite downloaded files;
  uploads are stripped instead, and noah itself can clean local files.
