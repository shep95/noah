import { LINKS } from '@/lib/site'
import { isLanguage, splitLanguage } from '@/lib/i18n'

// Visit counting lives in one Redis hash per day, reached over Upstash's REST
// api with plain fetch: no client library, and nothing about a visitor is
// stored, only counters. Vercel's Upstash integration sets the two variables;
// the legacy Vercel KV names are honoured too.
const DAY_PREFIX = 'stats:d:'
const TOTAL_KEY = 'stats:total'
const KEEP_DAYS = 400
const DAY_SECONDS = 24 * 60 * 60

function credentials(): { url: string; token: string } | null {
  const url = process.env.KV_REST_API_URL || process.env.UPSTASH_REDIS_REST_URL
  const token = process.env.KV_REST_API_TOKEN || process.env.UPSTASH_REDIS_REST_TOKEN
  if (!url || !token) return null
  return { url: url.replace(/\/$/, ''), token }
}

export function statsConfigured(): boolean {
  return credentials() !== null
}

async function pipeline(commands: (string | number)[][]): Promise<unknown[]> {
  const access = credentials()
  if (!access || commands.length === 0) return []
  // A slow store must not hold a page request open for the platform's whole
  // limit; four seconds is longer than any healthy answer.
  const response = await fetch(`${access.url}/pipeline`, {
    method: 'POST',
    headers: { Authorization: `Bearer ${access.token}`, 'content-type': 'application/json' },
    body: JSON.stringify(commands),
    cache: 'no-store',
    signal: AbortSignal.timeout(4000),
  })
  if (!response.ok) throw new Error(`stats store answered ${response.status}`)
  const results = (await response.json()) as { result?: unknown; error?: string }[]
  return results.map((entry) => entry.result)
}

function dayOf(date: Date): string {
  return date.toISOString().slice(0, 10)
}

// Referrers are reduced to the app or site they name, so the page can say
// "instagram" rather than list every l.instagram.com variant.
const SOURCES: [RegExp, string][] = [
  [/(^|\.)instagram\.com$/, 'instagram'],
  [/(^|\.)(facebook\.com|fb\.com|messenger\.com)$/, 'facebook'],
  [/(^|\.)threads\.(net|com)$/, 'threads'],
  [/(^|\.)(twitter\.com|x\.com|t\.co)$/, 'x'],
  [/(^|\.)tiktok\.com$/, 'tiktok'],
  [/(^|\.)(youtube\.com|youtu\.be)$/, 'youtube'],
  [/(^|\.)(reddit\.com|redd\.it)$/, 'reddit'],
  [/(^|\.)(discord\.com|discordapp\.com|discord\.gg)$/, 'discord'],
  [/(^|\.)(t\.me|telegram\.(org|me))$/, 'telegram'],
  [/(^|\.)(whatsapp\.com|wa\.me)$/, 'whatsapp'],
  [/(^|\.)(linkedin\.com|lnkd\.in)$/, 'linkedin'],
  [/(^|\.)pinterest\.[a-z.]+$/, 'pinterest'],
  [/(^|\.)snapchat\.com$/, 'snapchat'],
  [/(^|\.)github\.com$/, 'github'],
  [/(^|\.)news\.ycombinator\.com$/, 'hacker news'],
  [/(^|\.)producthunt\.com$/, 'product hunt'],
  [/(^|\.)google\.[a-z.]+$/, 'google'],
  [/(^|\.)bing\.com$/, 'bing'],
  [/(^|\.)duckduckgo\.com$/, 'duckduckgo'],
  [/(^|\.)yandex\.[a-z]+$/, 'yandex'],
  [/(^|\.)baidu\.com$/, 'baidu'],
  [/(^|\.)ecosia\.org$/, 'ecosia'],
  [/(^|\.)brave\.com$/, 'brave search'],
  [/(^|\.)(chatgpt\.com|openai\.com)$/, 'chatgpt'],
  [/(^|\.)perplexity\.ai$/, 'perplexity'],
  [/(^|\.)claude\.ai$/, 'claude'],
  [/(^|\.)asherin\.com$/, 'asherin.com'],
]

