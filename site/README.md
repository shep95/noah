# noah — website

The marketing and download site for **noah**, a free native AI code editor by
**#houseofasher** ([asherin.com](https://asherin.com)). The editor itself lives
in [shep95/noah](https://github.com/shep95/noah); this repo is only the website.

Built with Next.js (App Router). Pages render per request so each one gets a
fresh CSP nonce; everything in `public/` is served as static files.

## Develop

```sh
npm install
npm run dev      # http://localhost:3000
npm run build    # production build
```

## Deploy

This is a standard Next.js app and deploys unchanged to either host:

- **Vercel** — import the repo; `vercel.json` is included. Zero config.
- **Railway** — create a service from the repo; `railway.json` is included
  (Nixpacks build, `npm run start`). The start script binds `0.0.0.0` and
  Next.js reads Railway's `$PORT` automatically.

The canonical origin is `https://noah.asherin.com` (`lib/site.ts`). Production
builds on Vercel permanently redirect every `*.vercel.app` host there; preview
builds are not redirected and are served `noindex`. `NEXT_PUBLIC_APP_URL` only
affects `next dev`.

> Note: only this website is web-hosted. **noah the editor is a native desktop
> app** (Rust/GPUI) and is distributed as a downloadable binary — it is not, and
> cannot be, hosted on Vercel or Railway.

## Security

- No API routes, no user accounts, no data collection.
- Nonce-based CSP (no `unsafe-inline` or `unsafe-eval` for scripts) in
  `middleware.ts`; the other security headers, download headers and redirects
  in `next.config.mjs`.
- `/.well-known/security.txt` and `/security` say how to report a vulnerability.
  Renew the `Expires` date in security.txt before 2027-09-25.
- Next.js pinned to the latest patched 14.2.x. Remaining `npm audit` findings are
  in dev-only tooling (eslint / typescript-eslint) that is not shipped.
