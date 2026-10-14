import { useEffect, useRef, useState } from 'react'
import {
  Type,
  Square,
  Trash2,
  Loader2,
  Save,
  ChevronLeft,
  ChevronRight,
  CheckCircle2,
  AlertTriangle,
  FolderOpen,
  MousePointer2,
  Move,
} from 'lucide-react'
import { AnimatePresence, motion } from 'framer-motion'
import type { FileInfo } from '../../electron/preload'
import { renderPageAtWidth, type PageRender } from '../lib/pdfRender'
import { stampOverlays, type Overlay } from '../lib/pdfOps'
import { fontsSupported, listFamilies, fontBytes } from '../lib/systemFonts'

type Tool = 'select' | 'text' | 'cover'
type Box = {
  id: string
  page: number
  type: 'text' | 'cover'
  x: number
  y: number
  w: number
  h: number
  sizePx: number
  scale: number
  text: string
  color: string
  font: string // system family name; '' → Helvetica fallback
}
type Done = { ok: boolean; message: string; outPath?: string }

let seq = 0
const uid = () => `ov-${Date.now()}-${seq++}`
const TARGET_W = 660

export function PdfTextEditor({ file }: { file: FileInfo }) {
  const [render, setRender] = useState<PageRender | null>(null)
  const [pageIndex, setPageIndex] = useState(0)
  const [loading, setLoading] = useState(false)
  const [tool, setTool] = useState<Tool>('select')
  const [boxes, setBoxes] = useState<Box[]>([])
  const [selected, setSelected] = useState<string | null>(null)
  const [busy, setBusy] = useState(false)
  const [done, setDone] = useState<Done | null>(null)
  const [families, setFamilies] = useState<string[]>([])
  const bytes = useRef<Uint8Array | null>(null)
  const surfaceRef = useRef<HTMLDivElement>(null)

  // lazily query installed fonts on first interaction (needs a user gesture)
  const ensureFonts = () => {
    if (families.length || !fontsSupported()) return
    listFamilies().then(setFamilies)
  }

  // load bytes when the file changes; `ready` flips so the render effect fires
  const [ready, setReady] = useState(false)
  useEffect(() => {
    let alive = true
    setReady(false)
    setPageIndex(0)
    ;(async () => {
      const data = await window.pdf.readBytes(file.path)
      if (!alive) return
      bytes.current = data
      setReady(true)
    })()
    return () => {
      alive = false
    }
  }, [file.path])

  // render current page once bytes are ready
  useEffect(() => {
    if (!ready || !bytes.current) return
    let alive = true
    setLoading(true)
    ;(async () => {
      try {
        const r = await renderPageAtWidth(bytes.current!, pageIndex + 1, TARGET_W)
        if (alive) setRender(r)
      } finally {
        if (alive) setLoading(false)
      }
    })()
    return () => {
      alive = false
    }
  }, [pageIndex, ready])

  const pageCount = render?.pageCount ?? file.pages ?? 1
  const pageBoxes = boxes.filter((b) => b.page === pageIndex)

  function addBox(e: React.PointerEvent) {
    if (tool === 'select' || !render) return
    const rect = surfaceRef.current!.getBoundingClientRect()
    const x = e.clientX - rect.left
    const y = e.clientY - rect.top
    const base = {
      id: uid(),
      page: pageIndex,
      x,
      y,
      scale: render.scale,
    }
    const box: Box =
      tool === 'text'
        ? { ...base, type: 'text', w: 160, h: 28, sizePx: 16, text: 'Votre texte', color: '#111111', font: '' }
        : { ...base, type: 'cover', w: 150, h: 24, sizePx: 0, text: '', color: '#ffffff', font: '' }
    setBoxes((bs) => [...bs, box])
    setSelected(box.id)
    setTool('select')
  }

  const update = (id: string, patch: Partial<Box>) =>
    setBoxes((bs) => bs.map((b) => (b.id === id ? { ...b, ...patch } : b)))
  const removeBox = (id: string) => {
    setBoxes((bs) => bs.filter((b) => b.id !== id))
    setSelected(null)
  }

  // drag / resize with pointer capture
  function startDrag(e: React.PointerEvent, id: string, mode: 'move' | 'resize') {
    e.stopPropagation()
    const box = boxes.find((b) => b.id === id)
    if (!box) return
    setSelected(id)
    const startX = e.clientX
    const startY = e.clientY
    const orig = { x: box.x, y: box.y, w: box.w, h: box.h }
    const target = e.currentTarget as HTMLElement
    target.setPointerCapture(e.pointerId)
    const onMove = (ev: PointerEvent) => {
      const dx = ev.clientX - startX
      const dy = ev.clientY - startY
      if (mode === 'move') update(id, { x: orig.x + dx, y: orig.y + dy })
      else update(id, { w: Math.max(24, orig.w + dx), h: Math.max(14, orig.h + dy) })
    }
    const onUp = (ev: PointerEvent) => {
      target.releasePointerCapture(ev.pointerId)
      target.removeEventListener('pointermove', onMove)
      target.removeEventListener('pointerup', onUp)
    }
    target.addEventListener('pointermove', onMove)
    target.addEventListener('pointerup', onUp)
  }

  async function save() {
    if (!bytes.current || boxes.length === 0) return
    const out = await window.pdf.saveFile(
      `${file.name.replace(/\.[^.]+$/, '')}-texte.pdf`,
      [{ name: 'PDF', extensions: ['pdf'] }],
    )
    if (!out) return
    setBusy(true)
    setDone(null)
    try {
      const overlays: Overlay[] = boxes.map((b) =>
        b.type === 'cover'
          ? { type: 'cover', page: b.page, x: b.x, y: b.y, w: b.w, h: b.h, scale: b.scale, color: b.color }
          : { type: 'text', page: b.page, x: b.x, y: b.y, sizePx: b.sizePx, scale: b.scale, text: b.text, color: b.color, font: b.font },
      )
      // fetch bytes for each installed font actually used
      const usedFamilies = [...new Set(boxes.filter((b) => b.type === 'text' && b.font).map((b) => b.font))]
      const fonts: Record<string, Uint8Array> = {}
      for (const fam of usedFamilies) {
        const data = await fontBytes(fam)
        if (data) fonts[fam] = data
      }
      const edited = await stampOverlays(bytes.current, overlays, fonts)
      const r = await window.pdf.writeBytes(out, edited)
      setDone({
        ok: r.ok,
        message: r.ok ? `Enregistré · ${boxes.length} élément(s)` : r.error ?? 'Échec',
        outPath: out,
      })
    } catch (e) {
      setDone({ ok: false, message: String(e) })
    } finally {
      setBusy(false)
    }
  }

  const sel = boxes.find((b) => b.id === selected)

  return (
    <div className="flex flex-col gap-4">
      {/* toolbar */}
      <div className="glass reflect flex flex-wrap items-center gap-2 rounded-2xl p-3">
        <ToolBtn active={tool === 'select'} onClick={() => setTool('select')} icon={MousePointer2} label="Sélection" />
        <ToolBtn active={tool === 'text'} onClick={() => setTool('text')} icon={Type} label="Texte" />
        <ToolBtn active={tool === 'cover'} onClick={() => setTool('cover')} icon={Square} label="Masquer" />
        <span className="mx-1 h-5 w-px bg-[rgb(var(--line)/0.15)]" />
        {/* per-selection controls */}
        {sel ? (
          <div className="flex items-center gap-2">
            {sel.type === 'text' && (
              <>
                <select
                  value={sel.font}
                  onMouseDown={ensureFonts}
                  onFocus={ensureFonts}
                  onChange={(e) => update(sel.id, { font: e.target.value })}
                  className="max-w-[160px] rounded-lg border border-[rgb(var(--line)/0.12)] bg-[rgb(var(--line)/0.05)] px-2 py-1 text-sm outline-none"
                  title="Police (installée sur le PC)"
                  style={{ fontFamily: sel.font ? `"${sel.font}"` : undefined }}
                >
                  <option value="">Par défaut (Helvetica)</option>
                  {!fontsSupported() && <option disabled>Polices système indisponibles</option>}
                  {families.map((f) => (
                    <option key={f} value={f} style={{ fontFamily: `"${f}"` }}>
                      {f}
                    </option>
                  ))}
                </select>
                <input
                  type="number"
                  value={sel.sizePx}
                  min={6}
                  max={96}
                  onChange={(e) => update(sel.id, { sizePx: Number(e.target.value) })}
                  className="w-16 rounded-lg border border-[rgb(var(--line)/0.12)] bg-[rgb(var(--line)/0.05)] px-2 py-1 text-sm outline-none"
                  title="Taille (px)"
                />
              </>
            )}
            <input
              type="color"
              value={sel.color}
              onChange={(e) => update(sel.id, { color: e.target.value })}
              className="h-7 w-8 cursor-pointer rounded-md border border-[rgb(var(--line)/0.12)] bg-transparent"
              title="Couleur"
            />
            <button
              onClick={() => removeBox(sel.id)}
              className="grid h-7 w-7 place-items-center rounded-lg text-muted transition-colors hover:bg-red-500 hover:text-white"
              title="Supprimer"
            >
              <Trash2 size={14} />
            </button>
          </div>
        ) : (
          <span className="text-xs text-muted">
            {tool === 'select' ? 'Choisissez un outil, puis cliquez sur la page' : 'Cliquez sur la page pour placer'}
          </span>
        )}
      </div>

      {/* page surface */}
      <div className="flex justify-center">
        <div className="relative">
          {loading && (
            <div className="absolute inset-0 z-10 grid place-items-center rounded-lg bg-black/10">
              <Loader2 size={22} className="animate-spin text-muted" />
            </div>
          )}
          <div
            ref={surfaceRef}
            onPointerDown={(e) => {
              if (tool !== 'select') addBox(e)
              else if (e.target === e.currentTarget || (e.target as HTMLElement).tagName === 'IMG')
                setSelected(null)
            }}
            className="relative select-none rounded-lg border border-[rgb(var(--line)/0.15)] shadow-lg"
            style={{
              width: render?.width ?? TARGET_W,
              height: render?.height ?? TARGET_W * 1.4,
              cursor: tool === 'select' ? 'default' : 'crosshair',
            }}
          >
            {render && (
              <img
                src={render.url}
                alt={`Page ${pageIndex + 1}`}
                className="block h-full w-full rounded-lg"
                draggable={false}
              />
            )}

            {pageBoxes.map((b) => {
              const isSel = b.id === selected
              return (
                <div
                  key={b.id}
                  onPointerDown={(e) => b.type === 'cover' && startDrag(e, b.id, 'move')}
                  className="group absolute"
                  style={{ left: b.x, top: b.y, width: b.w, height: b.h }}
                >
                  {/* move handle — the reliable way to reposition (the textarea
                      captures the pointer for typing) */}
                  <div
                    onPointerDown={(e) => startDrag(e, b.id, 'move')}
                    title="Déplacer"
                    className={`absolute -top-6 left-0 z-10 flex h-5 cursor-grab items-center gap-1 rounded-md px-1.5 text-white active:cursor-grabbing ${
                      isSel ? 'opacity-100' : 'opacity-0 group-hover:opacity-100'
                    }`}
                    style={{ background: 'rgb(var(--accent))' }}
                  >
                    <Move size={11} />
                  </div>
                  {b.type === 'cover' ? (
                    <div
                      className="h-full w-full"
                      style={{
                        background: b.color,
                        outline: isSel ? '2px solid rgb(var(--accent))' : '1px dashed rgb(var(--line)/0.4)',
                      }}
                    />
                  ) : (
                    <textarea
                      value={b.text}
                      onChange={(e) => update(b.id, { text: e.target.value })}
                      onPointerDown={(e) => {
                        // let the caret work, but still allow dragging from edges
                        e.stopPropagation()
                        setSelected(b.id)
                      }}
                      spellCheck={false}
                      className="h-full w-full resize-none overflow-hidden bg-transparent leading-tight outline-none"
                      style={{
                        color: b.color,
                        fontSize: b.sizePx,
                        fontFamily: b.font ? `"${b.font}"` : 'Helvetica, Arial, sans-serif',
                        outline: isSel ? '2px solid rgb(var(--accent))' : '1px dashed rgb(var(--line)/0.4)',
                      }}
                    />
                  )}
                  {isSel && (
                    <div
                      onPointerDown={(e) => startDrag(e, b.id, 'resize')}
                      className="absolute -bottom-1 -right-1 h-3 w-3 cursor-se-resize rounded-sm bg-[rgb(var(--accent))]"
                    />
                  )}
                </div>
              )
            })}
          </div>
        </div>
      </div>

      {/* page nav */}
      <div className="flex items-center justify-center gap-3">
        <button
          onClick={() => setPageIndex((i) => Math.max(0, i - 1))}
          disabled={pageIndex === 0}
          className="btn-ghost no-drag !px-3 disabled:opacity-30"
        >
          <ChevronLeft size={16} />
        </button>
        <span className="text-sm text-muted">
          Page {pageIndex + 1} / {pageCount}
        </span>
        <button
          onClick={() => setPageIndex((i) => Math.min(pageCount - 1, i + 1))}
          disabled={pageIndex >= pageCount - 1}
          className="btn-ghost no-drag !px-3 disabled:opacity-30"
        >
          <ChevronRight size={16} />
        </button>
      </div>

      {/* result */}
      <AnimatePresence>
        {done && (
          <motion.div
            initial={{ opacity: 0, y: 10 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: -6 }}
            className="glass reflect flex items-center gap-3 rounded-2xl px-4 py-3.5"
          >
            {done.ok ? (
              <CheckCircle2 size={20} className="shrink-0 text-emerald-400" />
            ) : (
              <AlertTriangle size={20} className="shrink-0 text-amber-400" />
            )}
            <div className="min-w-0 flex-1">
              <p className="text-[13.5px] font-semibold">{done.ok ? 'Réussi' : 'Erreur'}</p>
              <p className="truncate text-xs text-muted">{done.message}</p>
            </div>
            {done.ok && done.outPath && (
              <button
                onClick={() => window.pdf.revealInFolder(done.outPath!)}
                className="btn-ghost no-drag shrink-0 !py-1.5 !text-xs"
              >
                <FolderOpen size={13} /> Ouvrir
              </button>
            )}
          </motion.div>
        )}
      </AnimatePresence>

      <div className="flex justify-end pb-4">
        <button onClick={save} disabled={boxes.length === 0 || busy} className="btn-primary no-drag">
          {busy ? (
            <>
              <Loader2 size={16} className="animate-spin" /> Enregistrement…
            </>
          ) : (
            <>
              <Save size={16} /> Enregistrer
            </>
          )}
        </button>
      </div>
    </div>
  )
}

function ToolBtn({
  active,
  onClick,
  icon: Icon,
  label,
}: {
  active: boolean
  onClick: () => void
  icon: typeof Type
  label: string
}) {
  return (
    <button
      onClick={onClick}
      className={`no-drag flex items-center gap-1.5 rounded-lg px-2.5 py-1.5 text-[13px] font-medium transition-colors ${
        active
          ? 'bg-[rgb(var(--accent)/0.15)] text-ink ring-1 ring-[rgb(var(--accent)/0.5)]'
          : 'text-muted hover:bg-[rgb(var(--line)/0.08)] hover:text-ink'
      }`}
    >
      <Icon size={15} /> {label}
    </button>
  )
}
