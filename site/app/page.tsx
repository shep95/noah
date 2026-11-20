import type { Metadata } from 'next'
import Download from '@/components/landing/Download'
import Shell from '@/components/landing/Shell'
import { HOME_TITLE, pageMetadata } from '@/lib/seo'
import { DEFAULT_LANGUAGE, dictionary } from '@/lib/i18n'
import { currentLanguage } from '@/lib/language'
import { availableInstallers } from '@/lib/installers'

export function generateMetadata(): Metadata {
  const language = currentLanguage()
  return pageMetadata({
    path: '/',
    title: HOME_TITLE,
    description: dictionary(language).home.description,
    absoluteTitle: true,
    language,
    translated: true,
  })
}

const MODELS = [
  { name: 'anthropic · claude', local: false },
  { name: 'deepseek · r1', local: false },
  { name: 'llama 3.3 70b', local: true },
  { name: 'qwen 2.5 coder 32b · q4', local: true },
]

const RESULTS = [
  { title: 'spawn_blocking in tokio::task', url: 'docs.rs/tokio' },
  { title: 'Bridging with sync code', url: 'tokio.rs/tokio/topics' },
  { title: 'When should you use spawn_blocking?', url: 'users.rust-lang.org' },
]

/** Feature titles break where the translator broke them. */
function Lines({ text }: { text: string }) {
  const lines = text.split('\n')
  return (
    <>
      {lines.map((line, index) => (
        <span key={index}>
          {line}
          {index < lines.length - 1 && <br />}
        </span>
      ))}
    </>
  )
}

