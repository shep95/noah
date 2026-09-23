// Strip all metadata from uploaded files before any in-app processing.
// Images: canvas re-draw removes EXIF (GPS, camera model, timestamps, etc.)
// Text/code: returned as-is (no metadata in plain text)
// PDFs/Office docs: returned as-is (binary stripping requires server-side tooling)

const IMAGE_TYPES = new Set([
  'image/jpeg', 'image/jpg', 'image/png', 'image/webp',
  'image/gif', 'image/bmp', 'image/tiff', 'image/heic', 'image/heif',
])

const TEXT_TYPES = new Set([
  'text/plain', 'text/html', 'text/css', 'text/javascript',
  'text/typescript', 'text/x-python', 'text/x-rust', 'text/x-go',
  'application/json', 'application/javascript', 'application/typescript',
  'application/xml', 'text/xml', 'text/markdown',
])

// Strip EXIF and all metadata from an image by re-drawing through canvas.
// Returns a new Blob (same MIME type) with no embedded metadata.
async function stripImageMetadata(file: File): Promise<Blob> {
  return new Promise((resolve, reject) => {
    const img = new Image()
    const objectUrl = URL.createObjectURL(file)

    img.onload = () => {
      try {
        const canvas = document.createElement('canvas')
        canvas.width = img.naturalWidth
        canvas.height = img.naturalHeight

        const ctx = canvas.getContext('2d')
        if (!ctx) {
          URL.revokeObjectURL(objectUrl)
          resolve(file) // fallback: return original
          return
        }

        // Draw onto blank canvas — drops all EXIF, ICC profiles, XMP, IPTC
        ctx.drawImage(img, 0, 0)
        URL.revokeObjectURL(objectUrl)

        const outType = file.type === 'image/png' ? 'image/png' : 'image/jpeg'
        canvas.toBlob(
          (blob) => {
            if (blob) resolve(blob)
            else resolve(file)
          },
          outType,
          0.93
        )
      } catch (err) {
        URL.revokeObjectURL(objectUrl)
        reject(err)
      }
    }

    img.onerror = () => {
      URL.revokeObjectURL(objectUrl)
      resolve(file) // fallback: return original on decode failure
    }

    img.src = objectUrl
  })
}

// Strip metadata from any file type.
// Returns a clean File with empty `lastModified` (epoch 0) to avoid leaking timestamps.
export async function stripFileMetadata(file: File): Promise<File> {
  try {
    // Images: canvas strip
    if (IMAGE_TYPES.has(file.type) || /\.(jpe?g|png|webp|gif|bmp|tiff?|heic)$/i.test(file.name)) {
      const cleanBlob = await stripImageMetadata(file)
      // Return as File with name preserved but lastModified zeroed
      return new File([cleanBlob], file.name, {
        type: cleanBlob.type,
        lastModified: 0,
      })
    }

    // Text/code: safe to pass through; no binary metadata
    if (
      TEXT_TYPES.has(file.type) ||
      file.type.startsWith('text/') ||
      /\.(ts|tsx|js|jsx|py|rs|go|java|cpp|c|h|cs|rb|php|swift|kt|vue|svelte|css|scss|less|html|json|yaml|yml|toml|md|sh|bash|txt|env|gitignore)$/i.test(file.name)
    ) {
      // Re-wrap without lastModified to avoid leaking filesystem timestamps
      const text = await file.text()
      return new File([text], file.name, { type: file.type, lastModified: 0 })
    }

    // All other files: zero the timestamp, return as-is
    const buf = await file.arrayBuffer()
    return new File([buf], file.name, { type: file.type, lastModified: 0 })
  } catch {
    // Never block the upload — return original as fallback
    return file
  }
}

// Read file content as text after stripping metadata.
export async function readCleanText(file: File): Promise<string> {
  const clean = await stripFileMetadata(file)
  return clean.text()
}

// Read file as a data URL (for wallpaper import) after stripping image metadata.
export async function readCleanDataUrl(file: File): Promise<string> {
  const clean = await stripFileMetadata(file)
  return new Promise((resolve, reject) => {
    const reader = new FileReader()
    reader.onload = () => resolve(reader.result as string)
    reader.onerror = reject
    reader.readAsDataURL(clean)
  })
}
