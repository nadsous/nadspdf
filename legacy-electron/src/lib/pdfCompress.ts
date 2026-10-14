import * as pdfjs from 'pdfjs-dist'
import workerUrl from 'pdfjs-dist/build/pdf.worker.min.mjs?url'
import { PDFDocument } from 'pdf-lib'

pdfjs.GlobalWorkerOptions.workerSrc = workerUrl

export type CompressLevel = 'screen' | 'ebook' | 'printer'

const PROFILE: Record<CompressLevel, { scale: number; quality: number }> = {
  printer: { scale: 2.0, quality: 0.92 }, // Léger — meilleure qualité
  ebook: { scale: 1.5, quality: 0.78 }, // Moyen — équilibré
  screen: { scale: 1.0, quality: 0.55 }, // Fort — plus léger
}

/**
 * Compress a PDF by rasterizing each page and re-encoding as JPEG, then
 * rebuilding the document. Self-contained (no Ghostscript). Best for
 * scans/image-heavy PDFs; vector text becomes a raster image.
 */
export async function compressPdf(
  data: Uint8Array,
  level: CompressLevel,
  onProgress?: (done: number, total: number) => void,
): Promise<Uint8Array> {
  const { scale, quality } = PROFILE[level]
  // clone: pdf.js detaches the ArrayBuffer it receives; keep the caller's intact
  const src = await pdfjs.getDocument({ data: data.slice() }).promise
  const out = await PDFDocument.create()

  for (let i = 1; i <= src.numPages; i++) {
    const page = await src.getPage(i)
    const viewport = page.getViewport({ scale })
    const canvas = document.createElement('canvas')
    canvas.width = Math.ceil(viewport.width)
    canvas.height = Math.ceil(viewport.height)
    const ctx = canvas.getContext('2d')!
    ctx.fillStyle = '#ffffff'
    ctx.fillRect(0, 0, canvas.width, canvas.height)
    await page.render({ canvasContext: ctx, viewport }).promise

    const blob: Blob = await new Promise((res) =>
      canvas.toBlob((b) => res(b!), 'image/jpeg', quality),
    )
    const jpg = await out.embedJpg(new Uint8Array(await blob.arrayBuffer()))
    // 96 dpi reference: page size in PDF points = pixels / scale * (72/96)... keep
    // simple: map back to the page's natural point size.
    const base = page.getViewport({ scale: 1 })
    const pdfPage = out.addPage([base.width, base.height])
    pdfPage.drawImage(jpg, { x: 0, y: 0, width: base.width, height: base.height })
    onProgress?.(i, src.numPages)
  }

  return out.save()
}
