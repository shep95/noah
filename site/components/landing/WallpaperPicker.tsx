'use client'

import { useEffect, useRef, useState } from 'react'
import { THEMED_LOOKS, WALLPAPER_STORAGE_KEY } from '@/lib/site'

export type Wallpaper = { name: string; src: string }

type Rgb = [number, number, number]

// Every token a sampled palette writes inline on .landing, so a hand-made
// look can clear them again.
const SAMPLED_PROPERTIES = [
  '--l-wall-image',
  '--l-wall-filter',
  '--l-bg',
  '--l-shade',
  '--l-fog',
  '--l-surface-rgb',
  '--l-surface2-rgb',
  '--l-raise',
  '--l-raise-hover',
  '--l-line',
  '--l-mist',
  '--l-accent',
  '--l-accent-rgb',
  '--l-accent-bright',
  '--l-glow',
  '--l-accent-soft',
  '--l-badge',
  '--l-code-kw',
]

// The font variables each hand-made look uses (see app/fonts.ts), loaded
// before a switch so the new look does not arrive in fallback type.
const LOOK_FONTS: Record<string, string[]> = {
  glitch: ['--font-glitch-display', '--font-glitch-mono'],
  halo: ['--font-halo-display', '--font-halo-body'],
  lights: ['--font-lights-display', '--font-lights-body'],
  meteor: ['--font-meteor-display', '--font-meteor-body'],
  rings: ['--font-rings-display', '--font-rings-body'],
  sea: ['--font-sea-display', '--font-sea-body'],
}

function isThemed(name: string): boolean {
  return (THEMED_LOOKS as readonly string[]).includes(name)
}

const triplet = ([red, green, blue]: Rgb) => `${red}, ${green}, ${blue}`
const rgb = ([red, green, blue]: Rgb) => `rgb(${red}, ${green}, ${blue})`
const mix = (from: Rgb, to: Rgb, amount: number): Rgb =>
  from.map((value, index) => Math.round(value + (to[index] - value) * amount)) as Rgb

function averageColor(src: string): Promise<Rgb | null> {
  return new Promise((resolve) => {
    const image = new Image()
    image.crossOrigin = 'anonymous'
    image.onload = () => {
      const canvas = document.createElement('canvas')
      canvas.width = 32
      canvas.height = 20
      const context = canvas.getContext('2d')
      if (!context) return resolve(null)
      context.drawImage(image, 0, 0, 32, 20)
      const pixels = context.getImageData(0, 0, 32, 20).data
      let red = 0
      let green = 0
      let blue = 0
      for (let index = 0; index < pixels.length; index += 4) {
        red += pixels[index]
        green += pixels[index + 1]
        blue += pixels[index + 2]
      }
      const count = pixels.length / 4
      resolve([red / count, green / count, blue / count])
    }
    image.onerror = () => resolve(null)
    image.src = src
  })
}

/** A dark palette in the image's own hue, for wallpapers without a hand-made look. */
async function sampledPalette(src: string): Promise<Record<string, string> | null> {
  const average = await averageColor(src)
  if (!average) return null
  const white: Rgb = [255, 255, 255]
  const black: Rgb = [0, 0, 0]
  // Sink the image's hue almost to black for the fog and surfaces, and lift
  // it to a muted mid tone for the accent.
  const sink = (depth: number) => average.map((value) => Math.round((value / 255) * depth)) as Rgb
  const accent = average.map((value) => Math.round(90 + (value / 255) * 80)) as Rgb
  const bright = mix(accent, white, 0.2)
  return {
    '--l-wall-filter': 'contrast(1.05) brightness(0.88)',
    '--l-bg': rgb(sink(10)),
    '--l-shade': triplet(sink(14)),
    '--l-fog': triplet(sink(14)),
    '--l-surface-rgb': triplet(sink(20)),
    '--l-surface2-rgb': triplet(sink(24)),
    '--l-raise': triplet(sink(28)),
    '--l-raise-hover': triplet(sink(40)),
    '--l-line': triplet(mix(average.map(Math.round) as Rgb, white, 0.6)),
    '--l-mist': triplet(mix(average.map(Math.round) as Rgb, white, 0.7)),
    '--l-accent': rgb(accent),
    '--l-accent-rgb': triplet(accent),
    '--l-accent-bright': rgb(bright),
    '--l-glow': triplet(bright),
    '--l-accent-soft': rgb(mix(accent, white, 0.45)),
    '--l-badge': triplet(mix(accent, black, 0.7)),
    '--l-code-kw': rgb(mix(accent, black, 0.2)),
  }
}

function preloadImage(src: string): Promise<void> {
  const image = new Image()
  image.src = src
  return image.decode().catch(() => undefined)
}

function preloadFonts(look: string): Promise<unknown> {
  const style = getComputedStyle(document.documentElement)
  const loads = (LOOK_FONTS[look] ?? []).flatMap((variable) => {
    const family = style.getPropertyValue(variable).trim()
    if (!family) return []
    return ['400', '300', '500', 'italic 400'].map((variant) =>
      document.fonts.load(`${variant} 1em ${family}`).catch(() => []),
    )
  })
  return Promise.all(loads)
}

// Waits at most this long for the new image and type, then switches anyway.
const PRELOAD_LIMIT_MS = 1200

function withinLimit(work: Promise<unknown>): Promise<unknown> {
  return Promise.race([work, new Promise((resolve) => window.setTimeout(resolve, PRELOAD_LIMIT_MS))])
}

type ViewTransitionDocument = Document & { startViewTransition?: (update: () => void) => unknown }