export default function Home() {
  const language = currentLanguage()
  const t = dictionary(language)
  const [readsFirst, yourModels, yourImage, looksOutward] = t.features
  const installers = availableInstallers()
  const inside = t.inside ?? dictionary(DEFAULT_LANGUAGE).inside
  const against = t.against ?? dictionary(DEFAULT_LANGUAGE).against
  const warning = t.warning ?? dictionary(DEFAULT_LANGUAGE).warning
  return (
    <Shell>
        <main>
          <section className="l-hero">
            <div className="l-hero-identity">
              <h1 className="l-hero-title">noah</h1>
              <p className="l-hero-sub">
                <span className="l-hero-claim">{t.hero.claim}</span>{' '}
                <span className="l-hero-proof">{t.hero.proof}</span>
              </p>
            </div>
            <div className="l-hero-cta">
              <Download language={language} t={t.download} installers={installers} />
            </div>
          </section>

          <div className="l-glass" id="features">
            <div className="l-feature-row l-reveal">
              <div className="l-feature-copy">
                <p className="l-feat-num">01 / 05</p>
                <h2 className="l-feat-title"><Lines text={readsFirst.title} /></h2>
                <p className="l-feat-body">{readsFirst.body}</p>
              </div>
              <div className="l-feature-visual" aria-hidden>
                <div className="l-code-block">
                  <div className="l-c-comment">{'// scanning project structure'}</div>
                  <div className="l-mt-12"><span className="l-c-arrow">→</span> <span className="l-c-label">src/components/</span></div>
                  <div><span className="l-c-arrow">→</span> <span className="l-c-label">naming convention</span>&nbsp;&nbsp;<span className="l-c-val">camelCase</span></div>
                  <div><span className="l-c-arrow">→</span> <span className="l-c-label">pattern</span>&nbsp;&nbsp;<span className="l-c-val">functional + hooks</span></div>
                  <div><span className="l-c-arrow">→</span> <span className="l-c-label">style layer</span>&nbsp;&nbsp;<span className="l-c-val">css modules</span></div>
                  <div><span className="l-c-arrow">→</span> <span className="l-c-label">test runner</span>&nbsp;&nbsp;<span className="l-c-val">vitest</span></div>
                  <div className="l-c-comment l-mt-18">{'// writing in your voice'}</div>
                  <div className="l-c-base l-mt-8">const <span className="l-c-val">useAuthState</span> = () =&gt; {'{'}</div>
                  <div className="l-c-base l-indent-1">const [user, setUser] = <span className="l-c-val">useState</span>(null)</div>
                  <div className="l-c-base l-indent-1"><span className="l-c-val">useEffect</span>(() =&gt; {'{'}</div>
                  <div className="l-c-base l-indent-2"><span className="l-c-val">checkSession</span>().then(setUser)</div>
                  <div className="l-c-base l-indent-1">{'}'}, [])</div>
                  <div className="l-c-base">{'}'}</div>
                </div>
              </div>
            </div>

            <div className="l-feature-row l-reverse l-reveal" id="models">
              <div className="l-feature-visual" aria-hidden>
                <div className="l-stack">
                  <div className="l-panel-label">{t.panels.connected}</div>
                  {MODELS.map((model) => (
                    <div key={model.name} className="l-row l-model-row">
                      <span className="l-model-name">{model.name}</span>
                      <span className={`l-badge ${model.local ? 'l-badge-local' : 'l-badge-key'}`}>
                        {model.local ? t.panels.local : t.panels.yourKey}
                      </span>
                    </div>
                  ))}
                  <p className="l-panel-note">{t.panels.nothingMetered}</p>
                </div>
              </div>
              <div className="l-feature-copy">
                <p className="l-feat-num">02 / 05</p>
                <h2 className="l-feat-title"><Lines text={yourModels.title} /></h2>
                <p className="l-feat-body">{yourModels.body}</p>
              </div>
            </div>

            <div className="l-feature-row l-reveal">
              <div className="l-feature-copy">
                <p className="l-feat-num">03 / 05</p>
                <h2 className="l-feat-title"><Lines text={yourImage.title} /></h2>
                <p className="l-feat-body">{yourImage.body}</p>
              </div>
              <div className="l-feature-visual" aria-hidden>
                <div className="l-editor-mock">
                  <div className="l-editor-bar">
                    <div className="l-dot l-dot-r" />
                    <div className="l-dot l-dot-y" />
                    <div className="l-dot l-dot-g" />
                    <span className="l-editor-filename">auth.ts</span>
                  </div>
                  <div className="l-editor-body">
                    <div><span className="l-ed-kw">export async function</span> <span className="l-ed-fn">validateToken</span><span className="l-ed-dim">(</span></div>
                    <div className="l-indent-1"><span className="l-ed-dim">token: string</span></div>
                    <div><span className="l-ed-dim">): Promise&lt;</span><span className="l-ed-kw">User | null</span><span className="l-ed-dim">&gt; {'{'}</span></div>
                    <div className="l-indent-1"><span className="l-ed-dim">const payload = await</span></div>
                    <div className="l-indent-2"><span className="l-ed-call">verify</span><span className="l-ed-dim">(token, secret)</span></div>
                    <div className="l-indent-1"><span className="l-ed-kw">if</span><span className="l-ed-dim"> (!payload) </span><span className="l-ed-kw">return</span><span className="l-ed-dim"> null</span></div>
                    <div className="l-indent-1"><span className="l-ed-kw">return</span> <span className="l-ed-call">findUser</span><span className="l-ed-dim">(payload.sub)</span></div>
                    <div><span className="l-ed-dim">{'}'}</span></div>
                  </div>
                </div>
              </div>
            </div>

            <div className="l-feature-row l-reverse l-reveal">
              <div className="l-feature-visual" aria-hidden>
                <div className="l-stack">
                  <div className="l-panel-label">{t.panels.webSearch}</div>
                  <div className="l-search-query">tokio spawn_blocking vs spawn</div>
                  {RESULTS.map((result) => (
                    <div key={result.url} className="l-row">
                      <div className="l-result-title">{result.title}</div>
                      <div className="l-result-url">{result.url}</div>
                    </div>
                  ))}
                </div>
              </div>
              <div className="l-feature-copy">
                <p className="l-feat-num">04 / 05</p>
                <h2 className="l-feat-title"><Lines text={looksOutward.title} /></h2>
                <p className="l-feat-body">{looksOutward.body}</p>
              </div>
            </div>
          </div>

          <section className="l-privacy l-reveal" id="privacy">
            <p className="l-feat-num">05 / 05</p>
            <h2 className="l-priv-title">{t.privacy.title}</h2>
            <p className="l-priv-body">{t.privacy.body}</p>
            <div className="l-stats">
              <div className="l-stat"><div className="l-stat-num">0</div><div className="l-stat-label">{t.privacy.accounts}</div></div>
              <div className="l-stat-divider" />
              <div className="l-stat"><div className="l-stat-num">0</div><div className="l-stat-label">{t.privacy.telemetry}</div></div>
              <div className="l-stat-divider" />
              <div className="l-stat"><div className="l-stat-num">∞</div><div className="l-stat-label">{t.privacy.localModels}</div></div>
              <div className="l-stat-divider" />
              <div className="l-stat"><div className="l-stat-num">0</div><div className="l-stat-label">{t.privacy.paywalls}</div></div>
            </div>
          </section>

          {inside && (
            <section className="l-inside l-reveal" id="inside">
              <p className="l-feat-num">{inside.eyebrow}</p>
              <h2 className="l-inside-title">{inside.title}</h2>
              <p className="l-inside-lede">{inside.lede}</p>
              <details className="l-inside-details">
                <summary>{inside.summary}</summary>
                <ul className="l-inside-list">
                  {inside.items.map((item) => (
                    <li key={item}>{item}</li>
                  ))}
                </ul>
                <p className="l-inside-note">{inside.note}</p>
              </details>
            </section>
          )}

          {against && (
            <section className="l-inside l-inside--against l-reveal" id="against">
              <p className="l-feat-num">{against.eyebrow}</p>
              <h2 className="l-inside-title">{against.title}</h2>
              <p className="l-inside-lede">{against.lede}</p>
              <details className="l-inside-details">
                <summary>{against.summary}</summary>
                <ul className="l-inside-list">
                  {against.items.map((item) => (
                    <li key={item}>{item}</li>
                  ))}
                </ul>
                <p className="l-inside-note">{against.note}</p>
              </details>
            </section>
          )}

          {warning && (
            <section className="l-inside l-inside--against l-reveal" id="warning">
              <p className="l-feat-num">{warning.eyebrow}</p>
              <h2 className="l-inside-title">{warning.title}</h2>
              <p className="l-inside-lede">{warning.lede}</p>
              <details className="l-inside-details">
                <summary>{warning.summary}</summary>
                <ol className="l-inside-list l-inside-layers">
                  {warning.layers.map((layer) => (
                    <li key={layer.who}>
                      <span className="l-inside-layer-who">{layer.who}</span>
                      {layer.what}
                    </li>
                  ))}
                </ol>
                <p className="l-inside-root">{warning.root}</p>
                <p className="l-inside-note">{warning.note}</p>
              </details>
            </section>
          )}

          <section className="l-cta" id="download">
            <p className="l-cta-eyebrow">{t.cta.eyebrow}</p>
            <h2 className="l-cta-title"><span>{t.cta.lineOne}</span><span>{t.cta.lineTwo}</span></h2>
            <Download language={language} t={t.download} installers={installers} />
          </section>
        </main>
    </Shell>
  )
}
