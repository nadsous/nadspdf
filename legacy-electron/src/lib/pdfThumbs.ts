import * as pdfjs from 'pdfjs-dist'
import workerUrl from 'pdfjs-dist/build/pdf.worker.min.mjs?url'

pdfjs.GlobalWorkerOptions.workerSrc = workerUrl

export type Thumb = { page: number; url: string; w: number; h: number }

/** Render low-res page thumbnails (data URLs) for previewing a PDF. */
export async function renderThumbs(
  data: Uint8Array,
  scale = 0.5,
  onThumb?: (t: Thumb) => void,
): Promise<Thumb[]> {
  // clone: pdf.js detaches the ArrayBuffer it receives; keep the caller's intact
  const doc = await pdfjs.getDocument({ data: data.slice() }).promise
  const thumbs: Thumb[] = []
  for (let i = 1; i <= doc.numPages; i++) {
    const page = await doc.getPage(i)
    const viewport = page.getViewport({ scale })
    const canvas = document.createElement('canvas')
    canvas.width = Math.ceil(viewport.width)
    canvas.height = Math.ceil(viewport.height)
    const ctx = canvas.getContext('2d')!
    ctx.fillStyle = '#ffffff'
    ctx.fillRect(0, 0, canvas.width, canvas.height)
    await page.render({ canvasContext: ctx, viewport }).promise
    const t: Thumb = {
      page: i,
      url: canvas.toDataURL('image/jpeg', 0.7),
      w: canvas.width,
      h: canvas.height,
    }
    thumbs.push(t)
    onThumb?.(t)
  }
  return thumbs
}
