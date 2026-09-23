import Nav from '@/components/landing/Nav'
import Hero from '@/components/landing/Hero'
import Features from '@/components/landing/Features'
import Footer from '@/components/landing/Footer'

export default function Home() {
  return (
    <main className="min-h-screen bg-bg-base relative">
      <Nav />
      <Hero />
      <Features />

      {/* CTA Section */}
      <section className="py-20 px-6 text-center relative">
        <div className="max-w-2xl mx-auto">
          <h2 className="text-3xl md:text-4xl font-light text-text-primary mb-4">
            Start writing better code today
          </h2>
          <p className="text-text-secondary mb-8">
            No account. No subscription. No friction. Just open the editor and go.
          </p>
          <a
            href="/ide"
            className="inline-flex items-center gap-2 px-8 py-4 rounded-xl bg-accent hover:bg-accent-hover text-text-primary font-medium transition-all duration-200 hover:shadow-[0_0_40px_rgba(58,100,73,0.5)]"
          >
            Open the IDE
            <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
              <path d="M3 7H11M8 4L11 7L8 10" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
            </svg>
          </a>
        </div>
      </section>

      <Footer />
    </main>
  )
}
