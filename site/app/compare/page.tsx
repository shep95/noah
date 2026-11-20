import type { Metadata } from 'next'
import Link from 'next/link'
import JsonLd from '@/components/JsonLd'
import EnglishNote from '@/components/landing/EnglishNote'
import Shell from '@/components/landing/Shell'
import { breadcrumbs, pageMetadata } from '@/lib/seo'

export const metadata: Metadata = pageMetadata({
  path: '/compare',
  title: 'noah compared with cursor, windsurf, zed and copilot',
  description:
    'how noah, the free ai code editor, compares with cursor, windsurf, zed and vs code with copilot: price, accounts, bring-your-own-key, local models, telemetry, source and the agent.',
})

// Facts here are the kind that don't move: the licence, whether an account
// is required, whether your own key or a local model is possible. Prices and
// plan names change and are left to each product's own page.
const ROWS: { question: string; noah: string; cursor: string; windsurf: string; zed: string; copilot: string }[] = [
  {
    question: 'price',
    noah: 'free, no tiers',
    cursor: 'subscription, with a limited free plan',
    windsurf: 'subscription, with a limited free plan',
    zed: 'free editor; hosted ai is a paid plan',
    copilot: 'subscription for copilot; vs code is free',
  },
  {
    question: 'account needed',
    noah: 'no',
    cursor: 'yes',
    windsurf: 'yes',
    zed: 'only for hosted ai and collaboration',
    copilot: 'yes (github)',
  },
  {
    question: 'your own api key',
    noah: 'yes, nearly fifty providers',
    cursor: 'some providers',
    windsurf: 'limited',
    zed: 'yes, several providers',
    copilot: 'partly, through extensions',
  },
  {
    question: 'local models, offline',
    noah: 'yes: ollama, lm studio, llama.cpp; any size your machine holds',
    cursor: 'not as a first-class path',
    windsurf: 'not as a first-class path',
    zed: 'yes, through ollama',
    copilot: 'through third-party extensions',
  },
  {
    question: 'telemetry',
    noah: 'none',
    cursor: 'on, with a privacy mode',
    windsurf: 'on, with settings',
    zed: 'on by default, can be turned off',
    copilot: 'on by default, can be turned off',
  },
  {
    question: 'source',
    noah: 'open, gpl-3.0 (built on zed)',
    cursor: 'closed',
    windsurf: 'closed',
    zed: 'open, gpl-3.0',
    copilot: 'vs code open (mit); copilot closed',
  },
  {
    question: 'the agent',
    noah: 'shepherd: reads the whole project first, works in phases, keeps evidence of what it verified',
    cursor: 'agent mode',
    windsurf: 'cascade',
    zed: 'agent panel',
    copilot: 'agent mode',
  },
  {
    question: 'runs on',
    noah: 'windows, linux (macos coming)',
    cursor: 'macos, windows, linux',
    windsurf: 'macos, windows, linux',
    zed: 'macos, linux, windows',
    copilot: 'macos, windows, linux',
  },
]

export default function ComparePage() {
  return (
    <Shell>
      <JsonLd data={breadcrumbs([{ name: 'compare', path: '/compare' }])} />
      <main className="l-doc l-doc-wide-page">
        <div className="l-doc-card l-doc-wide">
          <p className="l-doc-meta">compare · last checked september 2026</p>
          <h1>How noah compares</h1>
          <EnglishNote />
          <p className="l-doc-lede">
            The honest version. noah is not the most feature-laden editor on this list; it is the one that
            asks nothing of you: no account, no subscription, no telemetry, and the models answer to you under
            your own key or on your own machine. Where another editor does something better, this page says
            so.
          </p>

          <div className="l-table-scroll" tabIndex={0} role="region" aria-label="comparison table, scrolls sideways">
            <table className="l-stat-table l-compare">
              <thead>
                <tr>
                  <th scope="col"> </th>
                  <th scope="col">noah</th>
                  <th scope="col">cursor</th>
                  <th scope="col">windsurf</th>
                  <th scope="col">zed</th>
                  <th scope="col">vs code + copilot</th>
                </tr>
              </thead>
              <tbody>
                {ROWS.map((row) => (
                  <tr key={row.question}>
                    <th scope="row">{row.question}</th>
                    <td className="l-compare-noah">{row.noah}</td>
                    <td>{row.cursor}</td>
                    <td>{row.windsurf}</td>
                    <td>{row.zed}</td>
                    <td>{row.copilot}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>

          <h2>Where noah is the better choice</h2>
          <ul>
            <li>You already pay a provider, or several, and want one editor that uses those keys directly.</li>
            <li>Your code cannot leave the building: local models, no account, no telemetry, and a sandbox for what the agent runs.</li>
            <li>You want to see what the agent verified, not only what it says. shepherd keeps evidence with every finished piece of work.</li>
            <li>You want a Windows or Linux editor that treats those systems as first class.</li>
          </ul>

          <h2>Where another editor is</h2>
          <ul>
            <li>You want a hosted, all-inclusive plan with no key to manage: cursor, windsurf and copilot bundle the model into the price.</li>
            <li>You need macOS today: noah&apos;s macOS build is still on the way; zed and the others have it.</li>
            <li>You rely on a large extension marketplace: vs code&apos;s is the biggest; noah runs zed&apos;s extensions.</li>
          </ul>

          <h2>Try it</h2>
          <p>
            noah installs in under a minute and needs nothing from you. <Link href="/download">Download it</Link>,
            add a key or point it at a <Link href="/local-models">local model</Link>, and judge for yourself.
          </p>
        </div>
      </main>
    </Shell>
  )
}
