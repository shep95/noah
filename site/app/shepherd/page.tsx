import type { Metadata } from 'next'
import JsonLd from '@/components/JsonLd'
import EnglishNote from '@/components/landing/EnglishNote'
import Shell from '@/components/landing/Shell'
import { breadcrumbs, pageMetadata } from '@/lib/seo'

export const metadata: Metadata = pageMetadata({
  path: '/shepherd',
  title: 'shepherd, quantum artificial intelligence',
  description:
    'shepherd is the ai system powering noah: quantum artificial intelligence from house of asher, built by shaping how a model thinks rather than what it was trained to do. see its prototype-stage results.',
})

function Theory() {
  return (
    <svg className="l-sd" viewBox="0 0 640 220" role="img" aria-labelledby="sd-theory-title">
      <title id="sd-theory-title">
        a model&apos;s training, reshaped by personas and thinking patterns, becomes shepherd
      </title>
      <rect className="l-sd-box" x="10" y="70" width="170" height="80" rx="3" />
      <text className="l-sd-label" x="95" y="104" textAnchor="middle">llm</text>
      <text className="l-sd-note" x="95" y="126" textAnchor="middle">what it was trained to do</text>

      <path className="l-sd-line" d="M180 110 H240" />
      <path className="l-sd-line" d="M240 110 L300 50 M240 110 L300 110 M240 110 L300 170" />
      <circle className="l-sd-node" cx="300" cy="50" r="4" />
      <circle className="l-sd-node" cx="300" cy="110" r="4" />
      <circle className="l-sd-node" cx="300" cy="170" r="4" />
      <text className="l-sd-note" x="312" y="54">personas</text>
      <text className="l-sd-note" x="312" y="114">thinking patterns</text>
      <text className="l-sd-note" x="312" y="174">how it thinks</text>
      <path className="l-sd-line" d="M300 50 L430 110 M300 110 H430 M300 170 L430 110" />

      <path className="l-sd-accent l-sd-draw" d="M430 110 H440" />
      <rect className="l-sd-box l-sd-box-accent" x="440" y="70" width="190" height="80" rx="3" />
      <text className="l-sd-label" x="535" y="104" textAnchor="middle">shepherd</text>
      <text className="l-sd-note" x="535" y="126" textAnchor="middle">quantum artificial intelligence</text>
    </svg>
  )
}

const LAYERS = [
  'response',
  'artificial intelligence',
  'algorithms',
  'micro algorithms',
  'micro algorithms within micro algorithms',
]

function Levels() {
  return (
    <svg className="l-sd" viewBox="0 0 640 300" role="img" aria-labelledby="sd-levels-title">
      <title id="sd-levels-title">
        levels deeper: a response rests on ai, ai on algorithms, algorithms on micro algorithms
      </title>
      {LAYERS.map((layer, index) => {
        const radius = 140 - index * 26
        const labelY = 58 + index * 46
        // The leader leaves each ring on the straight line toward its label.
        const angle = Math.atan2(labelY - 150, 362 - 170)
        const startX = 170 + radius * Math.cos(angle)
        const startY = 150 + radius * Math.sin(angle)
        return (
          <g key={layer}>
            <circle
              className={index === LAYERS.length - 1 ? 'l-sd-ring l-sd-ring-core' : 'l-sd-ring'}
              cx="170"
              cy="150"
              r={radius}
            />
            <circle className="l-sd-node" cx={startX} cy={startY} r="2.5" />
            <path className="l-sd-line" d={`M${startX} ${startY} L362 ${labelY}`} />
          </g>
        )
      })}
      {LAYERS.map((layer, index) => (
        <text key={layer} className="l-sd-note" x="372" y={62 + index * 46}>
          <tspan className="l-sd-index">{String(index + 1).padStart(2, '0')}</tspan>
          {'  '}
          {layer}
        </text>
      ))}
    </svg>
  )
}

