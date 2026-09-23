'use client'

interface StatusBarProps {
  activeFile: string | null
  language: string
  lineCount: number
  model: string
  isAiEditing: boolean
  projectName: string | null
}

export default function StatusBar({
  activeFile,
  language,
  lineCount,
  model,
  isAiEditing,
  projectName,
}: StatusBarProps) {
  return (
    <div className="h-6 border-t border-border bg-bg-surface/90 flex items-center justify-between px-4 text-[11px] text-text-muted flex-shrink-0">
      <div className="flex items-center gap-4">
        {projectName && (
          <span className="text-text-secondary">
            {projectName}
          </span>
        )}
        {activeFile && (
          <span className="truncate max-w-64">{activeFile.split('/').slice(-2).join('/')}</span>
        )}
      </div>

      <div className="flex items-center gap-4">
        {isAiEditing && (
          <div className="flex items-center gap-1.5 text-accent">
            <span
              className="w-1 h-1 rounded-full bg-accent"
              style={{ animation: 'typingDot 1.2s infinite' }}
            />
            <span>shepherd editing…</span>
          </div>
        )}
        <span>{language || 'plaintext'}</span>
        <span>{lineCount} lines</span>
        <span className="hidden sm:block truncate max-w-40">{model}</span>
      </div>
    </div>
  )
}
