import type { Metadata } from 'next'
import JsonLd from '@/components/JsonLd'
import EnglishNote from '@/components/landing/EnglishNote'
import Shell from '@/components/landing/Shell'
import { breadcrumbs, pageMetadata } from '@/lib/seo'

export const metadata: Metadata = pageMetadata({
  path: '/terms',
  title: 'terms of service',
  description:
    'what noah owns and what it does not: your projects stay yours, nothing is stored, models answer to your own key, and web searches go to duckduckgo.',
})

export default function TermsPage() {
  return (
    <Shell>
      <JsonLd data={breadcrumbs([{ name: 'terms', path: '/terms' }])} />
      <main className="l-doc">
        <div className="l-doc-card">
          <p className="l-doc-meta">last updated september 2026 · #houseofasher</p>
          <h1>Terms of Service</h1>
          <EnglishNote />
          <p className="l-doc-lede">Plain terms for a free editor. No account, no records, no middleman.</p>

          <section>
            <h2>Your projects are yours</h2>
            <p>
              We do not own your projects, and we keep no records of them. We do not store your
              projects. We do not have access to your code, your files, or anything you create
              inside noah. Your work exists on your machine and nowhere else.
            </p>
          </section>

          <section>
            <h2>API models and your data</h2>
            <p>
              noah connects to AI models through your own API key — we do not hold a master
              key and we are not a middleman between you and the model provider. When you send a
              prompt, it goes from noah on your machine to the model provider directly (Venice AI,
              or whatever provider you configure).
            </p>
            <p>
              The models you connect may send your prompts and generated code back to their own
              infrastructure. We have no control over this. noah shows you which provider each
              model routes to so you can check that provider&apos;s data-and-prompt policy and make
              an informed choice before you type anything. Venice, for example, states it does not
              store prompts; other providers you add may differ — confirm with them.
            </p>
            <p>
              We do not log or read your prompts, and we do not hand them to anyone. Your prompts
              are sent by noah to the model provider under your API key — they are yours from
              start to finish. Alongside them, noah sends shepherd&apos;s brain: the fixed
              instructions that make the agent behave as shepherd. It ships inside the app, it is
              the same for everyone, and you can read it in the source.
            </p>
          </section>

          <section>
            <h2>What we collect</h2>
            <p>
              Nothing identifiable. No account. No sign-up. No email. No analytics that can be
              traced back to you as an individual.
            </p>
            <p>
              When shepherd searches the web, the search words go to DuckDuckGo, which returns
              the results directly to noah. noah asks you before every search.
            </p>
          </section>

          <section>
            <h2>API keys</h2>
            <p>
              Your Venice API key and any other credentials are stored locally on your machine by
              noah. They are never sent to our servers. They go only to the services they belong to
              — Venice AI, and any provider you configure — when you perform an action that requires
              them. Do not share your machine or noah profile with people you would not share API
              keys with.
            </p>
          </section>

          <section>
            <h2>No warranties</h2>
            <p>
              noah is provided as-is, free of charge, with no guarantees of uptime,
              accuracy, or fitness for any particular purpose. We are not liable for any
              consequences of code written with the assistance of the AI models connected
              through noah.
            </p>
          </section>

          <section>
            <h2>Changes to these terms</h2>
            <p>
              We may update these terms as the product evolves. Material changes will be
              communicated through the Discord community at{' '}
              <a
                href="https://discord.gg/M9hnebRwvk"
                target="_blank"
                rel="noopener noreferrer"
              >
                discord.gg/M9hnebRwvk
              </a>
              .
            </p>
          </section>

          <section>
            <h2>Contact</h2>
            <p>
              Questions, concerns, or feedback: join the Discord or visit{' '}
              <a
                href="https://asherin.com/"
                target="_blank"
                rel="noopener noreferrer"
              >
                asherin.com
              </a>
              .
            </p>
          </section>
        </div>
      </main>
    </Shell>
  )
}