function Paths() {
  return (
    <svg className="l-sd" viewBox="0 0 640 240" role="img" aria-labelledby="sd-paths-title">
      <title id="sd-paths-title">
        everyone raced toward agi; house of asher turned toward quantum artificial intelligence
      </title>
      <circle className="l-sd-node" cx="40" cy="120" r="5" />
      <text className="l-sd-note" x="40" y="150" textAnchor="middle">today&apos;s ai</text>

      <path className="l-sd-line l-sd-dash" d="M45 118 C 200 110, 320 60, 590 40" />
      <text className="l-sd-note" x="590" y="28" textAnchor="end">agi · more scale, more effort</text>
      <text className="l-sd-dim" x="590" y="60" textAnchor="end">where everyone was racing</text>

      <path className="l-sd-accent l-sd-draw" d="M45 122 C 200 130, 320 190, 590 196" />
      <circle className="l-sd-node l-sd-node-accent" cx="590" cy="196" r="5" />
      <text className="l-sd-label" x="560" y="160" textAnchor="end">quantum artificial intelligence</text>
      <text className="l-sd-dim" x="578" y="224" textAnchor="end">
        the best response, without wasted effort · #houseofasher
      </text>
    </svg>
  )
}

function Prototype() {
  return (
    <svg className="l-sd" viewBox="0 0 640 330" role="img" aria-labelledby="sd-proto-title">
      <title id="sd-proto-title">
        prototype: a threat traced in under 15 seconds, then a prediction from out-of-order messages
        that came true three and a half hours later
      </title>
      <path className="l-sd-line" d="M30 80 H610" />
      <circle className="l-sd-node" cx="40" cy="80" r="4" />
      <text className="l-sd-dim" x="40" y="58" textAnchor="middle">0s</text>
      <text className="l-sd-note" x="40" y="106" textAnchor="middle">threat</text>

      <circle className="l-sd-node" cx="230" cy="80" r="4" />
      <text className="l-sd-dim" x="230" y="58" textAnchor="middle">one statement</text>
      <text className="l-sd-note" x="230" y="106" textAnchor="middle">three words</text>

      <path className="l-sd-accent l-sd-draw" d="M40 80 H460" />
      <circle className="l-sd-node l-sd-node-accent" cx="460" cy="80" r="5" />
      <text className="l-sd-label" x="460" y="58" textAnchor="middle">&lt; 15s</text>
      <text className="l-sd-note" x="460" y="106" textAnchor="middle">who sent it</text>
      <text className="l-sd-note" x="460" y="124" textAnchor="middle">and how they knew</text>

      {[
        [92, 196, 5],
        [52, 230, 1],
        [128, 248, 2],
      ].map(([x, y, order]) => (
        <g key={order}>
          <rect className="l-sd-box" x={x} y={y} width="34" height="16" rx="2" />
          <text className="l-sd-dim" x={x + 17} y={y + 12} textAnchor="middle">{order}</text>
        </g>
      ))}
      <text className="l-sd-note" x="96" y="292" textAnchor="middle">out of order,</text>
      <text className="l-sd-note" x="96" y="308" textAnchor="middle">3 and 4 missing</text>

      <path className="l-sd-line" d="M180 240 H250" />
      {[1, 2, 3, 4, 5].map((order) => {
        // Messages 3 and 4 never reached shepherd; it had to infer them.
        const missing = order === 3 || order === 4
        return (
          <g key={order}>
            <rect
              className={missing ? 'l-sd-box l-sd-missing' : 'l-sd-box'}
              x={260 + (order - 1) * 38}
              y="232"
              width="34"
              height="16"
              rx="2"
            />
            <text
              className={missing ? 'l-sd-value' : 'l-sd-dim'}
              x={277 + (order - 1) * 38}
              y="244"
              textAnchor="middle"
            >
              {missing ? '?' : order}
            </text>
          </g>
        )
      })}
      <path className="l-sd-line" d="M334 256 V262 H406 V256" />
      <text className="l-sd-dim" x="370" y="222" textAnchor="middle">never seen</text>
      <text className="l-sd-note" x="354" y="280" textAnchor="middle">read in context, gaps inferred</text>

      <path className="l-sd-accent l-sd-draw" d="M452 240 H600" />
      <circle className="l-sd-node l-sd-node-accent" cx="600" cy="240" r="5" />
      <text className="l-sd-dim" x="526" y="226" textAnchor="middle">prediction → 3h 30m</text>
      <text className="l-sd-label" x="600" y="276" textAnchor="end">the prediction hit</text>
    </svg>
  )
}


