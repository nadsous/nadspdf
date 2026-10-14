import { useEffect, useRef, useState } from 'react'
import { motion } from 'framer-motion'
import { Check, Loader2 } from 'lucide-react'
import { renderThumbs, type Thumb } from '../lib/pdfThumbs'
import type { FileInfo } from '../../electron/preload'
import { cn } from '../lib/utils'

type Props = {
  file: FileInfo
  mode?: 'view' | 'select'
  selected?: Set<number>
  onToggle?: (page: number) => void
  rotate?: number
}

export function PdfPreview({ file, mode = 'view', selected, onToggle, rotate = 0 }: Props) {
  const [thumbs, setThumbs] = useState<Thumb[]>([])
  const [loading, setLoading] = useState(true)
  const lastPath = useRef<string>('')

  useEffect(() => {
    if (file.ext !== 'pdf') return
    if (lastPath.current === file.path) return
    lastPath.current = file.path
    let alive = true
    setLoading(true)
    setThumbs([])
    ;(async () => {
      try {
        const bytes = await window.pdf.readBytes(file.path)
        await renderThumbs(bytes, 0.55, (t) => {
          if (alive) setThumbs((prev) => [...prev, t])
        })
      } finally {
        if (alive) setLoading(false)
      }
    })()
    return () => {
      alive = false
    }
  }, [file.path, file.ext])

  if (file.ext !== 'pdf') return null

  return (
    <div className="glass reflect rounded-2xl p-4">
      <div className="mb-3 flex items-center justify-between">
        <p className="text-[13px] font-semibold">
          {mode === 'select' ? 'Sélectionnez les pages' : 'Aperçu'}
        </p>
        <span className="text-xs text-muted">
          {loading ? (
            <span className="inline-flex items-center gap-1.5">
              <Loader2 size={12} className="animate-spin" /> rendu…
            </span>
          ) : (
            `${thumbs.length} page${thumbs.length > 1 ? 's' : ''}`
          )}
        </span>
      </div>

      <div className="grid max-h-[360px] grid-cols-[repeat(auto-fill,minmax(96px,1fr))] gap-3 overflow-y-auto pr-1">
        {thumbs.map((t) => {
          const isSel = selected?.has(t.page)
          const interactive = mode === 'select'
          return (
            <motion.button
              key={t.page}
              type="button"
              initial={{ opacity: 0, scale: 0.92 }}
              animate={{ opacity: 1, scale: 1 }}
              transition={{ ease: [0.22, 1, 0.36, 1] }}
              onClick={() => interactive && onToggle?.(t.page)}
              className={cn(
                'group relative overflow-hidden rounded-lg border bg-white',
                interactive ? 'cursor-pointer' : 'cursor-default',
                isSel
                  ? 'border-transparent ring-2 ring-[rgb(var(--accent))]'
                  : 'border-[rgb(var(--line)/0.12)] hover:border-[rgb(var(--line)/0.3)]',
              )}
            >
              <img
                src={t.url}
                alt={`Page ${t.page}`}
                className="block h-full w-full object-contain transition-transform"
                style={{ transform: `rotate(${rotate}deg)` }}
              />
              <span className="absolute bottom-1 right-1 rounded bg-black/55 px-1.5 py-0.5 text-[10px] font-medium text-white">
                {t.page}
              </span>
              {interactive && (
                <span
                  className={cn(
                    'absolute left-1.5 top-1.5 grid h-5 w-5 place-items-center rounded-full border transition-all',
                    isSel
                      ? 'border-transparent bg-[rgb(var(--accent))] text-white'
                      : 'border-black/20 bg-white/70 text-transparent group-hover:border-black/40',
                  )}
                >
                  <Check size={12} strokeWidth={3} />
                </span>
              )}
            </motion.button>
          )
        })}
      </div>
    </div>
  )
}
