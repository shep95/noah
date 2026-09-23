'use client'

import { useState, useRef, useEffect, useCallback } from 'react'
import { buildSystemPrompt } from '@/lib/shepherd-brain'
import { streamVeniceChat, type VeniceMessage } from '@/lib/venice'
import { storePattern, buildPatternContext } from '@/lib/pattern-store'
import { loadSettings } from '@/lib/settings'

interface Message {
  id: string
  role: 'user' | 'assistant'
  content: string
  timestamp: number
}

interface AIPanelProps {
  activeFile: string | null
  fileContent: string
  language: string
  projectContext: string
  onApplyCode: (code: string) => void
  onAiEditingChange: (editing: boolean) => void
}

function TypingIndicator() {
  return (
    <div className="flex items-center gap-1 px-4 py-3">
      {[0, 1, 2].map((i) => (
        <span
          key={i}
          className="w-1.5 h-1.5 rounded-full bg-accent/60"
          style={{
            animation: 'typingDot 1.2s infinite',
            animationDelay: `${i * 0.2}s`,
          }}
        />
      ))}
    </div>
  )
}

function CodeBlock({ code, onApply }: { code: string; onApply: (c: string) => void }) {
  const [copied, setCopied] = useState(false)

  const copy = () => {
    navigator.clipboard.writeText(code)
    setCopied(true)
    setTimeout(() => setCopied(false), 1500)
  }

  return (
    <div className="mt-2 rounded-xl border border-border overflow-hidden">
      <div className="flex items-center justify-between px-3 py-1.5 bg-bg-elevated border-b border-border">
        <span className="text-text-muted text-xs font-mono">code</span>
        <div className="flex gap-2">
          <button
            onClick={copy}
            className="text-text-muted text-xs hover:text-text-primary transition-colors"
          >
            {copied ? 'Copied' : 'Copy'}
          </button>
          <button
            onClick={() => onApply(code)}
            className="text-accent text-xs hover:text-accent-hover transition-colors font-medium"
          >
            Apply
          </button>
        </div>
      </div>
      <pre className="p-3 text-xs font-mono text-text-secondary overflow-x-auto custom-scrollbar bg-bg-surface">
        {code}
      </pre>
    </div>
  )
}

function MessageBubble({ msg, onApply }: { msg: Message; onApply: (c: string) => void }) {
  const isUser = msg.role === 'user'

  // Extract code blocks
  const parts: { type: 'text' | 'code'; content: string }[] = []
  const codeRegex = /```[\w]*\n([\s\S]*?)```/g
  let last = 0
  let match
  while ((match = codeRegex.exec(msg.content)) !== null) {
    if (match.index > last) {
      parts.push({ type: 'text', content: msg.content.slice(last, match.index) })
    }
    parts.push({ type: 'code', content: match[1].trim() })
    last = match.index + match[0].length
  }
  if (last < msg.content.length) {
    parts.push({ type: 'text', content: msg.content.slice(last) })
  }

  return (
    <div className={`px-4 py-2 animate-fade-in ${isUser ? 'flex justify-end' : ''}`}>
      {isUser ? (
        <div className="max-w-xs px-3 py-2 rounded-xl bg-accent/20 border border-accent/20 text-text-primary text-sm">
          {msg.content}
        </div>
      ) : (
        <div className="text-text-secondary text-sm leading-relaxed">
          {parts.map((p, i) =>
            p.type === 'code' ? (
              <CodeBlock key={i} code={p.content} onApply={onApply} />
            ) : (
              <span key={i} className="whitespace-pre-wrap">
                {p.content}
              </span>
            )
          )}
        </div>
      )}
    </div>
  )
}