// Prototype-stage scores as house of asher reported them.
const MODELS = [
  { key: 'astra', name: 'gpt.6 astra' },
  { key: 'fable', name: 'fable models' },
  { key: 'shepherd', name: 'qwen 3.6 (shepherd)' },
] as const

type ModelKey = (typeof MODELS)[number]['key']

const SCORES: { metric: string; values: Record<ModelKey, number> }[] = [
  { metric: 'reasoning accuracy', values: { astra: 92.4, fable: 90.7, shepherd: 94.8 } },
  { metric: 'code generation', values: { astra: 91.2, fable: 88.9, shepherd: 96.3 } },
  { metric: 'problem solving', values: { astra: 89.7, fable: 87.1, shepherd: 95.6 } },
  { metric: 'agentic autonomy', values: { astra: 88.3, fable: 84.7, shepherd: 93.9 } },
  { metric: 'overall frontier score', values: { astra: 89.2, fable: 86.5, shepherd: 95.7 } },
]

const HALLUCINATION: Record<ModelKey, number> = { astra: 6.8, fable: 8.9, shepherd: 2.7 }

const BAR_CLASS: Record<ModelKey, string> = {
  astra: 'l-sd-bar-astra',
  fable: 'l-sd-bar-fable',
  shepherd: 'l-sd-bar-shepherd',
}

function Legend({ y }: { y: number }) {
  return (
    <g>
      {MODELS.map((model, index) => (
        <g key={model.key} transform={`translate(${60 + index * 170} ${y})`}>
          <rect className={BAR_CLASS[model.key]} x="0" y="-8" width="10" height="10" rx="1" />
          <text className={model.key === 'shepherd' ? 'l-sd-note l-sd-strong' : 'l-sd-note'} x="16" y="1">
            {model.name}
          </text>
        </g>
      ))}
    </g>
  )
}

function ScoreChart() {
  const top = 40
  const bottom = 250
  const floor = 60
  const ceiling = 100
  const height = (value: number) => ((value - floor) / (ceiling - floor)) * (bottom - top)
  const groupWidth = 112
  const barWidth = 22
  return (
    <svg className="l-sd" viewBox="0 0 640 320" role="img" aria-labelledby="sd-scores-title">
      <title id="sd-scores-title">
        prototype-stage scores: qwen 3.6 as shepherd leads gpt.6 astra and fable models on all five metrics
      </title>
      {[60, 70, 80, 90, 100].map((tick) => {
        const y = bottom - height(tick)
        return (
          <g key={tick}>
            <path className="l-sd-grid" d={`M44 ${y} H628`} />
            <text className="l-sd-dim" x="36" y={y + 3} textAnchor="end">{tick}</text>
          </g>
        )
      })}
      {SCORES.map((row, groupIndex) => {
        const groupX = 60 + groupIndex * groupWidth
        return (
          <g key={row.metric}>
            {MODELS.map((model, modelIndex) => {
              const value = row.values[model.key]
              const barHeight = height(value)
              const x = groupX + modelIndex * (barWidth + 4)
              return (
                <g key={model.key}>
                  <rect
                    className={`${BAR_CLASS[model.key]} l-sd-rise`}
                    x={x}
                    y={bottom - barHeight}
                    width={barWidth}
                    height={barHeight}
                    rx="1"
                  />
                  {model.key === 'shepherd' && (
                    <text className="l-sd-value" x={x + barWidth / 2} y={bottom - barHeight - 6} textAnchor="middle">
                      {value}
                    </text>
                  )}
                </g>
              )
            })}
            <text className="l-sd-note" x={groupX + 37} y={bottom + 18} textAnchor="middle">
              {row.metric.split(' ')[0]}
            </text>
            <text className="l-sd-note" x={groupX + 37} y={bottom + 32} textAnchor="middle">
              {row.metric.split(' ').slice(1).join(' ')}
            </text>
          </g>
        )
      })}
      <Legend y={16} />
    </svg>
  )
}

