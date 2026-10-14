import * as pdfjs from 'pdfjs-dist'
import workerUrl from 'pdfjs-dist/build/pdf.worker.min.mjs?url'

pdfjs.GlobalWorkerOptions.workerSrc = workerUrl

export type RenderProgress = (done: number, total: number) => void

export type PageRender = {
  url: string
  width: number // rendered px
  height: number
  scale: number // px per PDF point
  pageCount: number
}

/** Render a single page fitted to `targetWidth` px; returns the px↔pt scale. */
export async function renderPageAtWidth(
  data: Uint8Array,
  pageNum: number,
  targetWidth = 640,
): Promise<PageRender> {
  // clone: pdf.js transfers (and detaches) the ArrayBuffer we hand it, which
  // would corrupt the caller's bytes on any subsequent use (e.g. next page).
  const doc = await pdfjs.getDocument({ data: data.slice() }).promise
  const page = await doc.getPage(pageNum)
  const base = page.getViewport({ scale: 1 })
  const scale = targetWidth / base.width
  const viewport = page.getViewport({ scale })
  const canvas = document.createElement('canvas')
  canvas.width = Math.ceil(viewport.width)
  canvas.height = Math.ceil(viewport.height)
  const ctx = canvas.getContext('2d')!
  ctx.fillStyle = '#ffffff'
  ctx.fillRect(0, 0, canvas.width, canvas.height)
  await page.render({ canvasContext: ctx, viewport }).promise
  return {
    url: canvas.toDataURL('image/jpeg', 0.85),
    width: canvas.width,
    height: canvas.height,
    scale,
    pageCount: doc.numPages,
  }
}

/** Render every page of a PDF to PNG bytes at the given scale. */
export async function pdfToPngs(
  data: Uint8Array,
  scale = 2,
  onProgress?: RenderProgress,
): Promise<Uint8Array[]> {
  const doc = await pdfjs.getDocument({ data: data.slice() }).promise
  const out: Uint8Array[] = []
  for (let i = 1; i <= doc.numPages; i++) {
    const page = await doc.getPage(i)
    const viewport = page.getViewport({ scale })
    const canvas = document.createElement('canvas')
    canvas.width = Math.ceil(viewport.width)
    canvas.height = Math.ceil(viewport.height)
    const ctx = canvas.getContext('2d')!
    await page.render({ canvasContext: ctx, viewport }).promise
    const blob: Blob = await new Promise((res) =>
      canvas.toBlob((b) => res(b!), 'image/png'),
    )
    out.push(new Uint8Array(await blob.arrayBuffer()))
    onProgress?.(i, doc.numPages)
  }
  return out
}
