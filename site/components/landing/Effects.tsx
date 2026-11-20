'use client'

import { useEffect } from 'react'

// The wallpaper drifts slower than the page, sections rise in as they enter
// view, and the nav gains presence as you leave the hero.
export default function Effects() {
  useEffect(() => {
    const wallpaper = document.querySelector<HTMLElement>('.l-wallpaper')
    const nav = document.querySelector<HTMLElement>('.l-nav')
    const reducedMotion = window.matchMedia('(prefers-reduced-motion: reduce)').matches

    let frame = 0
    const onScroll = () => {
      if (frame) return
      frame = requestAnimationFrame(() => {
        frame = 0
        const scrolled = window.scrollY
        if (wallpaper && !reducedMotion) {
          const drift = Math.min(scrolled * 0.06, window.innerHeight * 0.18)
          wallpaper.style.transform = `translateY(${-drift}px)`
        }
        if (nav) {
          nav.style.opacity = String(0.4 + Math.min(1, scrolled / 300) * 0.6)
        }
      })
    }
    window.addEventListener('scroll', onScroll, { passive: true })

    // Each line in a mock and each figure knows its place in the queue, so
    // the stylesheet can let them arrive one after another.
    document
      .querySelectorAll('.l-code-block, .l-editor-body, .l-stack, .l-stats')
      .forEach((group) => {
        Array.from(group.children).forEach((child, index) => {
          ;(child as HTMLElement).style.setProperty('--i', String(index))
        })
      })

    const observer = new IntersectionObserver(
      (entries) => {
        for (const entry of entries) {
          if (entry.isIntersecting) {
            entry.target.classList.add('l-visible')
            observer.unobserve(entry.target)
          }
        }
      },
      { threshold: 0.12 },
    )
    document.querySelectorAll('.l-reveal').forEach((element) => observer.observe(element))

    return () => {
      window.removeEventListener('scroll', onScroll)
      if (frame) cancelAnimationFrame(frame)
      observer.disconnect()
    }
  }, [])

  return null
}
