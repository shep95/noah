'use client'

import { useState, useCallback, useEffect, useRef } from 'react'
import dynamic from 'next/dynamic'
import FileTree, { type FileNode } from './FileTree'
import AIPanel from './AIPanel'
import StatusBar from './StatusBar'
import Settings from './Settings'
import { loadSettings } from '@/lib/settings'
import { buildProjectProfile, buildProjectContextString, type FileNode as AnalyzerFile } from '@/lib/project-analyzer'
import { saveProjectProfile } from '@/lib/pattern-store'
import { detectLanguage } from '@/lib/project-analyzer'
import { readCleanText } from '@/lib/strip-metadata'

const Editor = dynamic(() => import('./Editor'), { ssr: false })

interface IDELayoutProps {
  wallpaperUrl: string
  wallpaperBrightness: number
}

const STARTER_FILE = `// Welcome to unlocket
// shepherd is ready — open a file or ask anything below

function greet(name: string): string {
  return \`Hello, \${name}!\`
}

console.log(greet('world'))
`

export default function IDELayout({ wallpaperUrl, wallpaperBrightness }: IDELayoutProps) {
  const settings = loadSettings()

  const [files, setFiles] = useState<FileNode[]>([
    { name: 'index.ts', path: 'index.ts', type: 'file', content: STARTER_FILE },
  ])
  const [activeFile, setActiveFile] = useState<string | null>('index.ts')
  const [activeContent, setActiveContent] = useState(STARTER_FILE)
  const [activeLanguage, setActiveLanguage] = useState('typescript')
  const [isAiEditing, setIsAiEditing] = useState(false)
  const [showSettings, setShowSettings] = useState(false)
  const [showImport, setShowImport] = useState(false)
  const [projectContext, setProjectContext] = useState('')
  const [projectName, setProjectName] = useState<string | null>(null)
  const [sidebarWidth, setSidebarWidth] = useState(200)
  const [aiPanelWidth, setAiPanelWidth] = useState(300)
  const [isImporting, setIsImporting] = useState(false)
  const [importError, setImportError] = useState('')
  const [ghOwner, setGhOwner] = useState('')
  const [ghRepo, setGhRepo] = useState('')
  const autoSaveRef = useRef<ReturnType<typeof setTimeout> | null>(null)
  const currentSettings = useRef(settings)

  useEffect(() => {
    currentSettings.current = loadSettings()
  }, [showSettings])

  const handleSelectFile = useCallback((path: string, content: string, language: string) => {
    setActiveFile(path)
    setActiveContent(content)
    setActiveLanguage(language)
  }, [])

  const handleContentChange = useCallback((val: string) => {
    setActiveContent(val)
    setFiles((prev) =>
      updateFileContent(prev, activeFile || '', val)
    )
    // Auto-save debounce
    if (currentSettings.current.autoSave) {
      if (autoSaveRef.current) clearTimeout(autoSaveRef.current)
      autoSaveRef.current = setTimeout(() => {
        // Files are stored in state — in a real app would persist here
      }, 1500)
    }
  }, [activeFile])

  const handleApplyCode = useCallback((code: string) => {
    setActiveContent(code)
    setFiles((prev) => updateFileContent(prev, activeFile || '', code))
  }, [activeFile])

  const importFromGitHub = async () => {
    if (!ghOwner || !ghRepo) return
    const s = loadSettings()
    if (!s.githubToken) {
      setImportError('Add a GitHub token in Settings first.')
      return
    }
    setIsImporting(true)
    setImportError('')
    try {
      const res = await fetch('/api/github', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ owner: ghOwner, repo: ghRepo, token: s.githubToken }),
      })
      const data = await res.json()
      if (!res.ok) {
        setImportError(data.error || 'Import failed')
        return
      }

      // Build file tree
      const tree = buildFileTree(data.files)
      setFiles(tree)
      setProjectName(data.name)

      // Build project profile for shepherd
      const analyzerFiles: AnalyzerFile[] = data.files.map((f: { path: string; content: string; language: string }) => ({
        name: f.path.split('/').pop() || f.path,
        path: f.path,
        type: 'file' as const,
        content: f.content,
        language: f.language,
      }))
      const profileBase = buildProjectProfile(data.name, analyzerFiles)
      const ctx = buildProjectContextString(profileBase)
      setProjectContext(ctx)
      await saveProjectProfile({ ...profileBase, id: `${data.name}_${Date.now()}` })

      // Select first file
      if (data.files.length > 0) {
        const first = data.files[0]
        setActiveFile(first.path)
        setActiveContent(first.content)
        setActiveLanguage(first.language || detectLanguage(first.path))
      }

      setShowImport(false)
    } catch (e) {
      setImportError('Network error. Try again.')
    } finally {
      setIsImporting(false)
    }
  }

  const handleLocalImport = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const file = e.target.files?.[0]
    if (!file) return
    // Strip all metadata (EXIF, timestamps, etc.) before reading content
    const content = await readCleanText(file)
    const lang = detectLanguage(file.name)
    const node: FileNode = {
      name: file.name,
      path: file.name,
      type: 'file',
      content,
    }
    setFiles((prev) => [...prev, node])
    setActiveFile(file.name)
    setActiveContent(content)
    setActiveLanguage(lang)
    // Reset input so same file can be re-imported
    e.target.value = ''
  }

  const s = loadSettings()

  return (
    <div className="flex flex-col h-screen overflow-hidden bg-bg-base">
      {/* Wallpaper behind entire IDE */}
      <div
        className="fixed inset-0 bg-cover bg-center pointer-events-none z-0"
        style={{
          backgroundImage: `url(${wallpaperUrl})`,
          opacity: (wallpaperBrightness / 100) * 0.4,
          filter: 'blur(2px)',
        }}
        aria-hidden
      />
      <div className="fixed inset-0 bg-bg-base/85 pointer-events-none z-0" aria-hidden />

      {/* Toolbar */}
      <div className="relative z-10 flex items-center gap-2 px-4 py-2 border-b border-border bg-bg-surface/90 backdrop-blur-sm flex-shrink-0">
        <div className="flex items-center gap-1.5 mr-2">
          <div className="w-5 h-5 rounded bg-accent flex items-center justify-center">
            <svg width="10" height="10" viewBox="0 0 10 10" fill="none">
              <path d="M5 1L8.5 3V7L5 9L1.5 7V3L5 1Z" stroke="#c4d4bc" strokeWidth="1.2" fill="none" />
            </svg>
          </div>
          <span className="text-text-primary text-xs font-medium">
            {projectName || 'unlocket'}
          </span>
        </div>

        <div className="flex items-center gap-1 ml-auto">
          <button
            onClick={() => setShowImport(true)}
            className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-text-secondary text-xs hover:bg-bg-hover hover:text-text-primary transition-colors"
          >
            <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
              <path d="M6 1V8M3 5L6 8L9 5" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
              <path d="M1 10H11" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
            </svg>
            Import
          </button>

          <label className="flex items-center gap-1.5 px-3 py-1.5 rounded-lg text-text-secondary text-xs hover:bg-bg-hover hover:text-text-primary transition-colors cursor-pointer">
            <svg width="12" height="12" viewBox="0 0 12 12" fill="none">
              <rect x="2" y="2" width="8" height="8" rx="1" stroke="currentColor" strokeWidth="1.3" />
              <path d="M6 4V8M4 6H8" stroke="currentColor" strokeWidth="1.3" strokeLinecap="round" />
            </svg>
            File
            <input type="file" className="hidden" onChange={handleLocalImport} accept="*/*" />
          </label>

          <button
            onClick={() => setShowSettings(true)}
            className="w-7 h-7 rounded-lg text-text-secondary hover:bg-bg-hover hover:text-text-primary transition-colors flex items-center justify-center"
            title="Settings"
          >
            <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
              <circle cx="7" cy="7" r="2" stroke="currentColor" strokeWidth="1.2" />
              <path
                d="M7 1.5V3M7 11V12.5M1.5 7H3M11 7H12.5M3.05 3.05L4.11 4.11M9.89 9.89L10.95 10.95M3.05 10.95L4.11 9.89M9.89 4.11L10.95 3.05"
                stroke="currentColor"
                strokeWidth="1.2"
                strokeLinecap="round"
              />
            </svg>
          </button>
        </div>
      </div>

      {/* Main area */}
      <div className="relative z-10 flex flex-1 min-h-0">
        {/* File tree */}
        <div
          className="border-r border-border bg-bg-surface/80 flex-shrink-0 overflow-hidden"
          style={{ width: sidebarWidth }}
        >
          <div className="px-3 py-2 border-b border-border">
            <span className="text-text-muted text-[10px] uppercase tracking-wider">Explorer</span>
          </div>
          <FileTree
            files={files}
            activeFile={activeFile}
            onSelectFile={handleSelectFile}
          />
        </div>

        {/* Editor */}
        <div className="flex-1 min-w-0 flex flex-col min-h-0">
          {/* Tab bar */}
          {activeFile && (
            <div className="flex items-center gap-0 border-b border-border bg-bg-surface/80 flex-shrink-0 overflow-x-auto">
              <div className="flex items-center gap-1 px-3 py-2 border-r border-border bg-bg-elevated/60 text-text-primary text-xs whitespace-nowrap">
                <span>{activeFile.split('/').pop()}</span>
              </div>
            </div>
          )}
          <Editor
            value={activeContent}
            onChange={handleContentChange}
            language={activeLanguage}
            wallpaperUrl={wallpaperUrl}
            wallpaperBrightness={wallpaperBrightness}
            fontSize={s.editorFontSize}
            tabSize={s.editorTabSize}
            isAiEditing={isAiEditing}
          />
        </div>

        {/* AI Panel */}
        <div
          className="border-l border-border bg-bg-surface/80 flex-shrink-0 overflow-hidden flex flex-col"
          style={{ width: aiPanelWidth }}
        >
          <AIPanel
            activeFile={activeFile}
            fileContent={activeContent}
            language={activeLanguage}
            projectContext={projectContext}
            onApplyCode={handleApplyCode}
            onAiEditingChange={setIsAiEditing}
          />
        </div>
      </div>

      {/* Status bar */}
      <div className="relative z-10">
        <StatusBar
          activeFile={activeFile}
          language={activeLanguage}
          lineCount={activeContent.split('\n').length}
          model={s.selectedModel}
          isAiEditing={isAiEditing}
          projectName={projectName}
        />
      </div>

      {/* Modals */}
      {showSettings && <Settings onClose={() => setShowSettings(false)} />}

      {showImport && (
        <div
          className="fixed inset-0 z-50 flex items-center justify-center p-4"
          onClick={(e) => e.target === e.currentTarget && setShowImport(false)}
        >
          <div className="absolute inset-0 bg-bg-base/80 backdrop-blur-sm" onClick={() => setShowImport(false)} />
          <div className="relative z-10 w-full max-w-md bg-bg-surface border border-border rounded-2xl overflow-hidden shadow-2xl animate-fade-up p-6">
            <h3 className="text-text-primary font-medium text-sm mb-1">Import from GitHub</h3>
            <p className="text-text-muted text-xs mb-4">
              Shepherd will read your repository and match your patterns.
            </p>
            <div className="space-y-3">
              <input
                type="text"
                value={ghOwner}
                onChange={(e) => setGhOwner(e.target.value)}
                placeholder="owner"
                className="w-full bg-bg-elevated border border-border rounded-xl px-4 py-2.5 text-text-primary text-sm placeholder:text-text-muted focus:outline-none focus:border-accent/60"
              />
              <input
                type="text"
                value={ghRepo}
                onChange={(e) => setGhRepo(e.target.value)}
                placeholder="repository"
                className="w-full bg-bg-elevated border border-border rounded-xl px-4 py-2.5 text-text-primary text-sm placeholder:text-text-muted focus:outline-none focus:border-accent/60"
              />
              {importError && (
                <p className="text-red-400 text-xs">{importError}</p>
              )}
              <button
                onClick={importFromGitHub}
                disabled={isImporting || !ghOwner || !ghRepo}
                className="w-full py-2.5 rounded-xl bg-accent hover:bg-accent-hover disabled:opacity-40 text-text-primary text-sm font-medium transition-all"
              >
                {isImporting ? 'Importing…' : 'Import Repository'}
              </button>
            </div>
          </div>
        </div>
      )}
    </div>
  )
}

function updateFileContent(nodes: FileNode[], path: string, content: string): FileNode[] {
  return nodes.map((node) => {
    if (node.path === path) return { ...node, content }
    if (node.children) return { ...node, children: updateFileContent(node.children, path, content) }
    return node
  })
}

function buildFileTree(files: { path: string; content: string; language: string }[]): FileNode[] {
  const root: FileNode[] = []
  const map = new Map<string, FileNode>()

  for (const f of files) {
    const parts = f.path.split('/')
    let current = root

    for (let i = 0; i < parts.length; i++) {
      const part = parts[i]
      const currentPath = parts.slice(0, i + 1).join('/')

      if (i === parts.length - 1) {
        const node: FileNode = {
          name: part,
          path: f.path,
          type: 'file',
          content: f.content,
          language: f.language,
        }
        current.push(node)
      } else {
        if (!map.has(currentPath)) {
          const dirNode: FileNode = {
            name: part,
            path: currentPath,
            type: 'dir',
            children: [],
          }
          map.set(currentPath, dirNode)
          current.push(dirNode)
        }
        current = map.get(currentPath)!.children!
      }
    }
  }

  return root
}