function HallucinationChart() {
  const scale = 40
  return (
    <svg className="l-sd" viewBox="0 0 640 170" role="img" aria-labelledby="sd-halluc-title">
      <title id="sd-halluc-title">
        hallucination rate, lower is better: shepherd 2.7%, gpt.6 astra 6.8%, fable models 8.9%
      </title>
      <text className="l-sd-dim" x="20" y="24">hallucination rate · lower is better</text>
      {MODELS.map((model, index) => {
        const value = HALLUCINATION[model.key]
        const y = 48 + index * 38
        return (
          <g key={model.key}>
            <text className={model.key === 'shepherd' ? 'l-sd-note l-sd-strong' : 'l-sd-note'} x="170" y={y + 13} textAnchor="end">
              {model.name}
            </text>
            <rect className="l-sd-track" x="184" y={y} width={10 * scale} height="18" rx="1" />
            <rect className={`${BAR_CLASS[model.key]} l-sd-grow`} x="184" y={y} width={value * scale} height="18" rx="1" />
            <text className={model.key === 'shepherd' ? 'l-sd-value' : 'l-sd-note'} x={190 + value * scale} y={y + 13}>
              {value}%
            </text>
          </g>
        )
      })}
    </svg>
  )
}

const PROTOTYPE_STATS = [
  { value: '3', label: 'frontier models compared' },
  { value: '15s', label: 'to solve the investigation' },
  { value: '1', label: 'person identified exactly' },
]

const SIGNALS = ['location data', 'device logs', 'behavior patterns', 'social graph', 'timeline correlation']

