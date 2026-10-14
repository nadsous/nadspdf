import { PDFDocument, degrees as pdfDegrees, rgb, StandardFonts, type PDFFont } from 'pdf-lib'
import fontkit from '@pdf-lib/fontkit'
import { rangesToSet } from './ranges'

/**
 * Byte→byte PDF operations built on pdf-lib (pure JS, runs in the renderer).
 * Every op takes a Uint8Array of PDF bytes and resolves to a new Uint8Array,
 * which makes them trivially chainable inside the pipeline runner.
 */

const load = (bytes: Uint8Array) => PDFDocument.load(bytes, { ignoreEncryption: true })

/** Concatenate several PDFs, in order, into one document. */
export async function mergeBytes(inputs: Uint8Array[]): Promise<Uint8Array> {
  const merged = await PDFDocument.create()
  for (const bytes of inputs) {
    const src = await load(bytes)
    const pages = await merged.copyPages(src, src.getPageIndices())
    pages.forEach((p) => merged.addPage(p))
  }
  return merged.save()
}

/** Keep only the pages named by a range spec ("1-3,5" / "all"), drop the rest. */
export async function keepPagesBytes(bytes: Uint8Array, ranges: string): Promise<Uint8Array> {
  const src = await load(bytes)
  const count = src.getPageCount()
  const keep = [...rangesToSet(ranges, count)].sort((a, b) => a - b).map((n) => n - 1)
  if (keep.length === 0) throw new Error('Aucune page à conserver')
  const out = await PDFDocument.create()
  const copied = await out.copyPages(src, keep)
  copied.forEach((p) => out.addPage(p))
  return out.save()
}

/** Remove the pages named by a range spec; keep everything else. */
export async function removePagesBytes(bytes: Uint8Array, ranges: string): Promise<Uint8Array> {
  const src = await load(bytes)
  const count = src.getPageCount()
  const drop = rangesToSet(ranges, count)
  const keep: number[] = []
  for (let i = 1; i <= count; i++) if (!drop.has(i)) keep.push(i - 1)
  if (keep.length === 0) throw new Error('Toutes les pages seraient supprimées')
  const out = await PDFDocument.create()
  const copied = await out.copyPages(src, keep)
  copied.forEach((p) => out.addPage(p))
  return out.save()
}

/** Rotate pages (all, or those in `ranges`) by a relative angle. */
export async function rotateBytes(
  bytes: Uint8Array,
  deg: number,
  ranges = 'all',
): Promise<Uint8Array> {
  const doc = await load(bytes)
  const count = doc.getPageCount()
  const target = rangesToSet(ranges, count)
  doc.getPages().forEach((page, i) => {
    if (!target.has(i + 1)) return
    const cur = page.getRotation().angle
    page.setRotation(pdfDegrees((((cur + deg) % 360) + 360) % 360))
  })
  return doc.save()
}

/** Rebuild a document from an explicit ordering of source page indices,
 *  optionally applying a per-page absolute rotation. Used by the page editor. */
export async function reorderBytes(
  bytes: Uint8Array,
  order: { index: number; rotate?: number }[],
): Promise<Uint8Array> {
  const src = await load(bytes)
  if (order.length === 0) throw new Error('Aucune page restante')
  const out = await PDFDocument.create()
  const copied = await out.copyPages(
    src,
    order.map((o) => o.index),
  )
  copied.forEach((page, i) => {
    const r = order[i].rotate ?? 0
    if (r) page.setRotation(pdfDegrees(((r % 360) + 360) % 360))
    out.addPage(page)
  })
  return out.save()
}

const hexToRgb = (hex: string) => {
  const m = /^#?([\da-f]{2})([\da-f]{2})([\da-f]{2})$/i.exec(hex.trim())
  if (!m) return rgb(0, 0, 0)
  return rgb(parseInt(m[1], 16) / 255, parseInt(m[2], 16) / 255, parseInt(m[3], 16) / 255)
}

export type WatermarkOpts = {
  text: string
  fontSize?: number
  opacity?: number
  color?: string
  diagonal?: boolean
}

/** Stamp a diagonal (or horizontal) text watermark across every page. */
export async function watermarkBytes(bytes: Uint8Array, opts: WatermarkOpts): Promise<Uint8Array> {
  const doc = await load(bytes)
  const font = await doc.embedFont(StandardFonts.HelveticaBold)
  const size = opts.fontSize ?? 48
  const color = hexToRgb(opts.color ?? '#888888')
  const opacity = opts.opacity ?? 0.18
  const diagonal = opts.diagonal ?? true
  for (const page of doc.getPages()) {
    const { width, height } = page.getSize()
    const tw = font.widthOfTextAtSize(opts.text, size)
    page.drawText(opts.text, {
      x: width / 2 - (diagonal ? tw / 2.6 : tw / 2),
      y: height / 2 - size / 2,
      size,
      font,
      color,
      opacity,
      rotate: diagonal ? pdfDegrees(45) : undefined,
    })
  }
  return doc.save()
}

