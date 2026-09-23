'use client'

import { useRef, useEffect, useState } from 'react'
import type { editor } from 'monaco-editor'

interface EditorProps {
  value: string
  onChange: (val: string) => void
  language: string
  wallpaperUrl: string
  wallpaperBrightness: number
  fontSize: number
  tabSize: number
  isAiEditing?: boolean
}

const SHEPHERD_THEME: editor.IStandaloneThemeData = {
  base: 'vs-dark',
  inherit: true,
  rules: [
    { token: 'comment', foreground: '445240', fontStyle: 'italic' },
    { token: 'keyword', foreground: '72876c' },
    { token: 'string', foreground: 'a0b49a' },
    { token: 'number', foreground: '8fa887' },
    { token: 'type', foreground: 'c4d4bc' },
    { token: 'function', foreground: 'd8e4d4' },
    { token: 'variable', foreground: 'b4c4ac' },
    { token: 'identifier', foreground: 'b4c4ac' },
    { token: 'operator', foreground: '607860' },
    { token: 'delimiter', foreground: '445240' },
  ],
  colors: {
    'editor.background': '#00000000',
    'editor.foreground': '#c4d4bc',
    'editor.lineHighlightBackground': '#1a231a40',
    'editor.selectionBackground': '#3a644960',
    'editor.inactiveSelectionBackground': '#3a644930',
    'editorCursor.foreground': '#c4d4bc',
    'editorLineNumber.foreground': '#2a3e2a',
    'editorLineNumber.activeForeground': '#607860',
    'editorIndentGuide.background': '#1a2a1a',
    'editorIndentGuide.activeBackground': '#2a4a2a',
    'editorWidget.background': '#0e130e',
    'editorWidget.border': '#1a2a1a',
    'editorSuggestWidget.background': '#0e130e',
    'editorSuggestWidget.border': '#1a2a1a',
    'editorSuggestWidget.selectedBackground': '#1a231a',
    'list.hoverBackground': '#1a231a',
    'scrollbarSlider.background': '#1a2a1a80',
    'scrollbarSlider.hoverBackground': '#2a4a2a80',
    'scrollbarSlider.activeBackground': '#3a644980',
  },
}

export default function Editor({
  value,
  onChange,
  language,
  wallpaperUrl,
  wallpaperBrightness,
  fontSize,
  tabSize,
  isAiEditing,
}: EditorProps) {
  const containerRef = useRef<HTMLDivElement>(null)
  const editorRef = useRef<editor.IStandaloneCodeEditor | null>(null)
  const [MonacoEditor, setMonacoEditor] = useState<typeof import('@monaco-editor/react').default | null>(null)

  useEffect(() => {
    import('@monaco-editor/react').then((mod) => setMonacoEditor(() => mod.default))
  }, [])

  const handleMount = (editorInstance: editor.IStandaloneCodeEditor, monaco: typeof import('monaco-editor')) => {
    editorRef.current = editorInstance
    monaco.editor.defineTheme('shepherd', SHEPHERD_THEME)
    monaco.editor.setTheme('shepherd')
    editorInstance.updateOptions({
      fontFamily: '"JetBrains Mono", "Fira Code", monospace',
      fontLigatures: true,
      fontSize,
      tabSize,
      smoothScrolling: true,
      cursorSmoothCaretAnimation: 'on',
      cursorBlinking: 'smooth',
      minimap: { enabled: false },
      scrollBeyondLastLine: false,
      padding: { top: 20, bottom: 20 },
      lineNumbers: 'on',
      renderWhitespace: 'selection',
      wordWrap: 'on',
    })
  }

  if (!MonacoEditor) {
    return (
      <div className="flex-1 flex items-center justify-center bg-transparent">
        <div className="flex gap-1.5">
          {[0, 1, 2].map((i) => (
            <span
              key={i}
              className="w-1.5 h-1.5 rounded-full bg-accent"
              style={{
                animation: 'typingDot 1.2s infinite',
                animationDelay: `${i * 0.2}s`,
              }}
            />
          ))}
        </div>
      </div>
    )
  }

  return (
    <div ref={containerRef} className="relative flex-1 min-h-0 overflow-hidden">
      {/* Wallpaper behind code */}
      <div
        className="absolute inset-0 bg-cover bg-center pointer-events-none z-0"
        style={{
          backgroundImage: `url(${wallpaperUrl})`,
          opacity: wallpaperBrightness / 100,
          filter: 'blur(0.5px)',
        }}
        aria-hidden
      />
      <div className="absolute inset-0 bg-bg-base/90 z-0 pointer-events-none" aria-hidden />

      {/* AI editing flash overlay */}
      {isAiEditing && (
        <div
          className="absolute inset-0 z-10 pointer-events-none rounded-sm"
          style={{ animation: 'codeEdit 0.4s ease-in-out' }}
          aria-hidden
        />
      )}

      <div className="relative z-[1] h-full">
        <MonacoEditor
          height="100%"
          language={language}
          value={value}
          onChange={(v) => onChange(v ?? '')}
          theme="shepherd"
          onMount={handleMount}
          options={{
            fontFamily: '"JetBrains Mono", monospace',
            fontSize,
            tabSize,
            minimap: { enabled: false },
            scrollBeyondLastLine: false,
            wordWrap: 'on',
            padding: { top: 20, bottom: 20 },
            cursorBlinking: 'smooth',
          }}
        />
      </div>
    </div>
  )
}
