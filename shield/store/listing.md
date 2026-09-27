# noah shield: store listings

Everything the three stores ask for, in one place. The packages come from
`script/build-shield`; the screenshots beside this file are 1280×800 and the
promo tile is 440×280.

## Name, summary, category

* Name: **noah shield**
* Summary (132 characters max, Chrome): *A tunnel that fails closed, trackers
  and scams stopped, cheaper prices and working coupons, and a watch on what
  records your screen.*
* Category: Privacy & Security (Chrome), Privacy & Security (Firefox),
  Utilities (Safari App Store).
* Website: https://noah.asherin.com/shield · Support: the discord on that
  page · Source: https://github.com/shep95/noah/tree/main/shield
* Licence: GPL-3.0-or-later (EasyPrivacy rules GPLv3 / CC BY-SA 3.0; jsQR Apache-2.0).

## Description

noah shield is a free, open-source extension for every browser, built by
House of Asher alongside the noah IDE. It does four big things and a hundred
small ones, and it says plainly what an extension can and cannot do.

Tunnel: an encrypted proxy tunnel (HTTPS or SOCKS5) that fails closed, so a
dropped server never leaks your real address; names resolved by the server,
WebRTC held to proxied routes, a server per site, a speed test, a leak test,
Tor built in, your own servers, a signed list of vetted servers.

Data protection: EasyPrivacy's rules in the browser's own blocking engine,
third-party cookies off, tracker cookies purged, cookies burned when a site's
last tab closes, location and prompts denied per site, tracking parameters
stripped, redirect wrappers unwrapped, Referer hidden, Global Privacy Control
sent, webmail tracking pixels blocked, a privacy grade and a who's-watching
list for every site, a form-leak guard.

Fingerprint: canvas, WebGL and audio noise seeded per site, rotated on your
schedule; blend-in mode; fake location matched to the tunnel's country.

Security: https everywhere, lookalike domains stopped before they load,
password reuse and plain-http warnings, leak checks through Have I Been
Pwned's k-anonymity API, risky downloads held, clipboard and crypto wallet
guards, fake support pages broken, an audit of your other extensions.

Shopping: the same product at other stores, coupon codes tried at checkout
(never placing the order), price history with fake-sale flags, a watchlist
with drop alerts, review health and seller trust, store checks by domain age,
hidden fees revealed, pre-ticked add-ons and subscription traps named,
receipts and warranty reminders.

Annoyances and modes: cookie banners answered with "reject all", overlays and
paywalls removed, autoplay stopped, urgency theatre struck out, copy
unblocked, dark mode, private search, focus hours, low data, battery saver,
browsing/shopping/banking profiles, parental mode, tab lock, expiring
downloads, a paste guard, photo metadata stripped before upload, a panic
button, encrypted notes and end-to-end encrypted settings sync.

No account, no telemetry. Its data feeds are signed with noah's release key
and refused when the signature does not match. Local-only mode turns off
every request the shield would make on its own.

## Single purpose (Chrome requires one sentence)

noah shield protects the user's privacy, security and money while browsing:
it blocks trackers and scams, guards what pages may read or record, routes
traffic through a tunnel the user chooses, and finds better prices and
working coupons while shopping.

## Permission justifications (Chrome's form asks for each)

* `proxy`: the tunnel routes the browser's traffic through the server the user chose, per site.
* `privacy`: turns off third-party cookies, ad topics and prefetch; holds WebRTC to proxied routes while the tunnel is on.
* `contentSettings`: denies or asks for location, notifications, camera and microphone per site.
* `declarativeNetRequest`, `declarativeNetRequestFeedback`: tracker, pixel, mining and malvertising rules; https upgrade; header rules (GPC, Referer, blend-in User-Agent); per-site trust; focus hours, parental and search redirects.
* `webRequest`, `webRequestAuthProvider`: counts blocked requests per tab for the badge and privacy score, records redirect hops, answers the tunnel server's authentication challenge.
* `webNavigation`: replaces a lookalike domain with the warning page before it renders; resets per-tab counts.
* `cookies`: purges tracking cookies, burns a site's cookies, signs out of chosen sites after idle.
* `browsingData`: "burn this site" and the panic button clear a site's storage and the history.
* `downloads`: holds risky downloads for a decision; expires downloads the user marked.
* `management`: audits other extensions for capture permissions; banking mode can pause them while the bank is in front.
* `notifications`: price drops, reminders, held tunnel, weekly report; off in quiet mode.
* `scripting`: fills decoy sign-up details and copies a cleaned link on the user's request.
* `tabs`, `alarms`, `storage`: tab state, schedules, settings.
* `clipboardWrite`: wipes the clipboard after a password copy; copies aliases and cleaned links.
* `history`: forgets visits to the sites the user listed.
* `idle`: signs out of chosen sites after idle.
* `contextMenus`: "copy clean link", "where does this link go", "throwaway window", "decoy form".
* Host permission `<all_urls>`: the guards and the shopping helper run on every page the user opens; price lookups fetch other stores' public search pages.

## Privacy practices (Chrome's data disclosure)

* Personally identifiable information: not collected.
* Health, financial, authentication, personal communications, location, web history, user activity, website content: **not transmitted to the developer**. Typed values are compared locally; password reuse uses a per-install keyed hash kept in the browser; breach checks send five hex characters of a SHA-1 to Have I Been Pwned; price lookups send only a product's name to the other stores; the exit check sends nothing but the request itself to noah.asherin.com.
* No sale of data, no use for purposes unrelated to the single purpose, no creditworthiness or lending use.
* Remote code: none. All code ships in the package; data feeds are JSON verified against a pinned ed25519 key.

## Firefox (addons.mozilla.org)

* Upload `dist/noah-shield-firefox.zip` (built by `script/build-shield`; it passes `web-ext lint`). The id is `shield@noah.asherin.com`. The listed package carries no update address, as Mozilla requires; `dist/noah-shield-firefox.xpi` is the self-hosted variant with `https://noah.asherin.com/shield/updates.json`.
* The linter warns about "coinminer usage" in `rules/trackers.json`: those are the names of miners being *blocked*, from EasyPrivacy. Say so in the reviewer notes.
* For self-distribution with one-click install from the site instead of a listing: sign once with your AMO credentials and host the result:

      npx web-ext sign --source-dir <unzipped firefox package> --channel unlisted \
          --api-key <AMO JWT issuer> --api-secret <AMO JWT secret> --artifacts-dir dist/signed

  Then copy the signed `.xpi` to `public/shield/noah-shield-firefox.xpi` on the website, set `firefoxSigned: true` in `lib/site.ts`, and rerun `script/build-shield <site>` so `updates.json` carries its hash. `build-shield` never overwrites a signed xpi with an unsigned one.
* Data collection permissions: "none" (declared in the manifest).

## Chrome Web Store and Edge Add-ons

* Upload `dist/noah-shield-chromium.zip` to the developer dashboard (a one-time registration fee applies). Fill the single purpose, the permission justifications and the privacy practices above verbatim.
* Once approved, paste the listing address into `SHIELD_STORES.chrome` (and `.edge`) in the website's `lib/site.ts`; the "Add to …" button on /shield becomes a one-click install and the store handles updates.

## Safari

* On a Mac with Xcode: `xcrun safari-web-extension-converter noah-shield-safari --app-name "noah shield" --bundle-identifier com.asherin.noah.shield`, archive, and submit through App Store Connect (an Apple developer account is needed). Paste the App Store address into `SHIELD_STORES.safari`.