// Apps that open links in their own web view name themselves in the user
// agent even when they send no referrer.
const IN_APP: [RegExp, string][] = [
  [/Instagram/i, 'instagram'],
  [/FBAN|FBAV|FB_IAB/i, 'facebook'],
  [/Barcelona/i, 'threads'],
  [/TikTok|musical_ly|BytedanceWebview/i, 'tiktok'],
  [/Twitter/i, 'x'],
  [/Discord/i, 'discord'],
  [/Telegram/i, 'telegram'],
  [/WhatsApp/i, 'whatsapp'],
  [/LinkedInApp/i, 'linkedin'],
  [/Snapchat/i, 'snapchat'],
  [/Pinterest/i, 'pinterest'],
  [/Reddit/i, 'reddit'],
  [/\bLine\//i, 'line'],
]

const BOT = /bot|crawl|spider|slurp|headless|lighthouse|pingdom|monitor|preview|fetch|scan|python|curl|wget/i

function clean(value: unknown, limit: number): string {
  if (typeof value !== 'string') return ''
  return value.replace(/[\s\u0000-\u001f]+/g, ' ').trim().slice(0, limit)
}

function sourceOf(referrer: string, campaign: string, userAgent: string, ownHost: string): string {
  const tagged = campaign.toLowerCase().replace(/[^a-z0-9._ -]/g, '').slice(0, 32)
  if (tagged) return tagged
  for (const [pattern, name] of IN_APP) if (pattern.test(userAgent)) return name
  if (!referrer) return 'direct'
  let host = ''
  try {
    host = new URL(referrer).hostname.toLowerCase().replace(/^www\./, '')
  } catch {
    return 'direct'
  }
  if (!host || host === ownHost) return 'direct'
  for (const [pattern, name] of SOURCES) if (pattern.test(host)) return name
  return host.slice(0, 40)
}

export type HitContext = {
  country: string | null
  region: string | null
  userAgent: string | null
  host: string | null
  address: string | null
}

// One browser sends a handful of hits a minute; a script sends thousands.
// The count lives in the same store as the stats, keyed by address and
// minute, so it holds across serverless instances. An IPv6 sender is
// bucketed by its /64, since one person holds the whole block; a sender
// with no address shares one bucket. A store that answers anything but a
// number is treated as over the limit: the hit is dropped, never counted
// unchecked.
const HITS_PER_MINUTE = 30

function bucketOf(address: string | null): string {
  if (!address) return 'unknown'
  if (address.includes(':')) return address.split(':').slice(0, 4).join(':')
  return address
}

async function overLimit(address: string | null): Promise<boolean> {
  const key = `rl:${bucketOf(address)}:${Math.floor(Date.now() / 60000)}`
  const [count] = await pipeline([
    ['INCR', key],
    ['EXPIRE', key, 120],
  ])
  return typeof count !== 'number' || count > HITS_PER_MINUTE
}

// The site's own routes and files, and the exact outside links it carries.
// Anything else is text a visitor could plant for the stats page to show
// everyone, so it is not recorded.
const OWN_ROUTES = new Set(['/', '/compare', '/download', '/faq', '/founder', '/local-models', '/security', '/shepherd', '/shield', '/stats', '/terms'])
const OWN_FILE = /^\/(downloads|shield)\/[a-z0-9._-]+\.(exe|deb|tar\.xz|dmg|zip|xpi|sha256)$/
const OWN_LINKS = new Set([LINKS.discord, LINKS.asherin, LINKS.source, LINKS.instagram, LINKS.twitter])

function isOwnTarget(target: string, ownHost: string): boolean {
  if (target.startsWith('/')) {
    const path = target.split(/[?#]/)[0]
    const { path: bare } = splitLanguage(path)
    return OWN_ROUTES.has(bare) || OWN_FILE.test(bare)
  }
  try {
    const url = new URL(target)
    const host = url.hostname.toLowerCase().replace(/^www\./, '')
    if (host === ownHost.replace(/^www\./, '')) return isOwnTarget(url.pathname + url.search, ownHost)
    return OWN_LINKS.has(url.href) || OWN_LINKS.has(url.origin + url.pathname)
  } catch {
    return false
  }
}

export async function recordHit(payload: unknown, context: HitContext): Promise<void> {
  if (!statsConfigured() || typeof payload !== 'object' || payload === null) return
  const userAgent = context.userAgent || ''
  if (BOT.test(userAgent)) return
  if (await overLimit(context.address)) return
  const data = payload as Record<string, unknown>
  const kind = clean(data.kind, 8)
  const { language: pathLanguage, path } = splitLanguage(clean(data.path, 120) || '/')
  const pagePath = path.startsWith('/') ? path.split('?')[0].slice(0, 80) : '/'
  const now = new Date()
  const day = `${DAY_PREFIX}${dayOf(now)}`
  const commands: (string | number)[][] = []
  const count = (field: string) => {
    commands.push(['HINCRBY', day, field, 1])
    commands.push(['HINCRBY', TOTAL_KEY, field, 1])
  }

  if (kind === 'view') {
    const country = (context.country || 'ZZ').toUpperCase().slice(0, 2)
    const region = clean(context.region, 8).toUpperCase()
    const language = clean(data.language, 5)
    const source = sourceOf(clean(data.referrer, 400), clean(data.source, 64), userAgent, (context.host || '').toLowerCase())
    count('visits')
    count(data.returning === true ? 'returning' : 'new')
    count(`h:${String(now.getUTCHours()).padStart(2, '0')}`)
    count(`c:${country}`)
    if (region) count(`r:${country}-${region}`)
    count(`s:${source}`)
    count(`p:${pagePath}`)
    count(`l:${isLanguage(language) ? language : pathLanguage}`)
    count(`d:${clean(data.device, 8) === 'phone' ? 'phone' : 'desktop'}`)
  } else if (kind === 'click') {
    const target = clean(data.target, 160)
    if (!target || !isOwnTarget(target, (context.host || '').toLowerCase())) return
    count(`k:${target}`)
  } else {
    return
  }
  commands.push(['EXPIRE', day, KEEP_DAYS * DAY_SECONDS])
  await pipeline(commands)
}

export type Counter = { name: string; count: number }
export type DayCount = { day: string; visits: number; fresh: number; returning: number }
export type CountryCount = { code: string; name: string; count: number; regions: Counter[] }

export type Summary = {
  days: number
  visits: number
  fresh: number
  returning: number
  today: number
  allTime: number
  perDay: DayCount[]
  hours: number[]
  countries: CountryCount[]
  sources: Counter[]
  pages: Counter[]
  languages: Counter[]
  devices: Counter[]
  clicks: Counter[]
}

function toCounters(map: Map<string, number>, limit: number): Counter[] {
  return [...map.entries()]
    .map(([name, count]) => ({ name, count }))
    .sort((a, b) => b.count - a.count || a.name.localeCompare(b.name))
    .slice(0, limit)
}

function regionName(code: string): string {
  try {
    return new Intl.DisplayNames(['en'], { type: 'region' }).of(code) || code
  } catch {
    return code
  }
}

function hashOf(result: unknown): Record<string, number> {
  const hash: Record<string, number> = {}
  if (!Array.isArray(result)) return hash
  for (let index = 0; index + 1 < result.length; index += 2) {
    const field = String(result[index])
    const value = Number(result[index + 1])
    if (Number.isFinite(value)) hash[field] = value
  }
  return hash
}

export async function readStats(days = 30): Promise<Summary> {
  const today = new Date()
  const dayKeys: string[] = []
  for (let back = days - 1; back >= 0; back--) {
    const date = new Date(today.getTime() - back * DAY_SECONDS * 1000)
    dayKeys.push(dayOf(date))
  }
  const results = await pipeline([...dayKeys.map((day) => ['HGETALL', `${DAY_PREFIX}${day}`]), ['HGETALL', TOTAL_KEY]])
  const total = hashOf(results[results.length - 1])

  const perDay: DayCount[] = []
  const hours = new Array<number>(24).fill(0)
  const countries = new Map<string, { count: number; regions: Map<string, number> }>()
  const sources = new Map<string, number>()
  const pages = new Map<string, number>()
  const languages = new Map<string, number>()
  const devices = new Map<string, number>()
  const clicks = new Map<string, number>()
  let visits = 0
  let fresh = 0
  let returning = 0
  const add = (map: Map<string, number>, key: string, value: number) => map.set(key, (map.get(key) || 0) + value)

  dayKeys.forEach((day, index) => {
    const hash = hashOf(results[index])
    perDay.push({ day, visits: hash.visits || 0, fresh: hash.new || 0, returning: hash.returning || 0 })
    visits += hash.visits || 0
    fresh += hash.new || 0
    returning += hash.returning || 0
    for (const [field, value] of Object.entries(hash)) {
      const separator = field.indexOf(':')
      if (separator < 0) continue
      const prefix = field.slice(0, separator)
      const name = field.slice(separator + 1)
      switch (prefix) {
        case 'h': {
          const hour = Number(name)
          if (hour >= 0 && hour < 24) hours[hour] += value
          break
        }
        case 'c': {
          const entry = countries.get(name) || { count: 0, regions: new Map() }
          entry.count += value
          countries.set(name, entry)
          break
        }
        case 'r': {
          const [code, ...rest] = name.split('-')
          const entry = countries.get(code) || { count: 0, regions: new Map() }
          add(entry.regions, rest.join('-'), value)
          countries.set(code, entry)
          break
        }
        case 's':
          add(sources, name, value)
          break
        case 'p':
          add(pages, name, value)
          break
        case 'l':
          add(languages, name, value)
          break
        case 'd':
          add(devices, name, value)
          break
        case 'k':
          add(clicks, name, value)
          break
      }
    }
  })

  return {
    days,
    visits,
    fresh,
    returning,
    today: perDay[perDay.length - 1]?.visits ?? 0,
    allTime: total.visits || 0,
    perDay,
    hours,
    countries: [...countries.entries()]
      .map(([code, entry]) => ({
        code,
        name: code === 'ZZ' ? 'unknown' : regionName(code),
        count: entry.count,
        regions: toCounters(entry.regions, 6),
      }))
      .sort((a, b) => b.count - a.count)
      .slice(0, 40),
    sources: toCounters(sources, 25),
    pages: toCounters(pages, 25),
    languages: toCounters(languages, 16),
    devices: toCounters(devices, 2),
    clicks: toCounters(clicks, 25),
  }
}
