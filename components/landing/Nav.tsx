'use client'

import { useState, useEffect } from 'react'
import Link from 'next/link'

export default function Nav() {
  const [scrolled, setScrolled] = useState(false)

  useEffect(() => {
    const onScroll = () => setScrolled(window.scrollY > 40)
    window.addEventListener('scroll', onScroll, { passive: true })
    return () => window.removeEventListener('scroll', onScroll)
  }, [])

  return (
    <nav
      className={`fixed top-0 left-0 right-0 z-50 transition-all duration-500 ${
        scrolled ? 'glass border-b border-border py-3' : 'py-5'
      }`}
    >
      <div className="max-w-6xl mx-auto px-6 flex items-center justify-between">
        <Link href="/" className="flex items-center gap-2 group">
          <div className="w-7 h-7 rounded-lg bg-accent flex items-center justify-center transition-all duration-300 group-hover:bg-accent-hover group-hover:shadow-[0_0_16px_rgba(58,100,73,0.6)]">
            <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
              <path
                d="M7 1L12 4V10L7 13L2 10V4L7 1Z"
                stroke="#c4d4bc"
                strokeWidth="1.5"
                fill="none"
              />
              <circle cx="7" cy="7" r="1.5" fill="#c4d4bc" />
            </svg>
          </div>
          <span className="text-text-primary font-medium tracking-wide text-sm">unlocket</span>
        </Link>

        <div className="flex items-center gap-6">
          <Link
            href="/terms"
            className="text-text-secondary text-sm hover:text-text-primary transition-colors duration-200"
          >
            Terms
          </Link>
          <a
            href="https://discord.gg/M9hnebRwvk"
            target="_blank"
            rel="noopener noreferrer"
            className="text-text-secondary text-sm hover:text-text-primary transition-colors duration-200"
          >
            Discord
          </a>
          <Link
            href="/ide"
            className="px-4 py-2 rounded-lg bg-accent hover:bg-accent-hover text-text-primary text-sm font-medium transition-all duration-200 hover:shadow-[0_0_20px_rgba(58,100,73,0.4)]"
          >
            Open Editor
          </Link>
        </div>
      </div>
    </nav>
  )
}
