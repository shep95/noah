'use client'

import { useState } from 'react'
import { detectLanguage } from '@/lib/project-analyzer'

export interface FileNode {
  name: string
  path: string
  type: 'file' | 'dir'
  content?: string
  children?: FileNode[]
  language?: string
}

interface FileTreeProps {
  files: FileNode[]
  activeFile: string | null
  onSelectFile: (path: string, content: string, language: string) => void
}

function FileIcon({ name }: { name: string }) {
  const ext = name.split('.').pop()?.toLowerCase() || ''
  const colors: Record<string, string> = {
    ts: '#72876c', tsx: '#72876c', js: '#a0b49a', jsx: '#a0b49a',
    py: '#8fa887', rs: '#c4d4bc', go: '#a0b49a', css: '#607860',
    html: '#8fa887', json: '#607860', md: '#72876c', yml: '#607860',
    yaml: '#607860',
  }
  const color = colors[ext] || '#445240'
  return (
    <span style={{ color, fontSize: 11 }}>
      {ext ? ext.slice(0, 2).toUpperCase() : '·'}
    </span>
  )
}

function DirIcon({ open }: { open: boolean }) {
  return (
    <svg
      width="10"
      height="10"
      viewBox="0 0 10 10"
      className={`transition-transform duration-150 ${open ? 'rotate-90' : ''}`}
      fill="none"
    >
      <path d="M3 2L7 5L3 8" stroke="#445240" strokeWidth="1.3" strokeLinecap="round" />
    </svg>
  )
}

function TreeNode({
  node,
  depth,
  activeFile,
  onSelectFile,
}: {
  node: FileNode
  depth: number
  activeFile: string | null
  onSelectFile: (path: string, content: string, language: string) => void
}) {
  const [open, setOpen] = useState(depth < 2)

  if (node.type === 'dir') {
    return (
      <div>
        <button
          onClick={() => setOpen((o) => !o)}
          className="flex items-center gap-1.5 w-full px-2 py-0.5 hover:bg-bg-hover rounded text-left transition-colors"
          style={{ paddingLeft: `${8 + depth * 12}px` }}
        >
          <DirIcon open={open} />
          <span className="text-text-muted text-xs">{node.name}</span>
        </button>
        {open && node.children?.map((child) => (
          <TreeNode
            key={child.path}
            node={child}
            depth={depth + 1}
            activeFile={activeFile}
            onSelectFile={onSelectFile}
          />
        ))}
      </div>
    )
  }

  const isActive = activeFile === node.path
  return (
    <button
      onClick={() => {
        const lang = detectLanguage(node.name)
        onSelectFile(node.path, node.content || '', lang)
      }}
      className={`flex items-center gap-2 w-full px-2 py-0.5 rounded text-left transition-colors text-xs ${
        isActive
          ? 'bg-accent/20 text-text-primary'
          : 'text-text-secondary hover:bg-bg-hover hover:text-text-primary'
      }`}
      style={{ paddingLeft: `${8 + depth * 12}px` }}
    >
      <FileIcon name={node.name} />
      <span className="truncate">{node.name}</span>
    </button>
  )
}

export default function FileTree({ files, activeFile, onSelectFile }: FileTreeProps) {
  if (files.length === 0) {
    return (
      <div className="p-4 text-text-muted text-xs text-center leading-relaxed">
        No files open.
        <br />
        Import a project or create a new file.
      </div>
    )
  }

  return (
    <div className="py-2 overflow-y-auto h-full custom-scrollbar">
      {files.map((node) => (
        <TreeNode
          key={node.path}
          node={node}
          depth={0}
          activeFile={activeFile}
          onSelectFile={onSelectFile}
        />
      ))}
    </div>
  )
}
