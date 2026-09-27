# noah shield

A browser extension for every browser: Chrome, Edge, Brave, Opera, Vivaldi,
Arc and the other Chromium browsers; Firefox; Safari (through Xcode's
converter). Four things, each described by what it can honestly do:

* **tunnel**: every request goes through one HTTPS or SOCKS5 server of your
  choosing and nowhere else; it fails closed, resolves names remotely and holds
  WebRTC to proxied routes. Tor on this computer is built in. Vetted servers
  come from a signed feed; you can add your own.
* **data protection**: EasyPrivacy rebuilt as declarativeNetRequest rules,
  third-party cookies off, tracker cookies purged, location denied, prompts
  denied, fingerprints blurred, tracking parameters stripped, Global Privacy
  Control sent; per-site trust and per-site no-cookies.
* **shopping**: the product on the page looked up at other stores (public
  search pages, no cookies), coupon codes tried at checkout in a sensible
  order, never placing the order; Amazon's on-page coupon clipped.
* **recording watch**: pages must ask before screen or camera capture; the
  other installed extensions are audited for capture reach and can be disabled.

## Layout

    manifest.json        Chromium manifest (Firefox and Safari ones are derived)
    background.js        the worker / event page; imports lib/*.js
    lib/common.js        api shim, settings, helpers, the release public key
    lib/feeds.js         signed feeds (ed25519 over "noah-shield\n<name>\n"+bytes)
    lib/tunnel.js        proxy (PAC on Chromium, proxy.onRequest on Firefox)
    lib/privacy.js       browser privacy switches, per-site rules, cookie purge
    lib/watch.js         extension audit
    lib/shopping.js      store finders, code sources
    content/guard.js     MAIN-world hooks (geolocation, capture, fingerprints)
    content/bridge.js    isolated-world half: settings in, capture bars out
    content/shop.js      product detection, price card, checkout codes
    rules/trackers.json  EasyPrivacy, built by script/build-shield-rules
    rules/headers.json   Sec-GPC and DNT
    rules/parameters.json tracking parameters stripped from navigations
    popup.*, options.*   the shield's own pages

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
GPLv3 / CC BY-SA 3.0.
