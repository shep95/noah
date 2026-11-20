import {
  Barlow,
  Cormorant_Garamond,
  Instrument_Serif,
  Italiana,
  JetBrains_Mono,
  Jost,
  Lexend_Exa,
  Libre_Baskerville,
  Lora,
  Manrope,
  Oswald,
  Space_Grotesk,
} from 'next/font/google'

// The typefaces of the wallpaper looks. The default fog look keeps the
// system's Georgia, so nothing here is preloaded: every face is self-hosted
// under font-src 'self' and the browser only fetches the ones the chosen look
// actually uses.
//
// shepherd's interface rule is thin, lowercase type, so every display face
// here has a real lowercase and a light cut; capitals-only faces are out.

const glitchDisplay = Space_Grotesk({ subsets: ['latin'], variable: '--font-glitch-display', preload: false })
const glitchMono = JetBrains_Mono({ subsets: ['latin'], style: ['normal'], variable: '--font-glitch-mono', preload: false })

const haloDisplay = Italiana({ subsets: ['latin'], weight: '400', variable: '--font-halo-display', preload: false })
const haloBody = Libre_Baskerville({
  subsets: ['latin'],
  weight: ['400'],
  style: ['normal', 'italic'],
  variable: '--font-halo-body',
  preload: false,
})

const lightsDisplay = Oswald({ subsets: ['latin'], weight: ['200', '300'], variable: '--font-lights-display', preload: false })
const lightsBody = Barlow({ subsets: ['latin'], weight: ['300', '400'], variable: '--font-lights-body', preload: false })

const meteorDisplay = Instrument_Serif({
  subsets: ['latin'],
  weight: '400',
  style: ['normal', 'italic'],
  variable: '--font-meteor-display',
  preload: false,
})
const meteorBody = Jost({ subsets: ['latin'], style: ['normal'], variable: '--font-meteor-body', preload: false })

const ringsDisplay = Lexend_Exa({ subsets: ['latin'], weight: ['200', '300'], variable: '--font-rings-display', preload: false })
const ringsBody = Manrope({ subsets: ['latin'], variable: '--font-rings-body', preload: false })

const seaDisplay = Cormorant_Garamond({
  subsets: ['latin'],
  weight: ['300', '400'],
  style: ['normal', 'italic'],
  variable: '--font-sea-display',
  preload: false,
})
const seaBody = Lora({ subsets: ['latin'], style: ['normal', 'italic'], variable: '--font-sea-body', preload: false })

/** Defines every look's font variables on <html>; app/landing.css picks them up per look. */
export const lookFontVariables = [
  glitchDisplay,
  glitchMono,
  haloDisplay,
  haloBody,
  lightsDisplay,
  lightsBody,
  meteorDisplay,
  meteorBody,
  ringsDisplay,
  ringsBody,
  seaDisplay,
  seaBody,
]
  .map((font) => font.variable)
  .join(' ')