export type PageNumberOpts = {
  format?: string // supports {n} and {total}
  fontSize?: number
  position?: 'bottom-center' | 'bottom-right' | 'bottom-left'
  color?: string
}

/** Draw page numbers on every page. */
export async function pageNumbersBytes(
  bytes: Uint8Array,
  opts: PageNumberOpts = {},
): Promise<Uint8Array> {
  const doc = await load(bytes)
  const font = await doc.embedFont(StandardFonts.Helvetica)
  const size = opts.fontSize ?? 10
  const color = hexToRgb(opts.color ?? '#444444')
  const format = opts.format ?? '{n} / {total}'
  const position = opts.position ?? 'bottom-center'
  const pages = doc.getPages()
  const total = pages.length
  pages.forEach((page, i) => {
    const label = format.replace(/\{n\}/g, String(i + 1)).replace(/\{total\}/g, String(total))
    const { width } = page.getSize()
    const tw = font.widthOfTextAtSize(label, size)
    const margin = 24
    const x =
      position === 'bottom-left'
        ? margin
        : position === 'bottom-right'
          ? width - margin - tw
          : width / 2 - tw / 2
    page.drawText(label, { x, y: margin, size, font, color })
  })
  return doc.save()
}

/** Build a PDF from a list of images (one page per image, at native size). */
export async function imagesToPdfBytes(
  images: { bytes: Uint8Array; png: boolean }[],
): Promise<Uint8Array> {
  const doc = await PDFDocument.create()
  for (const im of images) {
    const img = im.png ? await doc.embedPng(im.bytes) : await doc.embedJpg(im.bytes)
    const page = doc.addPage([img.width, img.height])
    page.drawImage(img, { x: 0, y: 0, width: img.width, height: img.height })
  }
  return doc.save()
}

/**
 * A text/cover overlay expressed in the editor's *pixel* space, along with the
 * render `scale` that maps those pixels back to PDF points (1 pt = scale px).
 * Origin is top-left (screen space); we flip to PDF's bottom-left at bake time.
 */
export type Overlay =
  | {
      type: 'text'
      page: number
      x: number
      y: number
      sizePx: number
      scale: number
      text: string
      color: string
      /** system font family key; bytes provided separately in `fonts` */
      font?: string
    }
  | {
      type: 'cover'
      page: number
      x: number
      y: number
      w: number
      h: number
      scale: number
      color: string
    }

/**
 * Bake text boxes and white-out rectangles onto the PDF (patch-style editing).
 * `fonts` maps a family key → the raw TTF/OTF bytes (from the system) so we can
 * embed the user's installed fonts; anything missing falls back to Helvetica.
 */
export async function stampOverlays(
  bytes: Uint8Array,
  overlays: Overlay[],
  fonts: Record<string, Uint8Array> = {},
): Promise<Uint8Array> {
  const doc = await load(bytes)
  doc.registerFontkit(fontkit)
  const fallback = await doc.embedFont(StandardFonts.Helvetica)
  // embed each provided system font once
  const embedded: Record<string, PDFFont> = {}
  for (const [family, data] of Object.entries(fonts)) {
    try {
      embedded[family] = await doc.embedFont(data, { subset: true })
    } catch {
      /* unembeddable (e.g. TTC/bitmap) — will fall back */
    }
  }
  const pages = doc.getPages()
  for (const o of overlays) {
    const page = pages[o.page]
    if (!page) continue
    const H = page.getHeight()
    if (o.type === 'cover') {
      page.drawRectangle({
        x: o.x / o.scale,
        y: H - (o.y + o.h) / o.scale,
        width: o.w / o.scale,
        height: o.h / o.scale,
        color: hexToRgb(o.color),
      })
    } else {
      const font = (o.font && embedded[o.font]) || fallback
      const size = o.sizePx / o.scale
      // screen y is the box top; baseline sits ~0.8em below it
      const baseline = H - (o.y + o.sizePx * 0.8) / o.scale
      const lines = o.text.split('\n')
      lines.forEach((line, i) => {
        page.drawText(line, {
          x: o.x / o.scale,
          y: baseline - i * size * 1.2,
          size,
          font,
          color: hexToRgb(o.color),
        })
      })
    }
  }
  return doc.save()
}

/** Count pages without keeping the document around. */
export async function pageCountOf(bytes: Uint8Array): Promise<number> {
  const doc = await load(bytes)
  return doc.getPageCount()
}
