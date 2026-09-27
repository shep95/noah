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

## Stage 8: ads everywhere, capture, recording, a VPN people understand (shipped)

EasyList joins EasyPrivacy: 20,000 ad rules as a second network ruleset,
plus EasyList's element hiding (13,600 generic selectors on every page, per-site
ones from a table) injected at navigation, all off for a trusted site. The
popup gained capture: a screenshot of the screen, the whole page stitched, or
an area you drag, with private details (emails, numbers, cards, keys,
filled-in fields) blurred in the page before the shot; and a recorder page:
screen, window or tab with the camera in a rounded corner, microphone and
system sound mixed, written straight to Downloads/noah-shield as WebM with the
muxer, writer and date tags blanked. The tunnel now reads as a VPN: a status
dot, one connect button, locations by country with flags, "fastest location"
by default, Tor named as the anonymous route, and statuses in plain words. A
disconnect can no longer be overwritten by an exit check that finishes late.

## Stage 9: typing guard, site trust, network (shipped)

Typing guard: what you typed and then deleted is remembered alongside what
stayed, and neither leaves for another site before you submit; keystroke
telemetry (the same few field names, many times, headed elsewhere) is stopped;
a script from another site that listens to every key on the page is named the
first time a key reaches it, and in strict mode never receives the keys. Site
trust: the popup says, under the grade, whether the site is a lookalike, is
plain http, and whether it has leaked its users' data before (Have I Been
Pwned's public breach list by domain, kept a week; the domain is all that
leaves), with "use a password you use nowhere else here" when it has. Network,
on the tools page: where the traffic comes out, and whether another extension,
a policy or a system proxy steers the route. The wifi's name, password and DNS
are beyond an extension; noah's device room shows them, with a join code.

## Stage 10: light, frequency, the look (shipped)

Light: a veil over every page by scene (bright and sunny, in a dark room,
restaurant, night) or by two sliders, dim and warm; the backlight itself is
the device room's. Frequency: a tone from 20 to 963 Hz, or below 20 Hz a
binaural beat (two carriers an ear apart), played under whatever is on from
Chrome's offscreen document or Firefox's tools page, with presets from 2 Hz
to 963 Hz and a volume. The look: one of noah's six pictures or one of your
own (shrunk and re-encoded in the browser, never uploaded) sets the palette
by rules people find easy on the eye: blacks that carry the picture's
temperature, words that always read, one accent from the picture seen only
where it matters. The shield's pages, the new tab page (picture, clock,
search, today's numbers, a plain switch) and, on Firefox, the browser's own
frame and toolbars wear it; Chrome and Edge let no extension recolor theirs.

## Stage 11: the log, noah on this computer, clean search (shipped)

* **The log.** Every block, warning, notice and action is written down in
  plain words with the site and the moment: `log.html` (filter by kind, site
  or a word; save as text; clear), a "what happened here" list in the popup
  (the requests stopped on this page grouped by who was on the other end,
  and what the shield did on it), and every notification opens the log.
* **noah as the shield's hands.** A native messaging host in noah's
  command-line program (`noah --shield-host`, registered by noah on every
  launch for Chrome, Chromium, Edge, Brave, Vivaldi and Firefox) does what an
  extension cannot: turns the screen's real brightness (WMI, brightnessctl or
  sysfs, the `brightness` tool on macOS) and runs Tor for the tunnel: fetches
  the Tor Project's expert bundle once (checked against its checksum), starts
  it on port 9350 with the exit country you chose, reports the bootstrap
  percentage, stops it when you disconnect. The manifest key fixes the
  extension's id so the host can trust it.
* **Light.** Scenes are brightness levels for the screen itself; nothing is
  laid over pages any more. The warm tint stays, opt-in.
* **VPN.** One press: "fastest location · through noah" or a country, with
  progress while Tor bootstraps. Without noah, the card says what to install.
* **Search.** On Google, Bing, DuckDuckGo, Brave, Yahoo, Startpage and Ecosia
  the paid results go, known content farms and scraper sites go, and pages
  whose titles read as written for the engine are faded; a count on the page,
  a line in the log, three switches. Search pages wear the look.
* **Ads.** EasyList's general and third-party sections now come first (that
  is where Google's ad domains live; the long tail of ad servers had been
  using up the cap), a second ruleset of 30,000 more rules turns on when the
  browser has room, and Google Analytics and DoubleClick are blocked by the
  shield's own rules.
* **Screenshots** work on tabs open since before an update: the capture
  script is put in on demand.

## Not possible from an extension, and what we do instead

* **A real VPN for the whole computer.** An extension can only proxy the
  browser. The tunnel says so, and noah's device room covers the machine.
* **The screen's brightness, a Tor of our own.** Not from an extension; noah
  on the computer does both for it (stage 11).
* **Chrome's bar under the new tab page** ("noah shield · Customize Chrome")
  is Chrome's own footer for any extension that provides the new tab page;
  Customize Chrome → footer turns it off. No extension can remove it.
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
