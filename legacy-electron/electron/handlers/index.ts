import type { IpcMain } from 'electron'
import { shell, BrowserWindow } from 'electron'
import { PDFDocument, degrees as pdfDegrees } from 'pdf-lib'
import fs from 'node:fs/promises'
import path from 'node:path'
import os from 'node:os'
import { randomUUID } from 'node:crypto'
import mammoth from 'mammoth'
import * as XLSX from 'xlsx'

type OpResult = {
  ok: boolean
  outPath?: string
  sizeBefore?: number
  sizeAfter?: number
  error?: string
}

async function size(p: string): Promise<number> {
  try {
    return (await fs.stat(p)).size
  } catch {
    return 0
  }
}

/** Parse "1-3,5,8-10" against a page count -> zero-based indices. */
function parseRanges(spec: string, pageCount: number): number[] {
  const out: number[] = []
  const clean = spec.trim()
  if (!clean || clean.toLowerCase() === 'all') {
    for (let i = 0; i < pageCount; i++) out.push(i)
    return out
  }
  for (const part of clean.split(',')) {
    const token = part.trim()
    if (!token) continue
    if (token.includes('-')) {
      let [a, b] = token.split('-').map((x) => parseInt(x.trim(), 10))
      if (Number.isNaN(a)) a = 1
      if (Number.isNaN(b)) b = pageCount
      const lo = Math.max(1, Math.min(a, b))
      const hi = Math.min(pageCount, Math.max(a, b))
      for (let i = lo; i <= hi; i++) out.push(i - 1)
    } else {
      const n = parseInt(token, 10)
      if (!Number.isNaN(n) && n >= 1 && n <= pageCount) out.push(n - 1)
    }
  }
  return out
}

const escapeHtml = (s: string) =>
  s.replace(/&/g, '&amp;').replace(/</g, '&lt;').replace(/>/g, '&gt;')

/** Wrap body fragment in a clean, print-ready document. */
function wrapHtml(inner: string): string {
  return `<!doctype html><html><head><meta charset="utf-8"><style>
    *{box-sizing:border-box}
    body{font-family:-apple-system,Segoe UI,Roboto,Helvetica,Arial,sans-serif;
      color:#16161a;line-height:1.55;font-size:13px;margin:0;padding:8px}
    h1,h2,h3{line-height:1.25}
    table{border-collapse:collapse;width:100%;font-size:12px;margin:8px 0}
    td,th{border:1px solid #d6d6dc;padding:5px 8px;text-align:left}
    th{background:#f4f4f7;font-weight:600}
    img{max-width:100%}
    pre{white-space:pre-wrap;font-family:ui-monospace,SFMono-Regular,Menlo,monospace;font-size:12px}
  </style></head><body>${inner}</body></html>`
}

/** Render an HTML string to PDF bytes using Electron's built-in engine. */
async function htmlToPdfBytes(html: string): Promise<Buffer> {
  const tmp = path.join(os.tmpdir(), `nadspdf-${randomUUID()}.html`)
  await fs.writeFile(tmp, wrapHtml(html), 'utf-8')
  const win = new BrowserWindow({
    show: false,
    webPreferences: { offscreen: true, sandbox: false },
  })
  try {
    await win.loadFile(tmp)
    return await win.webContents.printToPDF({
      printBackground: true,
      margins: { top: 0.5, bottom: 0.5, left: 0.5, right: 0.5 },
      pageSize: 'A4',
    })
  } finally {
    win.destroy()
    fs.unlink(tmp).catch(() => {})
  }
}

/** Render an HTML string to a PDF file using Electron's built-in engine. */
async function htmlToPdf(html: string, outPath: string): Promise<void> {
  await fs.writeFile(outPath, await htmlToPdfBytes(html))
}

/** Produce an HTML fragment for a given source file. */
async function fileToHtml(p: string): Promise<string> {
  const ext = path.extname(p).toLowerCase()
  if (ext === '.docx') {
    const { value } = await mammoth.convertToHtml({ path: p })
    return value
  }
  if (['.xlsx', '.xls', '.ods', '.csv'].includes(ext)) {
    const wb = XLSX.readFile(p)
    return wb.SheetNames.map((name) => {
      const table = XLSX.utils.sheet_to_html(wb.Sheets[name])
      return `<h2>${escapeHtml(name)}</h2>${table}`
    }).join('<div style="page-break-after:always"></div>')
  }
  if (ext === '.html' || ext === '.htm') {
    return await fs.readFile(p, 'utf-8')
  }
  if (ext === '.rtf') {
    const raw = await fs.readFile(p, 'utf-8')
    const text = raw
      .replace(/\\par[d]?/g, '\n')
      .replace(/\{\\[^}]*\}/g, '')
      .replace(/\\[a-z]+-?\d* ?/g, '')
      .replace(/[{}]/g, '')
    return `<pre>${escapeHtml(text.trim())}</pre>`
  }
  // txt and everything else: treat as plain text
  const text = await fs.readFile(p, 'utf-8')
  return `<pre>${escapeHtml(text)}</pre>`
}

