export interface CodePattern {
  id: string
  timestamp: number
  projectId: string
  type: 'error_correction' | 'style_preference' | 'architecture' | 'naming' | 'security'
  trigger: string
  correction: string
  language: string
  confidence: number
  occurrences: number
}

export interface ProjectProfile {
  id: string
  name: string
  language: string[]
  framework: string[]
  styleConventions: Record<string, string>
  namingConventions: Record<string, string>
  architecturePatterns: string[]
  detectedAt: number
  lastUpdated: number
}

const DB_NAME = 'shepherd_patterns'
const DB_VERSION = 1
const PATTERN_STORE = 'patterns'
const PROJECT_STORE = 'projects'

function openDB(): Promise<IDBDatabase> {
  return new Promise((resolve, reject) => {
    if (typeof indexedDB === 'undefined') {
      reject(new Error('indexeddb not available'))
      return
    }
    const req = indexedDB.open(DB_NAME, DB_VERSION)
    req.onerror = () => reject(req.error)
    req.onsuccess = () => resolve(req.result)
    req.onupgradeneeded = (e) => {
      const db = (e.target as IDBOpenDBRequest).result
      if (!db.objectStoreNames.contains(PATTERN_STORE)) {
        const store = db.createObjectStore(PATTERN_STORE, { keyPath: 'id' })
        store.createIndex('projectId', 'projectId')
        store.createIndex('type', 'type')
        store.createIndex('language', 'language')
      }
      if (!db.objectStoreNames.contains(PROJECT_STORE)) {
        db.createObjectStore(PROJECT_STORE, { keyPath: 'id' })
      }
    }
  })
}

export async function storePattern(pattern: Omit<CodePattern, 'id' | 'timestamp' | 'occurrences'>): Promise<CodePattern> {
  const db = await openDB()
  const full: CodePattern = {
    ...pattern,
    id: `pat_${Date.now()}_${Math.random().toString(36).slice(2)}`,
    timestamp: Date.now(),
    occurrences: 1
  }
  return new Promise((resolve, reject) => {
    const tx = db.transaction(PATTERN_STORE, 'readwrite')
    const req = tx.objectStore(PATTERN_STORE).add(full)
    req.onsuccess = () => resolve(full)
    req.onerror = () => reject(req.error)
  })
}

export async function getPatternsForProject(projectId: string): Promise<CodePattern[]> {
  const db = await openDB()
  return new Promise((resolve, reject) => {
    const tx = db.transaction(PATTERN_STORE, 'readonly')
    const index = tx.objectStore(PATTERN_STORE).index('projectId')
    const req = index.getAll(projectId)
    req.onsuccess = () => resolve(req.result as CodePattern[])
    req.onerror = () => reject(req.error)
  })
}

export async function getGlobalPatterns(): Promise<CodePattern[]> {
  const db = await openDB()
  return new Promise((resolve, reject) => {
    const tx = db.transaction(PATTERN_STORE, 'readonly')
    const req = tx.objectStore(PATTERN_STORE).getAll()
    req.onsuccess = () => resolve(req.result as CodePattern[])
    req.onerror = () => reject(req.error)
  })
}

export async function incrementPattern(id: string): Promise<void> {
  const db = await openDB()
  return new Promise((resolve, reject) => {
    const tx = db.transaction(PATTERN_STORE, 'readwrite')
    const store = tx.objectStore(PATTERN_STORE)
    const getReq = store.get(id)
    getReq.onsuccess = () => {
      const p = getReq.result as CodePattern
      if (!p) { resolve(); return }
      p.occurrences += 1
      p.confidence = Math.min(1, p.confidence + 0.05)
      store.put(p)
      resolve()
    }
    getReq.onerror = () => reject(getReq.error)
  })
}

export async function saveProjectProfile(profile: ProjectProfile): Promise<void> {
  const db = await openDB()
  return new Promise((resolve, reject) => {
    const tx = db.transaction(PROJECT_STORE, 'readwrite')
    const req = tx.objectStore(PROJECT_STORE).put(profile)
    req.onsuccess = () => resolve()
    req.onerror = () => reject(req.error)
  })
}

export async function getProjectProfile(id: string): Promise<ProjectProfile | null> {
  const db = await openDB()
  return new Promise((resolve, reject) => {
    const tx = db.transaction(PROJECT_STORE, 'readonly')
    const req = tx.objectStore(PROJECT_STORE).get(id)
    req.onsuccess = () => resolve((req.result as ProjectProfile) ?? null)
    req.onerror = () => reject(req.error)
  })
}

// Builds context string from learned patterns to inject into shepherd prompt
export async function buildPatternContext(projectId: string): Promise<string> {
  const [projectPatterns, globalPatterns, profile] = await Promise.all([
    getPatternsForProject(projectId).catch(() => [] as CodePattern[]),
    getGlobalPatterns().catch(() => [] as CodePattern[]),
    getProjectProfile(projectId).catch(() => null)
  ])

  const lines: string[] = []

  if (profile) {
    lines.push(`project: ${profile.name}`)
    lines.push(`languages: ${profile.language.join(', ')}`)
    if (profile.framework.length) lines.push(`frameworks: ${profile.framework.join(', ')}`)
    if (Object.keys(profile.namingConventions).length) {
      lines.push(`naming conventions: ${JSON.stringify(profile.namingConventions)}`)
    }
    if (profile.architecturePatterns.length) {
      lines.push(`architecture patterns detected: ${profile.architecturePatterns.join(', ')}`)
    }
  }

  const topPatterns = [...projectPatterns, ...globalPatterns]
    .sort((a, b) => b.occurrences - a.occurrences)
    .slice(0, 20)

  if (topPatterns.length) {
    lines.push('\nlearned corrections:')
    for (const p of topPatterns) {
      lines.push(`- [${p.type}] trigger: "${p.trigger}" → correction: "${p.correction}" (confidence: ${(p.confidence * 100).toFixed(0)}%, seen ${p.occurrences}x)`)
    }
  }

  return lines.join('\n')
}