/** Cross-fades the page into the new look, or fades only its colors where view transitions are missing. */
function transition(update: () => void) {
  if (window.matchMedia('(prefers-reduced-motion: reduce)').matches) {
    update()
    return
  }
  const viewTransitionDocument = document as ViewTransitionDocument
  if (typeof viewTransitionDocument.startViewTransition === 'function') {
    viewTransitionDocument.startViewTransition(update)
    return
  }
  const root = document.documentElement
  root.classList.add('l-theming')
  update()
  window.setTimeout(() => root.classList.remove('l-theming'), 400)
}

async function apply(wallpaper: Wallpaper, animate: boolean) {
  const landing = document.querySelector<HTMLElement>('.landing')
  if (!landing) return
  const root = document.documentElement
  const clearSampled = () => {
    for (const property of SAMPLED_PROPERTIES) landing.style.removeProperty(property)
  }

  let update: () => void
  if (isThemed(wallpaper.name)) {
    if (animate) await withinLimit(Promise.all([preloadImage(wallpaper.src), preloadFonts(wallpaper.name)]))
    update = () => {
      clearSampled()
      if (wallpaper.name === 'fog') root.removeAttribute('data-look')
      else root.setAttribute('data-look', wallpaper.name)
    }
  } else {
    const [palette] = await Promise.all([
      sampledPalette(wallpaper.src),
      animate ? withinLimit(preloadImage(wallpaper.src)) : Promise.resolve(),
    ])
    update = () => {
      // An image of the visitor's own keeps the fog look's type and layout.
      root.removeAttribute('data-look')
      clearSampled()
      landing.style.setProperty('--l-wall-image', `url('${wallpaper.src}')`)
      if (!palette) return
      for (const [property, value] of Object.entries(palette)) landing.style.setProperty(property, value)
    }
  }

  if (animate) transition(update)
  else update()
}

function SettingsIcon() {
  return (
    <svg width="16" height="16" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.6" strokeLinecap="round" strokeLinejoin="round" aria-hidden>
      <circle cx="12" cy="12" r="3" />
      <path d="M19.4 15a1.65 1.65 0 00.33 1.82l.06.06a2 2 0 01-2.83 2.83l-.06-.06a1.65 1.65 0 00-1.82-.33 1.65 1.65 0 00-1 1.51V21a2 2 0 01-4 0v-.09A1.65 1.65 0 009 19.4a1.65 1.65 0 00-1.82.33l-.06.06a2 2 0 01-2.83-2.83l.06-.06a1.65 1.65 0 00.33-1.82 1.65 1.65 0 00-1.51-1H3a2 2 0 010-4h.09A1.65 1.65 0 004.6 9a1.65 1.65 0 00-.33-1.82l-.06-.06a2 2 0 012.83-2.83l.06.06a1.65 1.65 0 001.82.33H9a1.65 1.65 0 001-1.51V3a2 2 0 014 0v.09a1.65 1.65 0 001 1.51 1.65 1.65 0 001.82-.33l.06-.06a2 2 0 012.83 2.83l-.06.06a1.65 1.65 0 00-.33 1.82V9a1.65 1.65 0 001.51 1H21a2 2 0 010 4h-.09a1.65 1.65 0 00-1.51 1z" />
    </svg>
  )
}

export default function WallpaperPicker({ wallpapers }: { wallpapers: Wallpaper[] }) {
  const [open, setOpen] = useState(false)
  const [current, setCurrent] = useState('fog')
  const panel = useRef<HTMLDivElement>(null)

  // The inline script in the layout has already put a hand-made look on
  // <html> before the first paint; this marks it chosen, or samples an image
  // of the visitor's own.
  useEffect(() => {
    let saved: string | null = null
    try {
      saved = window.localStorage.getItem(WALLPAPER_STORAGE_KEY)
    } catch {
      saved = null
    }
    const chosen = wallpapers.find((wallpaper) => wallpaper.name === saved)
    if (chosen) {
      setCurrent(chosen.name)
      void apply(chosen, false)
    }
  }, [wallpapers])

  useEffect(() => {
    if (!open) return
    const close = (event: MouseEvent | KeyboardEvent) => {
      if (event instanceof KeyboardEvent) {
        if (event.key === 'Escape') setOpen(false)
        return
      }
      if (panel.current && !panel.current.contains(event.target as Node)) setOpen(false)
    }
    window.addEventListener('mousedown', close)
    window.addEventListener('keydown', close)
    return () => {
      window.removeEventListener('mousedown', close)
      window.removeEventListener('keydown', close)
    }
  }, [open])

  const choose = (wallpaper: Wallpaper) => {
    if (wallpaper.name === current) return
    setCurrent(wallpaper.name)
    void apply(wallpaper, true)
    try {
      window.localStorage.setItem(WALLPAPER_STORAGE_KEY, wallpaper.name)
    } catch {
      // Private windows can refuse storage; the choice still applies now.
    }
  }

  return (
    <div className="l-wall-picker" ref={panel}>
      {open && (
        <div className="l-wall-panel" role="dialog" aria-label="choose a wallpaper">
          <p className="l-wall-title">wallpaper</p>
          <div className="l-wall-grid">
            {wallpapers.map((wallpaper) => (
              <button
                key={wallpaper.name}
                type="button"
                className={`l-wall-option${wallpaper.name === current ? ' is-current' : ''}`}
                onClick={() => choose(wallpaper)}
                aria-pressed={wallpaper.name === current}
                aria-label={wallpaper.name}
              >
                <img className="l-wall-thumb" src={wallpaper.src} alt="" loading="lazy" decoding="async" />
                <span>{wallpaper.name}</span>
              </button>
            ))}
          </div>
        </div>
      )}
      <button
        type="button"
        className="l-wall-toggle"
        onClick={() => setOpen((value) => !value)}
        aria-expanded={open}
        aria-label="wallpaper settings"
      >
        <SettingsIcon />
      </button>
    </div>
  )
}
