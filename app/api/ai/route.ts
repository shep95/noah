import { NextRequest, NextResponse } from 'next/server'

export const runtime = 'edge'

export async function POST(req: NextRequest) {
  try {
    const body = await req.json()
    const { messages, model, apiKey, stream = true } = body

    if (!apiKey) {
      return NextResponse.json({ error: 'API key required' }, { status: 401 })
    }

    if (!messages || !Array.isArray(messages)) {
      return NextResponse.json({ error: 'Messages array required' }, { status: 400 })
    }

    const veniceRes = await fetch('https://api.venice.ai/api/v1/chat/completions', {
      method: 'POST',
      headers: {
        'Content-Type': 'application/json',
        Authorization: `Bearer ${apiKey}`,
      },
      body: JSON.stringify({
        model: model || 'llama-3.3-70b',
        messages,
        stream,
        temperature: 0.7,
        max_tokens: 4096,
      }),
    })

    if (!veniceRes.ok) {
      const err = await veniceRes.text()
      return NextResponse.json(
        { error: `Venice API error: ${veniceRes.status}`, detail: err },
        { status: veniceRes.status }
      )
    }

    if (!stream) {
      const data = await veniceRes.json()
      return NextResponse.json(data)
    }

    // Proxy the SSE stream directly
    const encoder = new TextEncoder()
    const readable = new ReadableStream({
      async start(controller) {
        const reader = veniceRes.body?.getReader()
        if (!reader) {
          controller.close()
          return
        }
        try {
          while (true) {
            const { done, value } = await reader.read()
            if (done) break
            controller.enqueue(value)
          }
        } catch {
          // stream ended
        } finally {
          controller.close()
        }
      },
    })

    return new Response(readable, {
      headers: {
        'Content-Type': 'text/event-stream',
        'Cache-Control': 'no-cache',
        Connection: 'keep-alive',
        'X-Accel-Buffering': 'no',
      },
    })
  } catch (err) {
    console.error('AI route error:', err)
    return NextResponse.json({ error: 'Internal server error' }, { status: 500 })
  }
}
