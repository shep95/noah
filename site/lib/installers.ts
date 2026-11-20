import fs from 'node:fs'
import path from 'node:path'
import { INSTALLERS, type Installer } from './site'

// The release manifest (public/downloads/latest.json) names the files the
// release pipeline actually published, so an installer is offered only once
// it exists. lib/site.ts can list a platform ahead of its first build without
// showing a dead link. Server components only.
type Manifest = { assets?: Record<string, { url?: string }> }

function publishedFiles(): Set<string> {
  try {
    const manifestPath = path.join(process.cwd(), 'public', 'downloads', 'latest.json')
    const manifest = JSON.parse(fs.readFileSync(manifestPath, 'utf8')) as Manifest
    const files = new Set<string>()
    for (const asset of Object.values(manifest.assets ?? {})) {
      const file = asset.url?.split('/').pop()
      if (file) files.add(file)
    }
    return files
  } catch {
    return new Set()
  }
}

/** The installers people can download today, in the order lib/site.ts lists them. */
export function availableInstallers(): Installer[] {
  const published = publishedFiles()
  // The .deb is not in the manifest (the updater installs from the archive);
  // it ships with the archive, so it counts as published when that is.
  const linuxPublished = published.has('noah-linux-x86_64.tar.xz')
  return INSTALLERS.filter((installer) => {
    if (installer.file.endsWith('.deb')) return linuxPublished
    return published.has(installer.file)
  })
}
