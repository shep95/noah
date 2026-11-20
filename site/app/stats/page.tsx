import type { Metadata } from 'next'
import EnglishNote from '@/components/landing/EnglishNote'
import Shell from '@/components/landing/Shell'
import { pageMetadata } from '@/lib/seo'
import { readStats, statsConfigured, type Counter, type Summary } from '@/lib/stats'

export const metadata: Metadata = pageMetadata({
  path: '/stats',
  title: 'who visits noah',
  description:
    'how many people visit noah.asherin.com, by day, hour, country and region, where they come from, which pages they read and which links they click. counted without cookies or ip addresses.',
})

// Percent of the column, never below a hairline so empty hours still show.
function barHeight(value: number, peak: number): number {
  return Math.max(1, Math.round((value / peak) * 100))
}

function Bars({ values, labels, caption }: { values: number[]; labels: string[]; caption: string }) {
  const peak = Math.max(1, ...values)
  return (
    <div className="l-bars" role="img" aria-label={caption}>
      {values.map((value, index) => (
        <div key={labels[index]} className="l-bar-col" title={`${labels[index]}: ${value}`}>
          {/* An SVG attribute, not a style attribute, so the CSP needs no 'unsafe-inline' for styles. */}
          <svg className="l-bar" viewBox="0 0 1 100" preserveAspectRatio="none" aria-hidden focusable="false">
            <rect x="0" width="1" y={100 - barHeight(value, peak)} height={barHeight(value, peak)} />
          </svg>
          <span className="l-bar-label">{labels[index]}</span>
        </div>
      ))}
    </div>
  )
}

function Table({ rows, total, heading }: { rows: Counter[]; total: number; heading: string }) {
  if (rows.length === 0) return <p className="l-stat-empty">nothing yet.</p>
  return (
    <table className="l-stat-table">
      <thead>
        <tr>
          <th scope="col">{heading}</th>
          <th scope="col" className="l-num">visits</th>
          <th scope="col" className="l-num">share</th>
        </tr>
      </thead>
      <tbody>
        {rows.map((row) => (
          <tr key={row.name}>
            <td>{row.name}</td>
            <td className="l-num">{row.count.toLocaleString('en-US')}</td>
            <td className="l-num">{total ? `${Math.round((row.count / total) * 100)}%` : '–'}</td>
          </tr>
        ))}
      </tbody>
    </table>
  )
}

function Report({ summary }: { summary: Summary }) {
  // Day of the month only: thirty columns leave no room for more, and the
  // table under the chart carries the full dates.
  const shortDay = (day: string) => day.slice(8)
  const returningShare = summary.visits ? Math.round((summary.returning / summary.visits) * 100) : 0
  return (
    <>
      <div className="l-stat-grid">
        <div className="l-stat-box"><div className="l-stat-big">{summary.visits.toLocaleString('en-US')}</div><div className="l-stat-small">visits, last {summary.days} days</div></div>
        <div className="l-stat-box"><div className="l-stat-big">{summary.today.toLocaleString('en-US')}</div><div className="l-stat-small">today so far (utc)</div></div>
        <div className="l-stat-box"><div className="l-stat-big">{summary.fresh.toLocaleString('en-US')}</div><div className="l-stat-small">first visits</div></div>
        <div className="l-stat-box"><div className="l-stat-big">{returningShare}%</div><div className="l-stat-small">came back ({summary.returning.toLocaleString('en-US')})</div></div>
        <div className="l-stat-box"><div className="l-stat-big">{summary.countries.length}</div><div className="l-stat-small">countries</div></div>
        <div className="l-stat-box"><div className="l-stat-big">{summary.allTime.toLocaleString('en-US')}</div><div className="l-stat-small">visits since counting began</div></div>
      </div>

      <h2>By day</h2>
      <Bars values={summary.perDay.map((day) => day.visits)} labels={summary.perDay.map((day) => shortDay(day.day))} caption="visits per day" />
      <p className="l-stat-note">new and returning, per day:</p>
      <table className="l-stat-table">
        <thead>
          <tr><th scope="col">day</th><th scope="col" className="l-num">visits</th><th scope="col" className="l-num">new</th><th scope="col" className="l-num">returning</th></tr>
        </thead>
        <tbody>
          {[...summary.perDay].reverse().filter((day) => day.visits > 0).slice(0, 14).map((day) => (
            <tr key={day.day}>
              <td>{day.day}</td>
              <td className="l-num">{day.visits}</td>
              <td className="l-num">{day.fresh}</td>
              <td className="l-num">{day.returning}</td>
            </tr>
          ))}
        </tbody>
      </table>

      <h2>By hour of day (utc)</h2>
      <Bars values={summary.hours} labels={summary.hours.map((_, hour) => String(hour).padStart(2, '0'))} caption="visits per hour of the day" />

      <h2>Where in the world</h2>
      {summary.countries.length === 0 ? (
        <p className="l-stat-empty">nothing yet.</p>
      ) : (
        <table className="l-stat-table">
          <thead>
            <tr><th scope="col">country</th><th scope="col">regions</th><th scope="col" className="l-num">visits</th></tr>
          </thead>
          <tbody>
            {summary.countries.map((country) => (
              <tr key={country.code}>
                <td>{country.name}</td>
                <td className="l-stat-regions">
                  {country.regions.map((region) => `${region.name} ${region.count}`).join(' · ') || '–'}
                </td>
                <td className="l-num">{country.count.toLocaleString('en-US')}</td>
              </tr>
            ))}
          </tbody>
        </table>
      )}

      <h2>Where they came from</h2>
      <p className="l-stat-note">the app or site that sent them: a referrer, an in-app browser, or a ?ref= tag on the link. direct means typed, bookmarked, or from an app that says nothing.</p>
      <Table rows={summary.sources} total={summary.visits} heading="source" />

      <h2>Pages</h2>
      <Table rows={summary.pages} total={summary.visits} heading="page" />

      <h2>Links clicked</h2>
      <Table rows={summary.clicks} total={summary.clicks.reduce((sum, click) => sum + click.count, 0)} heading="link" />

      <h2>Languages and devices</h2>
      <Table rows={summary.languages} total={summary.visits} heading="site language" />
      <Table rows={summary.devices} total={summary.visits} heading="device" />
    </>
  )
}

export default async function StatsPage() {
  const configured = statsConfigured()
  let summary: Summary | null = null
  let failure: string | null = null
  if (configured) {
    try {
      summary = await readStats(30)
    } catch (error) {
      // The reason goes to the log; visitors get the fact.
      console.error('stats: could not read the store', error)
      failure = 'the store did not answer'
    }
  }

  return (
    <Shell>
      <main className="l-doc">
        <div className="l-doc-card l-doc-wide">
          <p className="l-doc-meta">stats · public</p>
          <h1>Who visits noah</h1>
          <EnglishNote />
          <p className="l-doc-lede">
            Counted here, on this site, without cookies, ip addresses or any third party. A visit is one page
            opened; &ldquo;came back&rdquo; means the browser had been here before. Nothing identifies a person.
          </p>
          {!configured && (
            <p>
              Counting has not started: the site has no store to count in. In the Vercel project, open Storage,
              add an Upstash Redis database and connect it to this project; it sets <code>KV_REST_API_URL</code>{' '}
              and <code>KV_REST_API_TOKEN</code>, and the next deploy starts counting. This page fills in from
              the first visit after that.
            </p>
          )}
          {configured && failure && <p>The store did not answer just now ({failure}). Reload in a moment.</p>}
          {summary && <Report summary={summary} />}
        </div>
      </main>
    </Shell>
  )
}
