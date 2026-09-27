# noah shield

A browser extension for every browser: Chrome, Edge, Brave, Opera, Vivaldi,
Arc and the other Chromium browsers; Firefox; Safari (through Xcode's
converter). Four things, each described by what it can honestly do:

* **tunnel**: every request goes through one HTTPS or SOCKS5 server of your
  choosing and nowhere else; it fails closed, resolves names remotely and holds
  WebRTC to proxied routes. Tor on this computer is built in. Vetted servers
  come from a signed feed; you can add your own.
* **ads and data protection**: EasyList and EasyPrivacy rebuilt as declarativeNetRequest rules, EasyList's element hiding on every page,
  third-party cookies off, tracker cookies purged, location denied, prompts
  denied, fingerprints blurred, tracking parameters stripped, Global Privacy
  Control sent; per-site trust and per-site no-cookies.
* **shopping**: the product on the page looked up at other stores (public
  search pages, no cookies), coupon codes tried at checkout in a sensible
  order, never placing the order; Amazon's on-page coupon clipped.
* **recording watch**: pages must ask before screen or camera capture; the
  other installed extensions are audited for capture reach and can be disabled.

Everything it does is written down in its log (`log.html`), and with noah
installed on the computer it also turns the screen's real brightness and runs
Tor for the tunnel through noah's native messaging host (see STAGES.md, stage 11).

## Layout

    manifest.json        Chromium manifest (Firefox and Safari ones are derived)
    background.js        the worker / event page; imports lib/*.js
    lib/common.js        api shim, settings, helpers, the release public key
    lib/feeds.js         signed feeds (ed25519 over "noah-shield\n<name>\n"+bytes)
    lib/tunnel.js        proxy (PAC on Chromium, proxy.onRequest on Firefox)
    lib/privacy.js       browser privacy switches, per-site rules, cookie purge
    lib/watch.js         extension audit
    lib/shopping.js      store finders, code sources
    lib/brands.js        brands phishing pages imitate, with their real domains
    lib/safety.js        lookalike check, password hashes, breach checks, downloads
    lib/owners.js        who is behind a third-party host
    lib/cities.js        cities a fake location can stand in
    lib/tracking.js      per-tab watchers, privacy score, burning, sync, seeds, generators
    lib/tools.js         profiles, focus hours, low data, search switch, parental, vault, letters, policy reader
    content/guard.js     MAIN-world hooks: geolocation, capture, clipboard, wallet, form leaks, scam alerts, fingerprints, fake location
    content/bridge.js    isolated-world half: settings in, bars and reports out
    content/safety.js    lookalike notice, password guards, hidden fields and frames, scam pages
    content/hash.js      SHA-256/SHA-1 for pages without crypto.subtle
    content/links.js     link cleaner, redirect unwrapper, anti-dox blur
    content/shop.js      product detection, price card and extras, checkout codes
    content/checkout.js  fees, pre-ticked add-ons, subscription traps, receipts
    content/annoyances.js cookie banners, overlays, autoplay, countdowns, copy unblock, dark mode
    content/paste.js     sensitive paste guard, upload metadata stripper
    content/lock.js      tab lock curtain
    rules/*.json         EasyPrivacy and EasyList (built by script/build-shield-rules), headers, parameters, security, mail, referrer, social login
    rules/cosmetic.*     EasyList element hiding: a generic stylesheet and a per-site table
    content/capture.js   screenshots: visible, full page, an area; private details blurred first
    record.*             the recorder page: screen plus camera bubble, straight to Downloads
    popup.*, options.*, tools.*, warn.*, leak.*  the shield's own pages
    STAGES.md            what shipped in which stage, and what an extension cannot do

## Building

    script/build-shield-rules                 # refresh rules/trackers.json from EasyPrivacy
    script/build-shield [<website checkout>]  # dist/noah-shield-{chromium,firefox,safari}.zip,
                                              # and with a checkout: signed feeds under public/shield

`NOAH_RELEASE_SIGNING_KEY_FILE` must point at noah's release key for the feeds;
the public half is pinned in `lib/common.js` and must equal
`crates/auto_update/release_public_key.txt`.

## Loading it while developing

Chromium: `chrome://extensions`, Developer mode, Load unpacked, pick `shield/`.
Firefox: `about:debugging#/runtime/this-firefox`, Load Temporary Add-on, pick
the Firefox zip from `dist/` (Firefox needs the derived manifest).

## Licences

The extension is GPL-3.0-or-later, like the rest of noah's GPL parts.
`rules/trackers.json` is derived from EasyPrivacy (https://easylist.to),
GPLv3 / CC BY-SA 3.0. `lib/vendor/jsQR.js` is jsQR (Apache-2.0).
