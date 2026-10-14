/**
 * Access the user's installed fonts via the Chromium Local Font Access API
 * (available in Electron). Gives us both the family name — for an accurate CSS
 * preview in the editor — and the raw font bytes, so we can embed the exact
 * TTF/OTF into the PDF with pdf-lib.
 */

export type LocalFont = {
  postscriptName: string
  fullName: string
  family: string
  style: string
  blob: () => Promise<Blob>
}

declare global {
  interface Window {
    queryLocalFonts?: () => Promise<LocalFont[]>
  }
}

let cache: LocalFont[] | null = null

export const fontsSupported = () => typeof window.queryLocalFonts === 'function'

/** Query (once) all installed font faces. Requires a user gesture the first time. */
export async function listFonts(): Promise<LocalFont[]> {
  if (!window.queryLocalFonts) return []
  if (cache) return cache
  try {
    cache = await window.queryLocalFonts()
  } catch {
    cache = []
  }
  return cache
}

/** Unique family names, alphabetically. */
export async function listFamilies(): Promise<string[]> {
  const fonts = await listFonts()
  return [...new Set(fonts.map((f) => f.family))].sort((a, b) => a.localeCompare(b))
}

/** Raw bytes of a family's regular face (falls back to any face of the family). */
export async function fontBytes(family: string): Promise<Uint8Array | null> {
  const fonts = await listFonts()
  const faces = fonts.filter((f) => f.family === family)
  if (faces.length === 0) return null
  const regular =
    faces.find((f) => /^(regular|normal|book|roman)$/i.test(f.style)) ??
    faces.find((f) => /regular|normal|book|roman/i.test(f.style)) ??
    faces[0]
  try {
    const blob = await regular.blob()
    return new Uint8Array(await blob.arrayBuffer())
  } catch {
    return null
  }
}
