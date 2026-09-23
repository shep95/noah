import type { ProjectProfile } from './pattern-store'

export interface FileNode {
  name: string
  path: string
  type: 'file' | 'directory'
  content?: string
  children?: FileNode[]
  language?: string
}

export function detectLanguage(filename: string): string {
  const ext = filename.split('.').pop()?.toLowerCase() ?? ''
  const map: Record<string, string> = {
    ts: 'typescript', tsx: 'typescript', js: 'javascript', jsx: 'javascript',
    py: 'python', rs: 'rust', go: 'go', java: 'java', cpp: 'cpp', c: 'c',
    cs: 'csharp', rb: 'ruby', php: 'php', swift: 'swift', kt: 'kotlin',
    dart: 'dart', r: 'r', scala: 'scala', hs: 'haskell', ex: 'elixir',
    css: 'css', scss: 'scss', html: 'html', json: 'json', yaml: 'yaml',
    yml: 'yaml', toml: 'toml', md: 'markdown', sh: 'shell', bash: 'shell',
    sql: 'sql', graphql: 'graphql', proto: 'protobuf', tf: 'terraform',
    vue: 'vue', svelte: 'svelte'
  }
  return map[ext] ?? 'plaintext'
}

export function detectFramework(files: FileNode[]): string[] {
  const names = new Set(files.map(f => f.name.toLowerCase()))
  const content = files.map(f => f.content ?? '').join('\n')
  const frameworks: string[] = []

  if (names.has('next.config.js') || names.has('next.config.ts')) frameworks.push('next.js')
  if (names.has('vite.config.ts') || names.has('vite.config.js')) frameworks.push('vite')
  if (names.has('svelte.config.js') || names.has('svelte.config.ts')) frameworks.push('svelte')
  if (names.has('nuxt.config.ts') || names.has('nuxt.config.js')) frameworks.push('nuxt')
  if (names.has('remix.config.js') || names.has('remix.config.ts')) frameworks.push('remix')
  if (names.has('astro.config.mjs') || names.has('astro.config.ts')) frameworks.push('astro')
  if (names.has('angular.json')) frameworks.push('angular')
  if (names.has('cargo.toml')) frameworks.push('rust/cargo')
  if (names.has('pyproject.toml') || names.has('requirements.txt')) frameworks.push('python')
  if (names.has('go.mod')) frameworks.push('go modules')
  if (names.has('pom.xml') || names.has('build.gradle')) frameworks.push('java/jvm')
  if (content.includes('express(')) frameworks.push('express')
  if (content.includes('fastapi') || content.includes('FastAPI')) frameworks.push('fastapi')
  if (content.includes('django')) frameworks.push('django')
  if (content.includes('rails')) frameworks.push('rails')

  return [...new Set(frameworks)]
}

export function detectNamingConventions(content: string): Record<string, string> {
  const conventions: Record<string, string> = {}
  const camelCaseCount = (content.match(/\b[a-z][a-zA-Z]*[A-Z][a-zA-Z]*\b/g) ?? []).length
  const snakeCaseCount = (content.match(/\b[a-z][a-z_]*_[a-z][a-z_]*\b/g) ?? []).length
  const pascalCaseCount = (content.match(/\b[A-Z][a-zA-Z]*[A-Z][a-zA-Z]*\b/g) ?? []).length

  if (camelCaseCount > snakeCaseCount && camelCaseCount > pascalCaseCount) {
    conventions.variables = 'camelCase'
  } else if (snakeCaseCount > camelCaseCount) {
    conventions.variables = 'snake_case'
  }

  if (pascalCaseCount > 0) conventions.classes = 'PascalCase'

  const funcKeyword = (content.match(/function\s+\w+/g) ?? []).length
  const arrowFunc = (content.match(/const\s+\w+\s*=\s*\([^)]*\)\s*=>/g) ?? []).length
  conventions.functions = arrowFunc > funcKeyword ? 'arrow functions preferred' : 'function keyword preferred'

  return conventions
}

export function detectArchitecturePatterns(files: FileNode[]): string[] {
  const paths = files.map(f => f.path)
  const patterns: string[] = []

  if (paths.some(p => p.includes('/api/') || p.includes('/routes/'))) patterns.push('REST API layer')
  if (paths.some(p => p.includes('/components/'))) patterns.push('component architecture')
  if (paths.some(p => p.includes('/store/') || p.includes('/stores/'))) patterns.push('state management store')
  if (paths.some(p => p.includes('/hooks/'))) patterns.push('custom hooks pattern')
  if (paths.some(p => p.includes('/lib/') || p.includes('/utils/'))) patterns.push('utility layer')
  if (paths.some(p => p.includes('/middleware/'))) patterns.push('middleware layer')
  if (paths.some(p => p.includes('/models/') || p.includes('/schema/'))) patterns.push('data models layer')
  if (paths.some(p => p.includes('/services/'))) patterns.push('service layer')
  if (paths.some(p => p.includes('test') || p.includes('spec'))) patterns.push('test suite present')
  if (paths.some(p => p.includes('docker'))) patterns.push('containerized')
  if (paths.some(p => p.includes('.github/workflows'))) patterns.push('github actions CI/CD')

  return patterns
}

export function buildProjectProfile(
  name: string,
  files: FileNode[]
): Omit<ProjectProfile, 'id'> {
  const allContent = files
    .filter(f => f.type === 'file' && f.content)
    .map(f => f.content ?? '')
    .join('\n')

  const languages = [...new Set(
    files
      .filter(f => f.type === 'file')
      .map(f => detectLanguage(f.name))
      .filter(l => l !== 'plaintext')
  )]

  return {
    name,
    language: languages,
    framework: detectFramework(files),
    styleConventions: {},
    namingConventions: detectNamingConventions(allContent),
    architecturePatterns: detectArchitecturePatterns(files),
    detectedAt: Date.now(),
    lastUpdated: Date.now()
  }
}

export function buildProjectContextString(profile: Omit<ProjectProfile, 'id'>): string {
  const lines = [
    `project: ${profile.name}`,
    `languages: ${profile.language.join(', ')}`,
    profile.framework.length ? `frameworks: ${profile.framework.join(', ')}` : '',
    Object.keys(profile.namingConventions).length
      ? `naming: ${Object.entries(profile.namingConventions).map(([k, v]) => `${k}=${v}`).join(', ')}`
      : '',
    profile.architecturePatterns.length
      ? `architecture: ${profile.architecturePatterns.join(', ')}`
      : ''
  ]
  return lines.filter(Boolean).join('\n')
}