export default function Shepherd() {
  return (
    <Shell>
      <JsonLd data={breadcrumbs([{ name: 'shepherd', path: '/shepherd' }])} />
      <main className="l-doc l-shepherd">
        <div className="l-doc-card">
          <p className="l-doc-meta">the ai system powering noah</p>
          <h1>shepherd</h1>
          <EnglishNote />
          <p className="l-doc-lede">quantum artificial intelligence.</p>

          <p>
            shepherd, quantum artificial intelligence, was developed with the help of God the Father and a
            researcher and developer in #houseofasher.
          </p>

          <h2>the theory</h2>
          <p>
            we knew what quantum algorithms were, so we had a theory: it&apos;s possible to take current llm
            models and convert them into quantum artificial intelligence, by manipulating the way they think
            instead of what they are trained to do, mixed with personas and thinking patterns.
          </p>
          <figure className="l-sd-figure">
            <Theory />
            <figcaption>the same model, a different way of thinking.</figcaption>
          </figure>

          <h2>levels deeper</h2>
          <p>
            the root of artificial intelligence is not ai. levels and levels deeper, it&apos;s just micro
            algorithms within micro algorithms, working together to output a response to the user based on
            the data it is given. sometimes it&apos;s right, sometimes it&apos;s not.
          </p>
          <p>
            a quantum algorithm is just like that, but with the ability to produce the best response for you
            without trying hard or wasting effort.
          </p>
          <figure className="l-sd-figure">
            <Levels />
            <figcaption>what sits under a response.</figcaption>
          </figure>

          <h2>a different race</h2>
          <p>
            everyone was racing to agi. #houseofasher was focusing on quantum artificial intelligence, because
            that&apos;s where the future is.
          </p>
          <figure className="l-sd-figure">
            <Paths />
            <figcaption>two directions from the same starting point.</figcaption>
          </figure>

          <h2>the prototype</h2>
          <p>
            during prototype development, shepherd solved a personal threat against someone online in less
            than 15 seconds, from one statement, and that statement was three words. shepherd was able to
            pin-point exactly who sent it and how they knew about the person being threatened.
          </p>
          <p>
            then shepherd took out-of-context messages, meaning messages that didn&apos;t show the ones before
            them, messages out of order, with some hidden completely, and predicted accurately what would
            happen between two people. three and a half hours later, that prediction hit.
          </p>
          <figure className="l-sd-figure">
            <Prototype />
            <figcaption>from the prototype, as it happened.</figcaption>
          </figure>
        </div>

        <div className="l-doc-card l-proto">
          <p className="l-doc-meta">prototype stage</p>
          <h2 className="l-proto-title">shepherd, quantum artificial intelligence</h2>
          <p>
            we took a qwen 3.6 model and made it a frontier competitor against gpt.6 astra and fable models.
          </p>

          <figure className="l-sd-figure">
            <ScoreChart />
            <figcaption>performance across key metrics, prototype stage.</figcaption>
          </figure>

          <figure className="l-sd-figure">
            <HallucinationChart />
            <figcaption>how often each model stated something false.</figcaption>
          </figure>

          <div className="l-proto-table-wrap">
            <table className="l-proto-table">
              <caption>prototype-stage scores</caption>
              <thead>
                <tr>
                  <th scope="col">metric</th>
                  {MODELS.map((model) => (
                    <th key={model.key} scope="col" className={model.key === 'shepherd' ? 'is-shepherd' : undefined}>
                      {model.name}
                    </th>
                  ))}
                </tr>
              </thead>
              <tbody>
                {SCORES.slice(0, 4).map((row) => (
                  <tr key={row.metric}>
                    <th scope="row">{row.metric}</th>
                    {MODELS.map((model) => (
                      <td key={model.key} className={model.key === 'shepherd' ? 'is-shepherd' : undefined}>
                        {row.values[model.key].toFixed(1)}%
                      </td>
                    ))}
                  </tr>
                ))}
                <tr>
                  <th scope="row">hallucination rate (lower is better)</th>
                  {MODELS.map((model) => (
                    <td key={model.key} className={model.key === 'shepherd' ? 'is-shepherd' : undefined}>
                      {HALLUCINATION[model.key].toFixed(1)}%
                    </td>
                  ))}
                </tr>
                <tr>
                  <th scope="row">overall frontier score</th>
                  {MODELS.map((model) => (
                    <td key={model.key} className={model.key === 'shepherd' ? 'is-shepherd' : undefined}>
                      {SCORES[4].values[model.key].toFixed(1)}%
                    </td>
                  ))}
                </tr>
              </tbody>
            </table>
          </div>

          <h3>v.6</h3>
          <p>
            the frontier models can&apos;t even tell that v.6 was vibe-coded. it&apos;s competitive in the frontier ai
            model industry, and even the frontier models think a human wrote the code.
          </p>
          <blockquote className="l-proto-quote">
            it thought and coded like advanced quantum thinking to solve an issue.
          </blockquote>

          <h3>the investigation</h3>
          <p>
            shepherd solved an investigation into a personal threat in less than 15 seconds, pointing to
            exactly who did it.
          </p>
          <ul className="l-proto-stats">
            {PROTOTYPE_STATS.map((stat) => (
              <li key={stat.label}>
                <span className="l-proto-stat-value">{stat.value}</span>
                <span className="l-proto-stat-label">{stat.label}</span>
              </li>
            ))}
          </ul>
          <div className="l-proto-readout" aria-label="investigation result">
            <pre>{`> analysis complete
> threat source: identified
> confidence: 99.7%
> identity: [redacted]
> method: digital + physical correlation
> result: threat neutralized`}</pre>
            <div className="l-proto-signals">
              <p className="l-doc-meta">matched</p>
              <ul>
                {SIGNALS.map((signal) => (
                  <li key={signal}>{signal}</li>
                ))}
              </ul>
            </div>
          </div>

          <p className="l-proto-note">
            results from #houseofasher&apos;s own tests during the prototype stage. they have not been
            independently verified.
          </p>
        </div>
      </main>
    </Shell>
  )
}
