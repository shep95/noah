# asherin.eye

This folder is your own copy of ADAM, a real-time globe (Cesium) with live
aircraft, vessels, satellites, weather, cameras and a voice analyst. noah
downloaded it once and never overwrites it, so edit anything. The upstream
docs are in `README.md` and `docs/`; `docs/CODE-BOUNDARIES.md` says which
folder owns what.

## Running it

noah starts `npm run dev` in a terminal when you open asherin.eye and shows
http://localhost:4173 in the browser room. Vite hot-reloads: save a file and
the page updates, no restart needed. If a change to an `/api` route under
`server/` doesn't show up, stop the terminal task and run `npm run dev` again.

- `npm test` runs the unit tests (`*.test.mjs` next to the code, Node's test
  runner). Run it after every change.
- `npm run check:boundaries` checks import directions between folders.
- `npm run build` produces `dist/`.
- API keys are optional. Put them in `.env` (see `.env.example`) or the
  in-app POWER UP panel. Never commit `.env`.

## Where things live

- `src/layers/<name>/`: one folder per map layer. `source.js` fetches and
  validates data (usually from `/api/...`), `index.js` exports
  `create<Name>Layer`, which draws it on the globe. `src/layers/cyclones/` is a
  small complete example.
- `src/app/constructCatalog.js`: the list of layers the app builds. A new
  layer is added there. `src/standalone/layerSources.js` creates the default
  source each layer gets.
- `server/providers/`: Node middleware for `/api/*` routes, one file or folder
  per provider, each exporting a `<name>Proxy()` Vite plugin. Upstream keys stay
  here, never in the browser. Register new ones in `localProviderPlugins()` in
  `server/providers/local.js`.
- `src/voice/`: the voice analyst. Tool schemas are in `actionSchemas.js`,
  handlers are grouped by domain in `src/voice/actions/*.js` (layer, scene,
  spatial, cockpit, analyst, context, intel) and dispatched by name from
  `gevActions.js`.
- `src/ui/`: panels and chrome; design tokens are in
  `src/ui/styles/tokens.css` (see `docs/DESIGN.md`).

## Adding a feature on top

1. Data from a new service: add `server/providers/<name>.js` with a
   `<name>Proxy()` route and register it in `server/providers/local.js`.
2. Show it on the globe: add `src/layers/<name>/` with `source.js` and
   `index.js`, following `src/layers/cyclones/`, then add the layer in
   `src/app/constructCatalog.js` and its source in
   `src/standalone/layerSources.js`.
3. Let the voice analyst use it: add a schema in `src/voice/actionSchemas.js`
   and a handler in the matching `src/voice/actions/*.js` map.
4. Add a `*.test.mjs` next to what you changed and run `npm test`.