export function registerHandlers(ipcMain: IpcMain) {
  ipcMain.handle('fs:readBytes', async (_e, p: string) => new Uint8Array(await fs.readFile(p)))

  ipcMain.handle('fs:writeBytes', async (_e, p: string, bytes: Uint8Array): Promise<OpResult> => {
    try {
      await fs.writeFile(p, Buffer.from(bytes))
      return { ok: true, outPath: p, sizeAfter: await size(p) }
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })

  ipcMain.handle('shell:reveal', async (_e, p: string) => shell.showItemInFolder(p))

  ipcMain.handle('pdf:fileInfo', async (_e, paths: string[]) => {
    const infos = []
    for (const p of paths) {
      const stat = await fs.stat(p).catch(() => null)
      const ext = path.extname(p).toLowerCase().replace('.', '')
      let pages: number | undefined
      if (ext === 'pdf') {
        try {
          const doc = await PDFDocument.load(await fs.readFile(p), { ignoreEncryption: true })
          pages = doc.getPageCount()
        } catch {
          /* unreadable */
        }
      }
      infos.push({ path: p, name: path.basename(p), ext, size: stat?.size ?? 0, pages })
    }
    return infos
  })

  ipcMain.handle('pdf:merge', async (_e, paths: string[], out: string): Promise<OpResult> => {
    try {
      const merged = await PDFDocument.create()
      for (const p of paths) {
        const src = await PDFDocument.load(await fs.readFile(p), { ignoreEncryption: true })
        const pages = await merged.copyPages(src, src.getPageIndices())
        pages.forEach((pg) => merged.addPage(pg))
      }
      await fs.writeFile(out, await merged.save())
      return { ok: true, outPath: out, sizeAfter: await size(out) }
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })

  ipcMain.handle(
    'pdf:split',
    async (_e, p: string, ranges: string, outDir: string): Promise<OpResult> => {
      try {
        const src = await PDFDocument.load(await fs.readFile(p), { ignoreEncryption: true })
        const indices = parseRanges(ranges, src.getPageCount())
        if (indices.length === 0) return { ok: false, error: 'Aucune page sélectionnée' }
        const doc = await PDFDocument.create()
        const copied = await doc.copyPages(src, indices)
        copied.forEach((pg) => doc.addPage(pg))
        const base = path.basename(p, path.extname(p))
        const out = path.join(outDir, `${base}-extrait.pdf`)
        await fs.writeFile(out, await doc.save())
        return { ok: true, outPath: out, sizeAfter: await size(out) }
      } catch (e) {
        return { ok: false, error: String(e) }
      }
    },
  )

  ipcMain.handle('pdf:rotate', async (_e, p: string, deg: number, out: string): Promise<OpResult> => {
    try {
      const doc = await PDFDocument.load(await fs.readFile(p), { ignoreEncryption: true })
      doc.getPages().forEach((page) => {
        const cur = page.getRotation().angle
        page.setRotation(pdfDegrees((cur + deg) % 360))
      })
      await fs.writeFile(out, await doc.save())
      return { ok: true, outPath: out, sizeAfter: await size(out) }
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })

  ipcMain.handle('pdf:imagesToPdf', async (_e, paths: string[], out: string): Promise<OpResult> => {
    try {
      const doc = await PDFDocument.create()
      for (const p of paths) {
        const bytes = await fs.readFile(p)
        const ext = path.extname(p).toLowerCase()
        const img = ext === '.png' ? await doc.embedPng(bytes) : await doc.embedJpg(bytes)
        const page = doc.addPage([img.width, img.height])
        page.drawImage(img, { x: 0, y: 0, width: img.width, height: img.height })
      }
      await fs.writeFile(out, await doc.save())
      return { ok: true, outPath: out, sizeAfter: await size(out) }
    } catch (e) {
      return { ok: false, error: String(e) }
    }
  })

  ipcMain.handle('pdf:convertBytes', async (_e, paths: string[]): Promise<
    { ok: boolean; bytes?: Uint8Array; error?: string }[]
  > => {
    const results: { ok: boolean; bytes?: Uint8Array; error?: string }[] = []
    const supported = ['.docx', '.xlsx', '.xls', '.ods', '.csv', '.txt', '.html', '.htm', '.rtf']
    for (const p of paths) {
      const ext = path.extname(p).toLowerCase()
      if (ext === '.pdf') {
        results.push({ ok: true, bytes: new Uint8Array(await fs.readFile(p)) })
        continue
      }
      if (!supported.includes(ext)) {
        results.push({ ok: false, error: `Format ${ext} non pris en charge sans LibreOffice` })
        continue
      }
      try {
        const html = await fileToHtml(p)
        const pdf = await htmlToPdfBytes(html)
        results.push({ ok: true, bytes: new Uint8Array(pdf) })
      } catch (e) {
        results.push({ ok: false, error: String(e) })
      }
    }
    return results
  })

  ipcMain.handle('pdf:convert', async (_e, paths: string[], outDir: string): Promise<OpResult[]> => {
    const results: OpResult[] = []
    const supported = ['.docx', '.xlsx', '.xls', '.ods', '.csv', '.txt', '.html', '.htm', '.rtf']
    for (const p of paths) {
      const before = await size(p)
      const ext = path.extname(p).toLowerCase()
      if (!supported.includes(ext)) {
        results.push({
          ok: false,
          error: `Format ${ext} non pris en charge sans LibreOffice`,
          sizeBefore: before,
        })
        continue
      }
      try {
        const html = await fileToHtml(p)
        const out = path.join(outDir, `${path.basename(p, ext)}.pdf`)
        await htmlToPdf(html, out)
        results.push({ ok: true, outPath: out, sizeBefore: before, sizeAfter: await size(out) })
      } catch (e) {
        results.push({ ok: false, error: String(e), sizeBefore: before })
      }
    }
    return results
  })
}
