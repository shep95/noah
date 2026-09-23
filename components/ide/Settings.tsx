'use client'

import { useState } from 'react'
import { loadSettings, saveSettings, type GlobalSettings } from '@/lib/settings'
import { VENICE_MODELS } from '@/lib/venice'
import { readCleanDataUrl } from '@/lib/strip-metadata'

interface SettingsProps {
  onClose: () => void
}

export default function Settings({ onClose }: SettingsProps) {
  const [settings, setSettings] = useState<GlobalSettings>(loadSettings())
  const [saved, setSaved] = useState(false)
  const [showKey, setShowKey] = useState(false)
  const [showGhToken, setShowGhToken] = useState(false)

  const selectedModelInfo = VENICE_MODELS.find((m) => m.id === settings.selectedModel)

  const handleSave = () => {
    saveSettings(settings)
    setSaved(true)
    setTimeout(() => setSaved(false), 2000)
  }

  const update = (partial: Partial<GlobalSettings>) => {
    setSettings((s) => ({ ...s, ...partial }))
  }

  return (
    <div
      className="fixed inset-0 z-50 flex items-center justify-center p-4"
      onClick={(e) => e.target === e.currentTarget && onClose()}
    >
      <div className="absolute inset-0 bg-bg-base/80 backdrop-blur-sm" onClick={onClose} />

      <div className="relative z-10 w-full max-w-lg bg-bg-surface border border-border rounded-2xl overflow-hidden shadow-2xl animate-fade-up">
        {/* Header */}
        <div className="flex items-center justify-between px-6 py-4 border-b border-border">
          <div>
            <h2 className="text-text-primary font-medium text-sm">Settings</h2>
            <p className="text-text-muted text-xs mt-0.5">Saved globally to your browser</p>
          </div>
          <button
            onClick={onClose}
            className="w-7 h-7 rounded-lg hover:bg-bg-hover flex items-center justify-center transition-colors text-text-muted hover:text-text-primary"
          >
            ✕
          </button>
        </div>

        <div className="p-6 space-y-6 max-h-[70vh] overflow-y-auto custom-scrollbar">
          {/* Venice API Key */}
          <section>
            <label className="text-text-secondary text-xs font-medium uppercase tracking-wider block mb-3">
              Venice API Key
            </label>
            <div className="relative">
              <input
                type={showKey ? 'text' : 'password'}
                value={settings.veniceApiKey}
                onChange={(e) => update({ veniceApiKey: e.target.value })}
                placeholder="venice-xxxxxxxxxxxx"
                className="w-full bg-bg-elevated border border-border rounded-xl px-4 py-2.5 text-text-primary text-sm placeholder:text-text-muted focus:outline-none focus:border-accent/60 pr-16 transition-colors"
              />
              <button
                onClick={() => setShowKey((s) => !s)}
                className="absolute right-3 top-1/2 -translate-y-1/2 text-text-muted text-xs hover:text-text-primary transition-colors"
              >
                {showKey ? 'Hide' : 'Show'}
              </button>
            </div>
            <p className="text-text-muted text-xs mt-2">
              Your key is stored locally and never sent to our servers. It is sent directly
              to the Venice API when you use the editor.
            </p>
          </section>

          {/* Model Selection */}
          <section>
            <label className="text-text-secondary text-xs font-medium uppercase tracking-wider block mb-3">
              AI Model
            </label>
            <select
              value={settings.selectedModel}
              onChange={(e) => update({ selectedModel: e.target.value })}
              className="w-full bg-bg-elevated border border-border rounded-xl px-4 py-2.5 text-text-primary text-sm focus:outline-none focus:border-accent/60 transition-colors"
            >
              {VENICE_MODELS.map((m) => (
                <option key={m.id} value={m.id}>
                  {m.name}
                </option>
              ))}
            </select>

            {selectedModelInfo && (
              <div
                className={`mt-2 px-3 py-2 rounded-lg text-xs ${
                  selectedModelInfo.tracksData
                    ? 'bg-red-900/20 border border-red-800/30 text-red-400'
                    : 'bg-accent/10 border border-accent/20 text-text-secondary'
                }`}
              >
                <span className="font-medium">
                  {selectedModelInfo.tracksData ? '⚠ Data tracking: ' : '✓ '}
                </span>
                {selectedModelInfo.trackingNote}
              </div>
            )}
          </section>

          {/* Editor */}
          <section>
            <label className="text-text-secondary text-xs font-medium uppercase tracking-wider block mb-3">
              Editor
            </label>
            <div className="space-y-3">
              <div className="flex items-center justify-between">
                <span className="text-text-secondary text-sm">Font size</span>
                <input
                  type="number"
                  min={10}
                  max={24}
                  value={settings.editorFontSize}
                  onChange={(e) => update({ editorFontSize: Number(e.target.value) })}
                  className="w-16 bg-bg-elevated border border-border rounded-lg px-2 py-1 text-text-primary text-sm text-center focus:outline-none focus:border-accent/60"
                />
              </div>
              <div className="flex items-center justify-between">
                <span className="text-text-secondary text-sm">Tab size</span>
                <select
                  value={settings.editorTabSize}
                  onChange={(e) => update({ editorTabSize: Number(e.target.value) })}
                  className="w-16 bg-bg-elevated border border-border rounded-lg px-2 py-1 text-text-primary text-sm focus:outline-none focus:border-accent/60"
                >
                  <option>2</option>
                  <option>4</option>
                </select>
              </div>
              <div className="flex items-center justify-between">
                <span className="text-text-secondary text-sm">Auto-save</span>
                <button
                  onClick={() => update({ autoSave: !settings.autoSave })}
                  className={`w-10 h-5 rounded-full transition-colors duration-200 flex items-center ${
                    settings.autoSave ? 'bg-accent' : 'bg-bg-hover'
                  }`}
                >
                  <span
                    className={`w-4 h-4 rounded-full bg-text-primary mx-0.5 transition-transform duration-200 ${
                      settings.autoSave ? 'translate-x-5' : 'translate-x-0'
                    }`}
                  />
                </button>
              </div>
            </div>
          </section>

          {/* Wallpaper */}
          <section>
            <label className="text-text-secondary text-xs font-medium uppercase tracking-wider block mb-3">
              Editor Wallpaper
            </label>
            <div className="space-y-3">
              {/* Upload image — metadata stripped before use */}
              <label className="flex items-center gap-3 cursor-pointer">
                <span className="text-text-secondary text-sm flex-shrink-0">Upload image</span>
                <div className="flex-1 px-3 py-1.5 rounded-lg border border-border border-dashed bg-bg-elevated text-text-muted text-xs text-center hover:border-accent/50 transition-colors">
                  click to choose · metadata auto-stripped
                </div>
                <input
                  type="file"
                  accept="image/*"
                  className="hidden"
                  onChange={async (e) => {
                    const file = e.target.files?.[0]
                    if (!file) return
                    const dataUrl = await readCleanDataUrl(file)
                    update({ wallpaperUrl: dataUrl })
                  }}
                />
              </label>
              <div className="flex items-center gap-3">
                <span className="text-text-secondary text-sm flex-shrink-0">Or URL</span>
                <input
                  type="text"
                  value={settings.wallpaperUrl.startsWith('data:') ? '' : settings.wallpaperUrl === '/wallpaper.jpg' ? '' : settings.wallpaperUrl}
                  onChange={(e) =>
                    update({ wallpaperUrl: e.target.value || '/wallpaper.jpg' })
                  }
                  placeholder="/wallpaper.jpg"
                  className="flex-1 bg-bg-elevated border border-border rounded-lg px-3 py-1.5 text-text-primary text-xs placeholder:text-text-muted focus:outline-none focus:border-accent/60"
                />
              </div>
              {settings.wallpaperUrl !== '/wallpaper.jpg' && (
                <button
                  onClick={() => update({ wallpaperUrl: '/wallpaper.jpg' })}
                  className="text-text-muted text-xs hover:text-text-primary transition-colors"
                >
                  Reset to default
                </button>
              )}
              <div className="flex items-center justify-between">
                <span className="text-text-secondary text-sm">
                  Opacity ({settings.wallpaperBrightness}%)
                </span>
                <input
                  type="range"
                  min={1}
                  max={20}
                  value={settings.wallpaperBrightness}
                  onChange={(e) => update({ wallpaperBrightness: Number(e.target.value) })}
                  className="w-28 accent-[#3a6449]"
                />
              </div>
            </div>
          </section>

          {/* GitHub Token */}
          <section>
            <label className="text-text-secondary text-xs font-medium uppercase tracking-wider block mb-3">
              GitHub Token (optional)
            </label>
            <div className="relative">
              <input
                type={showGhToken ? 'text' : 'password'}
                value={settings.githubToken}
                onChange={(e) => update({ githubToken: e.target.value })}
                placeholder="ghp_xxxxxxxxxxxx"
                className="w-full bg-bg-elevated border border-border rounded-xl px-4 py-2.5 text-text-primary text-sm placeholder:text-text-muted focus:outline-none focus:border-accent/60 pr-16 transition-colors"
              />
              <button
                onClick={() => setShowGhToken((s) => !s)}
                className="absolute right-3 top-1/2 -translate-y-1/2 text-text-muted text-xs hover:text-text-primary transition-colors"
              >
                {showGhToken ? 'Hide' : 'Show'}
              </button>
            </div>
            <p className="text-text-muted text-xs mt-2">
              Used to import private repositories. Stored locally only.
            </p>
          </section>
        </div>

        {/* Footer */}
        <div className="px-6 py-4 border-t border-border flex items-center justify-between">
          <p className="text-text-muted text-xs">Settings save to your browser only</p>
          <button
            onClick={handleSave}
            className="px-5 py-2 rounded-xl bg-accent hover:bg-accent-hover text-text-primary text-sm font-medium transition-all duration-200"
          >
            {saved ? 'Saved ✓' : 'Save'}
          </button>
        </div>
      </div>
    </div>
  )
}
