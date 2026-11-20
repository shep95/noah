import { NextResponse, type NextRequest } from 'next/server'

// noah shield asks this after it brings its tunnel up, to show where the
// browser's traffic now comes out. The answer is what Vercel already knows
// about the connection; nothing is stored and nothing else is read.
// No CORS: the shield reads this with its own host permission, and no other
// site needs to learn a visitor's address and city from here.
const HEADERS = {
  'Cache-Control': 'no-store',
  'X-Robots-Tag': 'noindex',
}

// A header is not guaranteed to be well-formed percent-encoding.
function safeDecode(value: string): string {
  try {
    return decodeURIComponent(value)
  } catch {
    return value
  }
}

export function GET(request: NextRequest) {
  const forwarded = request.headers.get('x-forwarded-for') || ''
  const ip = request.headers.get('x-real-ip') || forwarded.split(',')[0].trim()
  return NextResponse.json(
    {
      ip,
      country: request.headers.get('x-vercel-ip-country') || '',
      region: request.headers.get('x-vercel-ip-country-region') || '',
      city: safeDecode(request.headers.get('x-vercel-ip-city') || ''),
    },
    { headers: HEADERS },
  )
}

