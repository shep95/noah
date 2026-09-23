import Nav from '@/components/landing/Nav'
import Footer from '@/components/landing/Footer'
import type { Metadata } from 'next'

export const metadata: Metadata = {
  title: 'Terms of Service — unlocket',
  description: 'What we own, what we do not, and how your data is handled when using unlocket.',
}

export default function TermsPage() {
  return (
    <main className="min-h-screen bg-bg-base">
      <div className="wallpaper opacity-30" aria-hidden />
      <Nav />

      <div className="relative z-10 max-w-2xl mx-auto px-6 pt-28 pb-20">
        <h1 className="text-3xl font-light text-text-primary mb-2">Terms of Service</h1>
        <p className="text-text-muted text-sm mb-12">Last updated September 2025 · #houseofasher</p>

        <div className="space-y-10 text-text-secondary leading-relaxed text-sm">
          <section>
            <h2 className="text-text-primary font-medium mb-3 text-base">Your projects are yours</h2>
            <p>
              We do not own your projects. We do not store your projects. We do not have access
              to your code, your files, or anything you create inside unlocket. Your work exists
              in your browser and nowhere else.
            </p>
          </section>

          <section>
            <h2 className="text-text-primary font-medium mb-3 text-base">API models and your data</h2>
            <p className="mb-3">
              Unlocket connects to AI models through your own API key — we do not hold a master
              key and we are not a middleman between you and the model provider. When you send a
              prompt, it goes from your browser to the model provider directly (Venice AI, or
              whatever provider their API routes to).
            </p>
            <p className="mb-3">
              The models you connect may send your prompts and generated code back to their
              own infrastructure. We have no control over this. When you select a model in the
              IDE, we show you exactly what data-handling policy applies to that model so you
              can make an informed choice before you type anything.
            </p>
            <p>
              We do not log your prompts. We do not read your prompts. Your prompts are sent
              by your browser to the model provider under your API key — they are yours from
              start to finish.
            </p>
          </section>

          <section>
            <h2 className="text-text-primary font-medium mb-3 text-base">What we collect</h2>
            <p className="mb-3">
              Nothing identifiable. No account. No sign-up. No email. No analytics that can be
              traced back to you as an individual.
            </p>
            <p>
              The pattern learning system that makes shepherd smarter over time runs entirely
              in your browser using IndexedDB. Those patterns are yours. They are never
              transmitted anywhere.
            </p>
          </section>

          <section>
            <h2 className="text-text-primary font-medium mb-3 text-base">API keys</h2>
            <p>
              Your Venice API key and GitHub token are stored in your browser&apos;s localStorage.
              They are never sent to our servers. They go only to the services they belong to —
              Venice AI and GitHub respectively — when you perform an action that requires them.
              Do not share unlocket sessions or browsers with people you would not share API
              keys with.
            </p>
          </section>

          <section>
            <h2 className="text-text-primary font-medium mb-3 text-base">No warranties</h2>
            <p>
              Unlocket is provided as-is, free of charge, with no guarantees of uptime,
              accuracy, or fitness for any particular purpose. We are not liable for any
              consequences of code written with the assistance of the AI models connected
              through unlocket.
            </p>
          </section>

          <section>
            <h2 className="text-text-primary font-medium mb-3 text-base">Changes to these terms</h2>
            <p>
              We may update these terms as the product evolves. Material changes will be
              communicated through the Discord community at{' '}
              <a
                href="https://discord.gg/M9hnebRwvk"
                target="_blank"
                rel="noopener noreferrer"
                className="text-accent hover:text-accent-hover underline underline-offset-2"
              >
                discord.gg/M9hnebRwvk
              </a>
              .
            </p>
          </section>

          <section>
            <h2 className="text-text-primary font-medium mb-3 text-base">Contact</h2>
            <p>
              Questions, concerns, or feedback: join the Discord or visit{' '}
              <a
                href="https://asherin.com/"
                target="_blank"
                rel="noopener noreferrer"
                className="text-accent hover:text-accent-hover underline underline-offset-2"
              >
                asherin.com
              </a>
              .
            </p>
          </section>
        </div>
      </div>

      <Footer />
    </main>
  )
}
