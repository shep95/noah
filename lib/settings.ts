export interface GlobalSettings {
  veniceApiKey: string
  selectedModel: string
  wallpaperUrl: string
  wallpaperBrightness: number
  wallpaperBlur: number
  editorFontSize: number
  editorTabSize: number
  editorWordWrap: boolean
  editorMinimap: boolean
  theme: 'dark'
  autoSave: boolean
  githubToken: string
}

const SETTINGS_KEY = 'unlocket_settings'

const defaults: GlobalSettings = {
  veniceApiKey: '',
  selectedModel: 'llama-3.3-70b',
  wallpaperUrl: '/wallpaper.jpg',
  wallpaperBrightness: 8,
  wallpaperBlur: 0,
  editorFontSize: 14,
  editorTabSize: 2,
  editorWordWrap: false,
  editorMinimap: false,
  theme: 'dark',
  autoSave: true,
  githubToken: ''
}

export function loadSettings(): GlobalSettings {
  if (typeof window === 'undefined') return defaults
  try {
    const stored = localStorage.getItem(SETTINGS_KEY)
    if (!stored) return defaults
    return { ...defaults, ...JSON.parse(stored) }
  } catch {
    return defaults
  }
}

export function saveSettings(settings: Partial<GlobalSettings>): GlobalSettings {
  if (typeof window === 'undefined') return defaults
  const current = loadSettings()
  const updated = { ...current, ...settings }
  try {
    localStorage.setItem(SETTINGS_KEY, JSON.stringify(updated))
  } catch {
    // localStorage unavailable
  }
  return updated
}

export function clearSettings(): void {
  if (typeof window === 'undefined') return
  try {
    localStorage.removeItem(SETTINGS_KEY)
  } catch {
    // ignore
  }
}
