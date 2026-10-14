import { useEffect, useRef, useState } from 'react'
import { AnimatePresence, motion, Reorder } from 'framer-motion'
import {
  RotateCw,
  Trash2,
  Undo2,
  Loader2,
  Save,
  CheckCircle2,
  AlertTriangle,
  FolderOpen,
  PencilRuler,
  ChevronLeft,
  ChevronRight,
} from 'lucide-react'
import { useStore } from '../store/useStore'
import { DropZone } from './DropZone'
import { PdfTextEditor } from './PdfTextEditor'
import { renderThumbs, type Thumb } from '../lib/pdfThumbs'
import { reorderBytes } from '../lib/pdfOps'

const ease = [0.22, 1, 0.36, 1] as const
const pdfFilter = [{ name: 'PDF', extensions: ['pdf'] }]

type Page = { key: string; index: number; rotate: number; url: string }
type Done = { ok: boolean; message: string; outPath?: string }

export function PdfEditor() {
  const files = useStore((s) => s.files)
  const file = files[0]

  const [pages, setPages] = useState<Page[]>([])
  const [loading, setLoading] = useState(false)
  const [busy, setBusy] = useState(false)
  const [done, setDone] = useState<Done | null>(null)
  const [tab, setTab] = useState<'pages' | 'text'>('pages')
  const bytes = useRef<Uint8Array | null>(null)
  const lastPath = useRef<string>('')

  useEffect(() => {
    if (!file || file.ext !== 'pdf') {
      setPages([])
      bytes.current = null
      lastPath.current = ''
      return
    }
    if (lastPath.current === file.path) return
    lastPath.current = file.path
    let alive = true
    setLoading(true)
    setPages([])
    setDone(null)
    ;(async () => {
      try {
        const data = await window.pdf.readBytes(file.path)
        bytes.current = data
        await renderThumbs(data, 0.7, (t: Thumb) => {
          if (alive)
            setPages((prev) => [
              ...prev,
              { key: `p-${t.page}`, index: t.page - 1, rotate: 0, url: t.url },
            ])
        })
      } finally {
        if (alive) setLoading(false)
      }
    })()
    return () => {
      alive = false
    }
  }, [file?.path, file?.ext])

  const rotate = (key: string) =>
    setPages((ps) => ps.map((p) => (p.key === key ? { ...p, rotate: (p.rotate + 90) % 360 } : p)))
  const remove = (key: string) => setPages((ps) => ps.filter((p) => p.key !== key))
  const move = (key: string, dir: -1 | 1) =>
    setPages((ps) => {
      const i = ps.findIndex((p) => p.key === key)
      const j = i + dir
      if (i < 0 || j < 0 || j >= ps.length) return ps
      const next = [...ps]
      ;[next[i], next[j]] = [next[j], next[i]]
      return next
    })

  const dirty = file && pages.length > 0

  async function save() {
    if (!bytes.current || pages.length === 0) return
    const out = await window.pdf.saveFile(
      `${file.name.replace(/\.[^.]+$/, '')}-modifié.pdf`,
      pdfFilter,
    )
    if (!out) return
    setBusy(true)
    setDone(null)
    try {
      const edited = await reorderBytes(
        bytes.current,
        pages.map((p) => ({ index: p.index, rotate: p.rotate })),
      )
      const r = await window.pdf.writeBytes(out, edited)
      setDone({
        ok: r.ok,
        message: r.ok ? `Enregistré · ${pages.length} page(s)` : r.error ?? 'Échec',
        outPath: out,
      })
    } catch (e) {
      setDone({ ok: false, message: String(e) })
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="mx-auto flex w-full max-w-3xl flex-col gap-5">
      {/* Header */}
      <div className="flex items-start gap-4">
        <div
          className="grid h-12 w-12 shrink-0 place-items-center rounded-2xl"
          style={{ background: 'linear-gradient(135deg, rgb(var(--g2)), rgb(var(--g4)))' }}
        >
          <PencilRuler size={22} className="text-white" />
        </div>
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Éditer</h1>
          <p className="mt-0.5 text-sm text-muted text-balance">
            Réorganisez par glisser-déposer, faites pivoter ou supprimez des pages, puis enregistrez.
          </p>
        </div>
      </div>

      {!file && <DropZone accept={['pdf']} multiple={false} grad={['var(--g2)', 'var(--g4)']} />}

      {file && (
        <div className="glass reflect flex gap-1 self-start rounded-2xl p-1">
          {(['pages', 'text'] as const).map((t) => (
            <button
              key={t}
              onClick={() => setTab(t)}
              className={`no-drag rounded-xl px-4 py-1.5 text-[13px] font-semibold transition-colors ${
                tab === t ? 'bg-[rgb(var(--line)/0.1)] text-ink' : 'text-muted hover:text-ink'
              }`}
            >
              {t === 'pages' ? 'Pages' : 'Texte'}
            </button>
          ))}
        </div>
      )}

      {file && tab === 'text' && <PdfTextEditor file={file} />}

      {file && tab === 'pages' && (
        <div className="glass reflect rounded-2xl p-4">
          <div className="mb-3 flex items-center justify-between">
            <p className="truncate text-[13px] font-semibold">{file.name}</p>
            <span className="text-xs text-muted">
              {loading ? (
                <span className="inline-flex items-center gap-1.5">
                  <Loader2 size={12} className="animate-spin" /> rendu…
                </span>
              ) : (
                `${pages.length} page${pages.length > 1 ? 's' : ''}`
              )}
            </span>
          </div>

          <Reorder.Group
            axis="y"
            values={pages}
            onReorder={setPages}
            className="grid grid-cols-[repeat(auto-fill,minmax(120px,1fr))] gap-3"
          >
            <AnimatePresence>
              {pages.map((p, i) => (
                <Reorder.Item
                  key={p.key}
                  value={p}
                  className="group relative cursor-grab active:cursor-grabbing"
                  initial={{ opacity: 0, scale: 0.9 }}
                  animate={{ opacity: 1, scale: 1 }}
                  exit={{ opacity: 0, scale: 0.85 }}
                  transition={{ ease }}
                  whileDrag={{ scale: 1.05, zIndex: 10 }}
                >
                  <div className="relative overflow-hidden rounded-lg border border-[rgb(var(--line)/0.12)] bg-white">
                    <img
                      src={p.url}
                      alt={`Page ${i + 1}`}
                      className="block h-full w-full object-contain transition-transform"
                      style={{ transform: `rotate(${p.rotate}deg)` }}
                    />
                    <span className="absolute bottom-1 right-1 rounded bg-black/55 px-1.5 py-0.5 text-[10px] font-medium text-white">
                      {i + 1}
                    </span>
                    {/* move controls */}
                    <div className="absolute inset-x-0 bottom-0 flex justify-start gap-1 p-1 opacity-0 transition-opacity group-hover:opacity-100">
                      <button
                        onPointerDownCapture={(e) => e.stopPropagation()}
                        onClick={() => move(p.key, -1)}
                        disabled={i === 0}
                        title="Déplacer à gauche"
                        className="no-drag grid h-6 w-6 place-items-center rounded-md bg-black/55 text-white transition-colors hover:bg-black/75 disabled:opacity-30"
                      >
                        <ChevronLeft size={13} />
                      </button>
                      <button
                        onPointerDownCapture={(e) => e.stopPropagation()}
                        onClick={() => move(p.key, 1)}
                        disabled={i === pages.length - 1}
                        title="Déplacer à droite"
                        className="no-drag grid h-6 w-6 place-items-center rounded-md bg-black/55 text-white transition-colors hover:bg-black/75 disabled:opacity-30"
                      >
                        <ChevronRight size={13} />
                      </button>
                    </div>
                    {/* controls — stopPropagation on pointerdown so the parent
                        Reorder drag doesn't swallow the click */}
                    <div className="absolute inset-x-0 top-0 flex justify-end gap-1 p-1 opacity-0 transition-opacity group-hover:opacity-100">
                      <button
                        onPointerDownCapture={(e) => e.stopPropagation()}
                        onClick={() => rotate(p.key)}
                        title="Pivoter 90°"
                        className="no-drag grid h-6 w-6 place-items-center rounded-md bg-black/55 text-white transition-colors hover:bg-black/75"
                      >
                        <RotateCw size={12} />
                      </button>
                      <button
                        onPointerDownCapture={(e) => e.stopPropagation()}
                        onClick={() => remove(p.key)}
                        title="Supprimer"
                        className="no-drag grid h-6 w-6 place-items-center rounded-md bg-black/55 text-white transition-colors hover:bg-red-500"
                      >
                        <Trash2 size={12} />
                      </button>
                    </div>
                  </div>
                </Reorder.Item>
              ))}
            </AnimatePresence>
          </Reorder.Group>

          {!loading && pages.length === 0 && (
            <p className="py-6 text-center text-sm text-muted">
              Toutes les pages ont été retirées.{' '}
              <button
                onClick={() => {
                  lastPath.current = ''
                  useStore.getState().clearFiles()
                }}
                className="font-medium text-ink underline"
              >
                Recharger
              </button>
            </p>
          )}
        </div>
      )}

      {/* Result */}
      <AnimatePresence>
        {done && tab === 'pages' && (
          <motion.div
            initial={{ opacity: 0, y: 10, scale: 0.98 }}
            animate={{ opacity: 1, y: 0, scale: 1 }}
            exit={{ opacity: 0, y: -6 }}
            transition={{ type: 'spring', stiffness: 360, damping: 28 }}
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

      {/* Action */}
      {file && (
        <div className="flex items-center justify-end gap-3 pb-4">
          <button
            onClick={() => {
              lastPath.current = ''
              useStore.getState().clearFiles()
            }}
            className="btn-ghost no-drag"
          >
            <Undo2 size={15} /> Changer de fichier
          </button>
          {tab === 'pages' && (
            <button onClick={save} disabled={!dirty || busy} className="btn-primary no-drag">
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
          )}
        </div>
      )}
    </div>
  )
}
