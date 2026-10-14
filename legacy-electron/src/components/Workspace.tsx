import { useState } from 'react'
import { AnimatePresence, motion } from 'framer-motion'
import { ArrowRight, CheckCircle2, AlertTriangle, FolderOpen, Loader2 } from 'lucide-react'
import { useStore } from '../store/useStore'
import { toolById } from '../lib/tools'
import { DropZone } from './DropZone'
import { FileList } from './FileList'
import { PdfPreview } from './PdfPreview'
import { rangesToSet, setToRanges } from '../lib/ranges'
import { formatBytes } from '../lib/utils'
import { pdfToPngs } from '../lib/pdfRender'
import { compressPdf } from '../lib/pdfCompress'
import type { OpResult } from '../../electron/preload'

type Done = { ok: boolean; message: string; outPath?: string }

const ease = [0.22, 1, 0.36, 1] as const

export function Workspace() {
  const tool = useStore((s) => s.tool)
  const files = useStore((s) => s.files)
  const def = toolById(tool)

  const [busy, setBusy] = useState(false)
  const [progress, setProgress] = useState<number | null>(null)
  const [done, setDone] = useState<Done | null>(null)

  // per-tool options
  const [level, setLevel] = useState<'screen' | 'ebook' | 'printer'>('ebook')
  const [ranges, setRanges] = useState('all')
  const [degrees, setDegrees] = useState(90)
  const [scale, setScale] = useState(2)

  const reset = () => {
    setDone(null)
    setProgress(null)
  }

  const first = files[0]
  const showPreview =
    files.length === 1 && first?.ext === 'pdf' && ['split', 'rotate', 'compress', 'pdf-to-images'].includes(tool)
  const pageCount = first?.pages ?? 0

  const togglePage = (page: number) => {
    const set = rangesToSet(ranges, pageCount)
    set.has(page) ? set.delete(page) : set.add(page)
    setRanges(setToRanges(set, pageCount))
  }

  async function run() {
    if (files.length === 0) return
    setBusy(true)
    setDone(null)
    setProgress(null)
    try {
      const first = files[0]
      const baseName = first.name.replace(/\.[^.]+$/, '')

      if (tool === 'merge') {
        const out = await window.pdf.saveFile('fusion.pdf', pdfFilter)
        if (!out) return
        finish(await window.pdf.merge(files.map((f) => f.path), out))
      } else if (tool === 'images-to-pdf') {
        const out = await window.pdf.saveFile('images.pdf', pdfFilter)
        if (!out) return
        finish(await window.pdf.imagesToPdf(files.map((f) => f.path), out))
      } else if (tool === 'compress') {
        const out = await window.pdf.saveFile(`${baseName}-compressé.pdf`, pdfFilter)
        if (!out) return
        setProgress(0)
        const input = await window.pdf.readBytes(first.path)
        const compressed = await compressPdf(input, level, (d, t) => setProgress(d / t))
        const r = await window.pdf.writeBytes(out, compressed)
        if (r.ok && r.sizeAfter) {
          const saved = Math.max(0, 100 - (r.sizeAfter / first.size) * 100)
          finish(
            r,
            `${formatBytes(first.size)} → ${formatBytes(r.sizeAfter)} · −${saved.toFixed(0)}%`,
          )
        } else finish(r)
      } else if (tool === 'rotate') {
        const out = await window.pdf.saveFile(`${baseName}-pivoté.pdf`, pdfFilter)
        if (!out) return
        finish(await window.pdf.rotate(first.path, degrees, out))
      } else if (tool === 'split') {
        const dir = await window.pdf.openDir()
        if (!dir) return
        finish(await window.pdf.split(first.path, ranges, dir))
      } else if (tool === 'convert') {
        const dir = await window.pdf.openDir()
        if (!dir) return
        const results = await window.pdf.convert(files.map((f) => f.path), dir)
        const okCount = results.filter((r) => r.ok).length
        const firstErr = results.find((r) => !r.ok)
        setDone({
          ok: okCount > 0,
          message: okCount === results.length
            ? `${okCount} fichier(s) converti(s)`
            : firstErr?.error ?? `${okCount}/${results.length} convertis`,
          outPath: results.find((r) => r.ok)?.outPath,
        })
      } else if (tool === 'pdf-to-images') {
        const dir = await window.pdf.openDir()
        if (!dir) return
        setProgress(0)
        const bytes = await window.pdf.readBytes(first.path)
        const pngs = await pdfToPngs(bytes, scale, (d, t) => setProgress(d / t))
        let written = 0
        for (let i = 0; i < pngs.length; i++) {
          const target = `${dir}/${baseName}-page-${String(i + 1).padStart(3, '0')}.png`
          const r = await window.pdf.writeBytes(target, pngs[i])
          if (r.ok) written++
        }
        setDone({ ok: written > 0, message: `${written} image(s) exportée(s)`, outPath: dir })
      }
    } catch (e) {
      setDone({ ok: false, message: String(e) })
    } finally {
      setBusy(false)
      setProgress(null)
    }
  }

  function finish(r: OpResult, extra?: string) {
    setDone({
      ok: r.ok,
      message: r.ok ? extra ?? 'Terminé' : r.error ?? 'Échec',
      outPath: r.outPath,
    })
  }

  return (
    <div className="mx-auto flex w-full max-w-2xl flex-col gap-5">
      {/* Header */}
      <motion.div
        key={tool}
        initial={{ opacity: 0, y: 10 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ ease }}
        className="flex items-start gap-4"
      >
        <div
          className="grid h-12 w-12 shrink-0 place-items-center rounded-2xl"
          style={{ background: `linear-gradient(135deg, rgb(${def.grad[0]}), rgb(${def.grad[1]}))` }}
        >
          <def.icon size={22} className="text-white" />
        </div>
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">{def.label}</h1>
          <p className="mt-0.5 text-sm text-muted text-balance">{def.desc}</p>
        </div>
      </motion.div>

      <DropZone />
      <FileList />

      {/* Options */}
      <AnimatePresence>
        {files.length > 0 && (
          <motion.div
            initial={{ opacity: 0, y: 8 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0, y: 8 }}
            transition={{ ease }}
            className="flex flex-col gap-4"
          >
            {tool === 'compress' && (
              <Segmented
                label="Niveau de compression"
                value={level}
                onChange={(v) => setLevel(v as typeof level)}
                options={[
                  { value: 'printer', label: 'Léger', hint: 'Qualité max' },
                  { value: 'ebook', label: 'Moyen', hint: 'Équilibré' },
                  { value: 'screen', label: 'Fort', hint: 'Plus petit' },
                ]}
              />
            )}
            {tool === 'rotate' && (
              <Segmented
                label="Rotation"
                value={String(degrees)}
                onChange={(v) => setDegrees(Number(v))}
                options={[
                  { value: '90', label: '90°' },
                  { value: '180', label: '180°' },
                  { value: '270', label: '270°' },
                ]}
              />
            )}
            {tool === 'pdf-to-images' && (
              <Segmented
                label="Qualité"
                value={String(scale)}
                onChange={(v) => setScale(Number(v))}
                options={[
                  { value: '1', label: '1×' },
                  { value: '2', label: '2×', hint: 'HD' },
                  { value: '3', label: '3×', hint: 'Max' },
                ]}
              />
            )}
            {tool === 'split' && (
              <div className="glass reflect rounded-2xl p-4">
                <label className="mb-2 block text-[13px] font-semibold">Pages à extraire</label>
                <input
                  value={ranges}
                  onChange={(e) => setRanges(e.target.value)}
                  placeholder="ex : 1-3, 5, 8-10  ou  all"
                  className="w-full rounded-xl border border-[rgb(var(--line)/0.1)] bg-[rgb(var(--line)/0.05)] px-3.5 py-2.5 text-sm outline-none transition-colors focus:border-[rgb(var(--accent)/0.6)]"
                />
                <p className="mt-2 text-xs text-muted">
                  Séparez par des virgules, ou cliquez les pages dans l’aperçu ci-dessous.
                </p>
              </div>
            )}
          </motion.div>
        )}
      </AnimatePresence>

      {/* Live PDF preview */}
      {showPreview && (
        <PdfPreview
          file={first}
          mode={tool === 'split' ? 'select' : 'view'}
          selected={tool === 'split' ? rangesToSet(ranges, pageCount) : undefined}
          onToggle={tool === 'split' ? togglePage : undefined}
          rotate={tool === 'rotate' ? degrees : 0}
        />
      )}

      {/* Progress */}
      <AnimatePresence>
        {busy && (
          <motion.div
            initial={{ opacity: 0 }}
            animate={{ opacity: 1 }}
            exit={{ opacity: 0 }}
            className="h-1.5 overflow-hidden rounded-full bg-[rgb(var(--line)/0.08)]"
          >
            <motion.div
              className="h-full rounded-full"
              style={{
                background: `linear-gradient(90deg, rgb(${def.grad[0]}), rgb(${def.grad[1]}))`,
                width: progress == null ? '40%' : `${Math.round(progress * 100)}%`,
              }}
              animate={progress == null ? { x: ['-40%', '320%'] } : {}}
              transition={
                progress == null
                  ? { repeat: Infinity, duration: 1.1, ease: 'easeInOut' }
                  : { ease }
              }
            />
          </motion.div>
        )}
      </AnimatePresence>

      {/* Result */}
      <AnimatePresence>
        {done && (
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
      <div className="flex items-center justify-end gap-3 pb-4">
        {done && (
          <button onClick={reset} className="btn-ghost no-drag">
            Recommencer
          </button>
        )}
        <button
          onClick={run}
          disabled={files.length === 0 || busy}
          className="btn-primary no-drag"
        >
          {busy ? (
            <>
              <Loader2 size={16} className="animate-spin" /> En cours…
            </>
          ) : (
            <>
              {def.short} <ArrowRight size={16} />
            </>
          )}
        </button>
      </div>
    </div>
  )
}

const pdfFilter = [{ name: 'PDF', extensions: ['pdf'] }]

function Segmented({
  label,
  value,
  onChange,
  options,
}: {
  label: string
  value: string
  onChange: (v: string) => void
  options: { value: string; label: string; hint?: string }[]
}) {
  return (
    <div className="glass reflect rounded-2xl p-4">
      <p className="mb-3 text-[13px] font-semibold">{label}</p>
      <div className="grid grid-flow-col gap-2" style={{ gridAutoColumns: '1fr' }}>
        {options.map((o) => {
          const active = o.value === value
          return (
            <button
              key={o.value}
              onClick={() => onChange(o.value)}
              className="no-drag relative rounded-xl px-3 py-2.5 text-center transition-colors"
            >
              {active && (
                <motion.div
                  layoutId={`seg-${label}`}
                  className="absolute inset-0 rounded-xl border border-[rgb(var(--line)/0.12)]"
                  style={{ background: 'rgb(var(--line) / 0.1)' }}
                  transition={{ type: 'spring', stiffness: 480, damping: 36 }}
                />
              )}
              <span
                className={`relative block text-sm font-semibold ${active ? 'text-ink' : 'text-muted'}`}
              >
                {o.label}
              </span>
              {o.hint && (
                <span className="relative mt-0.5 block text-[11px] text-muted">{o.hint}</span>
              )}
            </button>
          )
        })}
      </div>
    </div>
  )
}
