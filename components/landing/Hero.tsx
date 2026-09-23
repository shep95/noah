'use client'

import Link from 'next/link'
import { useEffect, useState } from 'react'

const DEMO_LINES = [
  '// shepherd understands your project',
  'const auth = createMiddleware({',
  '  strategy: "jwt",',
  '  refresh: true,',
  '})',
  '',
  '// adapts to your patterns automatically',
  'export async function handler(',
  '  req: NextRequest',
  ') {',
  '  const user = await auth.verify(req)',
  '  return Response.json({ user })',
  '}',
]

export default function Hero() {
  const [visibleLines, setVisibleLines] = useState(0)
  const [cursor, setCursor] = useState(true)

  useEffect(() => {
    if (visibleLines < DEMO_LINES.length) {
      const t = setTimeout(() => setVisibleLines((v) => v + 1), 120)
      return () => clearTimeout(t)
    }
  }, [visibleLines])

  useEffect(() => {
    const t = setInterval(() => setCursor((c) => !c), 530)
    return () => clearInterval(t)
  }, [])

  return (
    <section className="relative min-h-screen flex items-center justify-center pt-20 pb-16 px-6">
      <div className="wallpaper" aria-hidden />

      <div className="relative z-10 max-w-5xl mx-auto text-center">
        {/* Badge */}
        <div className="inline-flex items-center gap-2 px-3 py-1.5 rounded-full border border-border bg-bg-surface/60 text-text-secondary text-xs mb-8 animate-fade-in">
          <span className="w-1.5 h-1.5 rounded-full bg-accent animate-pulse-slow" />
          Free forever · No sign-up · #houseofasher
        </div>

        {/* Headline */}
        <h1 className="text-4xl md:text-6xl lg:text-7xl font-light text-text-primary leading-tight mb-6 animate-fade-up">
          Code with an AI that
          <br />
          <span className="text-fog-mid">knows your project</span>
        </h1>

        <p className="text-text-secondary text-lg md:text-xl max-w-2xl mx-auto mb-10 font-light animate-fade-up delay-100">
          Shepherd reads your codebase, learns your patterns, and writes code that belongs
          there. No setup, no subscription, no compromise.
        </p>

        {/* CTAs */}
        <div className="flex flex-col sm:flex-row gap-3 justify-center animate-fade-up delay-200">
          <Link
            href="/ide"
            className="px-7 py-3.5 rounded-xl bg-accent hover:bg-accent-hover text-text-primary font-medium transition-all duration-200 hover:shadow-[0_0_30px_rgba(58,100,73,0.5)] text-sm"
          >
            Start Writing Code
          </Link>
          <a
            href="https://discord.gg/M9hnebRwvk"
            target="_blank"
            rel="noopener noreferrer"
            className="px-7 py-3.5 rounded-xl border border-border hover:border-accent/60 text-text-secondary hover:text-text-primary font-medium transition-all duration-200 text-sm"
          >
            Join Discord
          </a>
        </div>

        {/* Demo code card */}
        <div className="mt-16 max-w-2xl mx-auto animate-fade-up delay-300">
          <div className="rounded-2xl border border-border bg-bg-surface/80 backdrop-blur-md overflow-hidden shadow-2xl">
            {/* Window chrome */}
            <div className="flex items-center gap-2 px-4 py-3 border-b border-border">
              <div className="w-3 h-3 rounded-full bg-red-900/60" />
              <div className="w-3 h-3 rounded-full bg-yellow-900/60" />
              <div className="w-3 h-3 rounded-full bg-green-900/60" />
              <span className="ml-3 text-text-muted text-xs font-mono">auth/middleware.ts</span>
              <div className="ml-auto flex items-center gap-1.5">
                <span className="w-1.5 h-1.5 rounded-full bg-accent animate-pulse-slow" />
                <span className="text-text-muted text-xs">shepherd</span>
              </div>
            </div>
            {/* Code */}
            <div className="p-5 text-left font-mono text-sm leading-6">
              {DEMO_LINES.slice(0, visibleLines).map((line, i) => (
                <div key={i} className="text-text-secondary animate-fade-in">
                  {line.startsWith('//') ? (
                    <span className="text-text-muted">{line}</span>
                  ) : (
                    line
                  )}
                  {i === visibleLines - 1 && (
                    <span
                      className={`inline-block w-0.5 h-4 bg-accent align-middle ml-0.5 transition-opacity duration-100 ${
                        cursor ? 'opacity-100' : 'opacity-0'
                      }`}
                    />
                  )}
                </div>
              ))}
            </div>
          </div>
        </div>
      </div>
    </section>
  )
}
