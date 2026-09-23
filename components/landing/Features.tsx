'use client'

const FEATURES = [
  {
    icon: (
      <svg width="20" height="20" viewBox="0 0 20 20" fill="none">
        <path
          d="M10 2L14 5V9C14 12.5 10 16 10 16C10 16 6 12.5 6 9V5L10 2Z"
          stroke="currentColor"
          strokeWidth="1.5"
          fill="none"
        />
        <path d="M8 9.5L9.5 11L12 8" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
      </svg>
    ),
    title: 'Project Intelligence',
    body: 'Import any codebase and shepherd reads everything — architecture, naming conventions, patterns, style. Every suggestion fits your project like it was already there.',
  },
  {
    icon: (
      <svg width="20" height="20" viewBox="0 0 20 20" fill="none">
        <circle cx="10" cy="10" r="7" stroke="currentColor" strokeWidth="1.5" />
        <path d="M7 10L9 12L13 8" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
      </svg>
    ),
    title: 'Live AI Editing',
    body: 'Watch your code get written in real time. Fading transitions show every insertion as shepherd works through your files with full context awareness.',
  },
  {
    icon: (
      <svg width="20" height="20" viewBox="0 0 20 20" fill="none">
        <rect x="3" y="3" width="6" height="6" rx="1" stroke="currentColor" strokeWidth="1.5" />
        <rect x="11" y="3" width="6" height="6" rx="1" stroke="currentColor" strokeWidth="1.5" />
        <rect x="3" y="11" width="6" height="6" rx="1" stroke="currentColor" strokeWidth="1.5" />
        <path d="M14 11V17M11 14H17" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
      </svg>
    ),
    title: '120+ Models',
    body: 'Connect your Venice API key and access every model in the catalog. No model is locked behind a tier. You choose what runs your code.',
  },
  {
    icon: (
      <svg width="20" height="20" viewBox="0 0 20 20" fill="none">
        <path
          d="M4 14V6C4 4.9 4.9 4 6 4H14C15.1 4 16 4.9 16 6V14C16 15.1 15.1 16 14 16H6C4.9 16 4 15.1 4 14Z"
          stroke="currentColor"
          strokeWidth="1.5"
        />
        <path d="M8 8L10 10L12 8" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
        <path d="M10 10V13" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
      </svg>
    ),
    title: 'Your Wallpaper, Your IDE',
    body: 'Import any image and the entire editor adapts — fonts, colors, and atmosphere shift to match. Set it once, it persists everywhere.',
  },
  {
    icon: (
      <svg width="20" height="20" viewBox="0 0 20 20" fill="none">
        <path
          d="M10 3C10 3 5 5 5 10C5 13.3 7.2 16 10 16C12.8 16 15 13.3 15 10C15 5 10 3 10 3Z"
          stroke="currentColor"
          strokeWidth="1.5"
          fill="none"
        />
        <path d="M10 8V12M8 10H12" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
      </svg>
    ),
    title: 'Pattern Learning',
    body: "Shepherd remembers every correction you make. Over time it stops making the same mistakes. Your preferences accumulate into a personal pattern layer.",
  },
  {
    icon: (
      <svg width="20" height="20" viewBox="0 0 20 20" fill="none">
        <circle cx="10" cy="10" r="7" stroke="currentColor" strokeWidth="1.5" />
        <path d="M10 7V10L12 12" stroke="currentColor" strokeWidth="1.5" strokeLinecap="round" />
      </svg>
    ),
    title: 'Zero Friction',
    body: 'No account. No signup. No paywall. Open the editor and start. Settings persist in your browser. Your work stays yours.',
  },
]

export default function Features() {
  return (
    <section className="relative py-24 px-6">
      <div className="max-w-5xl mx-auto">
        <div className="text-center mb-16">
          <h2 className="text-3xl md:text-4xl font-light text-text-primary mb-4">
            Built for how you actually work
          </h2>
          <p className="text-text-secondary max-w-xl mx-auto">
            Every feature is designed around one principle: the AI should disappear into your
            workflow, not demand your attention.
          </p>
        </div>

        <div className="grid md:grid-cols-2 lg:grid-cols-3 gap-4">
          {FEATURES.map((f, i) => (
            <div
              key={f.title}
              className="p-6 rounded-2xl border border-border bg-bg-surface/50 hover:bg-bg-elevated/80 hover:border-accent/40 transition-all duration-300 group"
              style={{ animationDelay: `${i * 80}ms` }}
            >
              <div className="w-9 h-9 rounded-xl border border-border bg-bg-elevated flex items-center justify-center text-text-secondary group-hover:text-accent group-hover:border-accent/50 transition-all duration-300 mb-4">
                {f.icon}
              </div>
              <h3 className="text-text-primary font-medium text-sm mb-2">{f.title}</h3>
              <p className="text-text-secondary text-sm leading-relaxed">{f.body}</p>
            </div>
          ))}
        </div>
      </div>
    </section>
  )
}
