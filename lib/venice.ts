export interface VeniceModel {
  id: string
  name: string
  contextLength: number
  category: 'code' | 'general' | 'reasoning' | 'vision'
  tracksData: boolean
  trackingNote: string
}

// Venice AI model catalog with data tracking transparency
export const VENICE_MODELS: VeniceModel[] = [
  { id: 'llama-3.3-70b', name: 'Llama 3.3 70B', contextLength: 128000, category: 'general', tracksData: false, trackingNote: 'Venice routes this privately — no data logging by model provider' },
  { id: 'llama-3.1-405b', name: 'Llama 3.1 405B', contextLength: 128000, category: 'general', tracksData: false, trackingNote: 'Venice routes this privately — no data logging by model provider' },
  { id: 'deepseek-r1-671b', name: 'DeepSeek R1 671B', contextLength: 128000, category: 'reasoning', tracksData: false, trackingNote: 'Venice routes this privately — no data logging by model provider' },
  { id: 'deepseek-coder-v2', name: 'DeepSeek Coder V2', contextLength: 128000, category: 'code', tracksData: false, trackingNote: 'Venice routes this privately — no data logging by model provider' },
  { id: 'qwen-2.5-coder-32b', name: 'Qwen 2.5 Coder 32B', contextLength: 128000, category: 'code', tracksData: false, trackingNote: 'Venice routes this privately — no data logging by model provider' },
  { id: 'qwen-2.5-72b', name: 'Qwen 2.5 72B', contextLength: 128000, category: 'general', tracksData: false, trackingNote: 'Venice routes this privately — no data logging by model provider' },
  { id: 'mistral-31-24b', name: 'Mistral 3.1 24B', contextLength: 128000, category: 'general', tracksData: false, trackingNote: 'Venice routes this privately — no data logging by model provider' },
  { id: 'gemma-3-27b', name: 'Gemma 3 27B', contextLength: 128000, category: 'general', tracksData: false, trackingNote: 'Venice routes this privately — no data logging by model provider' },
  { id: 'llama-3.2-90b-vision', name: 'Llama 3.2 90B Vision', contextLength: 128000, category: 'vision', tracksData: false, trackingNote: 'Venice routes this privately — no data logging by model provider' },
  { id: 'qwen-2.5-vl-72b', name: 'Qwen 2.5 VL 72B', contextLength: 128000, category: 'vision', tracksData: false, trackingNote: 'Venice routes this privately — no data logging by model provider' }
]

export function getModelById(id: string): VeniceModel | undefined {
  return VENICE_MODELS.find(m => m.id === id)
}

export function getModelsByCategory(category: VeniceModel['category']): VeniceModel[] {
  return VENICE_MODELS.filter(m => m.category === category)
}

export type VeniceMessage = {
  role: 'system' | 'user' | 'assistant'
  content: string
}

// Legacy alias
export type ChatMessage = VeniceMessage

export function streamVeniceChat(
  messages: VeniceMessage[],
  model: string,
  apiKey: string,
  onChunk: (text: string) => void,
  onComplete?: () => void,
  onError?: (err: string) => void
): { abort: () => void } {
  const controller = new AbortController()

  const run = async () => {
    try {
      const response = await fetch('/api/ai', {
        method: 'POST',
        headers: { 'Content-Type': 'application/json' },
        body: JSON.stringify({ messages, model, apiKey }),
        signal: controller.signal,
      })

      if (!response.ok) {
        const err = await response.json().catch(() => ({ error: 'request failed' }))
        onError?.(err.error || `api error: ${response.status}`)
        return
      }

      const reader = response.body?.getReader()
      if (!reader) { onError?.('no response stream'); return }

      const decoder = new TextDecoder()
      let buffer = ''

      while (true) {
        const { done, value } = await reader.read()
        if (done) break

        buffer += decoder.decode(value, { stream: true })
        const lines = buffer.split('\n')
        buffer = lines.pop() ?? ''

        for (const line of lines) {
          if (!line.startsWith('data: ')) continue
          const data = line.slice(6).trim()
          if (data === '[DONE]') { onComplete?.(); return }
          try {
            const parsed = JSON.parse(data)
            const text = parsed.choices?.[0]?.delta?.content ?? ''
            if (text) onChunk(text)
          } catch {
            // malformed chunk, skip
          }
        }
      }
      onComplete?.()
    } catch (err: unknown) {
      if (err instanceof Error && err.name === 'AbortError') return
      onError?.(err instanceof Error ? err.message : 'unknown error')
    }
  }

  run()
  return { abort: () => controller.abort() }
}
