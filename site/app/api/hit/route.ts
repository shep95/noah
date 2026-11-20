import { NextResponse, type NextRequest } from 'next/server'
import { recordHit } from '@/lib/stats'

// The beacon from components/Analytics.tsx lands here. It answers nothing
// but a status; failures in the store are logged, never shown to visitors.
const NO_BODY = { 'Cache-Control': 'no-store', 'X-Robots-Tag': 'noindex' }

const MAX_BODY_BYTES = 2000

export async function POST(request: NextRequest) {
  // The declared size is checked before the body is read, so a large body
  // never has to be buffered to be refused.
  const declared = Number(request.headers.get('content-length') || '0')
  if (!(declared <= MAX_BODY_BYTES)) return new NextResponse(null, { status: 413, headers: NO_BODY })
  // A chunked body has no declared size; it is read with a byte cap.
  const text = await readCapped(request, MAX_BODY_BYTES)
  if (text === null) return new NextResponse(null, { status: 413, headers: NO_BODY })
  let payload: unknown
  try {
    payload = JSON.parse(text)
  } catch {
    return new NextResponse(null, { status: 400, headers: NO_BODY })
  }
  try {
    await recordHit(payload, {
      country: request.headers.get('x-vercel-ip-country'),
      region: request.headers.get('x-vercel-ip-country-region'),
      userAgent: request.headers.get('user-agent'),
      host: request.headers.get('host'),
      address: request.headers.get('x-real-ip') || (request.headers.get('x-forwarded-for') || '').split(',')[0].trim(),
    })
  } catch (error) {
    console.error('stats: could not record a hit', error)
  }
  return new NextResponse(null, { status: 204, headers: NO_BODY })
}

async function readCapped(request: NextRequest, limit: number): Promise<string | null> {
  const body = request.body
  if (!body) return ''
  const reader = body.getReader()
  const chunks: Uint8Array[] = []
  let size = 0
  for (;;) {
    const { value, done } = await reader.read()
    if (done) break
    size += value.byteLength
    if (size > limit) {
      await reader.cancel().catch(() => {})
      return null
    }
    chunks.push(value)
  }
  const joined = new Uint8Array(size)
  let offset = 0
  for (const chunk of chunks) {
    joined.set(chunk, offset)
    offset += chunk.byteLength
  }
  return new TextDecoder().decode(joined)
}

export function GET() {
  return new NextResponse(null, { status: 405, headers: { ...NO_BODY, Allow: 'POST' } })
}