export default function AIPanel({
  activeFile,
  fileContent,
  language,
  projectContext,
  onApplyCode,
  onAiEditingChange,
}: AIPanelProps) {
  const [messages, setMessages] = useState<Message[]>([
    {
      id: '0',
      role: 'assistant',
      content:
        "I'm shepherd. Open a file and tell me what you want to build, fix, or refactor. I'll work within your project's patterns.",
      timestamp: Date.now(),
    },
  ])
  const [input, setInput] = useState('')
  const [streaming, setStreaming] = useState(false)
  const bottomRef = useRef<HTMLDivElement>(null)
  const abortRef = useRef<(() => void) | null>(null)

  useEffect(() => {
    bottomRef.current?.scrollIntoView({ behavior: 'smooth' })
  }, [messages, streaming])

  const send = useCallback(async () => {
    const text = input.trim()
    if (!text || streaming) return

    const settings = loadSettings()
    if (!settings.veniceApiKey) {
      setMessages((m) => [
        ...m,
        {
          id: Date.now().toString(),
          role: 'assistant',
          content: 'Please add your Venice API key in Settings (gear icon) to use shepherd.',
          timestamp: Date.now(),
        },
      ])
      return
    }

    const userMsg: Message = {
      id: Date.now().toString(),
      role: 'user',
      content: text,
      timestamp: Date.now(),
    }
    setMessages((m) => [...m, userMsg])
    setInput('')
    setStreaming(true)
    onAiEditingChange(true)

    const patternCtx = await buildPatternContext(activeFile || 'global')
    const systemPrompt = buildSystemPrompt(
      `File: ${activeFile || 'none'}\nLanguage: ${language}\n\n${projectContext}\n\n${patternCtx}\n\nCurrent file content:\n\`\`\`${language}\n${fileContent.slice(0, 6000)}\n\`\`\``
    )

    const historyMsgs: VeniceMessage[] = messages.slice(-10).map((m) => ({
      role: m.role,
      content: m.content,
    }))
    historyMsgs.push({ role: 'user', content: text })

    const assistantId = (Date.now() + 1).toString()
    setMessages((m) => [
      ...m,
      { id: assistantId, role: 'assistant', content: '', timestamp: Date.now() },
    ])

    let accumulated = ''
    try {
      const { abort } = streamVeniceChat(
        [{ role: 'system', content: systemPrompt }, ...historyMsgs],
        settings.selectedModel,
        settings.veniceApiKey,
        (chunk) => {
          accumulated += chunk
          setMessages((m) =>
            m.map((msg) =>
              msg.id === assistantId ? { ...msg, content: accumulated } : msg
            )
          )
        },
        async () => {
          setStreaming(false)
          onAiEditingChange(false)
          // Store any code suggestions as patterns
          const codeRegex = /```[\w]*\n([\s\S]*?)```/g
          let match
          while ((match = codeRegex.exec(accumulated)) !== null) {
            await storePattern({
              projectId: activeFile || 'global',
              type: 'architecture',
              trigger: text.slice(0, 200),
              correction: match[1].trim().slice(0, 500),
              language,
              confidence: 0.6,
            })
          }
        },
        (err) => {
          setStreaming(false)
          onAiEditingChange(false)
          setMessages((m) =>
            m.map((msg) =>
              msg.id === assistantId
                ? { ...msg, content: `Error: ${err}` }
                : msg
            )
          )
        }
      )
      abortRef.current = abort
    } catch {
      setStreaming(false)
      onAiEditingChange(false)
    }
  }, [input, streaming, messages, activeFile, fileContent, language, projectContext, onAiEditingChange])

  const handleKeyDown = (e: React.KeyboardEvent) => {
    if (e.key === 'Enter' && !e.shiftKey) {
      e.preventDefault()
      send()
    }
  }

  return (
    <div className="flex flex-col h-full">
      {/* Header */}
      <div className="px-4 py-3 border-b border-border flex items-center justify-between flex-shrink-0">
        <div className="flex items-center gap-2">
          <div className="w-2 h-2 rounded-full bg-accent animate-pulse-slow" />
          <span className="text-text-primary text-xs font-medium">shepherd</span>
        </div>
        {streaming && (
          <button
            onClick={() => { abortRef.current?.(); setStreaming(false); onAiEditingChange(false) }}
            className="text-text-muted text-xs hover:text-text-primary transition-colors"
          >
            stop
          </button>
        )}
      </div>

      {/* Messages */}
      <div className="flex-1 overflow-y-auto custom-scrollbar py-2">
        {messages.map((msg) => (
          <MessageBubble
            key={msg.id}
            msg={msg}
            onApply={(code) => {
              onApplyCode(code)
              onAiEditingChange(true)
              setTimeout(() => onAiEditingChange(false), 600)
            }}
          />
        ))}
        {streaming && <TypingIndicator />}
        <div ref={bottomRef} />
      </div>

      {/* Input */}
      <div className="p-3 border-t border-border flex-shrink-0">
        <div className="flex gap-2 items-end">
          <textarea
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={handleKeyDown}
            placeholder={activeFile ? `Ask shepherd about ${activeFile.split('/').pop()}…` : 'Ask shepherd anything…'}
            rows={1}
            className="flex-1 bg-bg-elevated border border-border rounded-xl px-3 py-2.5 text-text-primary text-sm placeholder:text-text-muted resize-none focus:outline-none focus:border-accent/60 transition-colors custom-scrollbar"
            style={{ maxHeight: '120px' }}
            onInput={(e) => {
              const t = e.target as HTMLTextAreaElement
              t.style.height = 'auto'
              t.style.height = Math.min(t.scrollHeight, 120) + 'px'
            }}
          />
          <button
            onClick={send}
            disabled={!input.trim() || streaming}
            className="w-9 h-9 rounded-xl bg-accent hover:bg-accent-hover disabled:opacity-30 disabled:cursor-not-allowed flex items-center justify-center transition-all duration-200 flex-shrink-0"
          >
            <svg width="14" height="14" viewBox="0 0 14 14" fill="none">
              <path
                d="M2 7H12M8 3L12 7L8 11"
                stroke="#c4d4bc"
                strokeWidth="1.5"
                strokeLinecap="round"
                strokeLinejoin="round"
              />
            </svg>
          </button>
        </div>
        <p className="text-text-muted text-[10px] mt-1.5 px-1">
          Enter to send · Shift+Enter for newline
        </p>
      </div>
    </div>
  )
}
