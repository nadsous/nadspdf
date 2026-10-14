import { AnimatePresence, motion, Reorder } from 'framer-motion'
import { X, GripVertical } from 'lucide-react'
import { useStore } from '../store/useStore'
import { formatBytes } from '../lib/utils'
import { toolById } from '../lib/tools'
import { fileVisual } from '../lib/fileVisual'

export function FileList({ reorderable: reorderableProp }: { reorderable?: boolean } = {}) {
  const files = useStore((s) => s.files)
  const setFiles = useStore((s) => s.setFiles)
  const removeFile = useStore((s) => s.removeFile)
  const tool = useStore((s) => s.tool)
  const def = toolById(tool)
  const reorderable =
    reorderableProp ?? (def.multiple && (tool === 'merge' || tool === 'images-to-pdf'))

  if (files.length === 0) return null

  const Row = ({ f }: { f: (typeof files)[number] }) => {
    const v = fileVisual(f.ext)
    const Icon = v.icon
    return (
      <div className="group flex items-center gap-3 px-4 py-3">
        {reorderable && (
          <GripVertical
            size={15}
            className="cursor-grab text-muted opacity-40 transition-opacity group-hover:opacity-100"
          />
        )}
        <span
          className="grid h-9 w-9 shrink-0 place-items-center rounded-lg shadow-sm transition-transform group-hover:scale-105"
          style={{ background: `linear-gradient(135deg, ${v.from}, ${v.to})` }}
        >
          <Icon size={16} className="text-white" />
        </span>
        <div className="min-w-0 flex-1">
          <p className="truncate text-[13.5px] font-medium">{f.name}</p>
          <p className="text-xs text-muted">
            {formatBytes(f.size)}
            {f.pages != null && ` · ${f.pages} page${f.pages > 1 ? 's' : ''}`}
          </p>
        </div>
        <span
          className="pill font-semibold"
          style={{ color: v.to, background: `${v.from}1f`, borderColor: `${v.from}33` }}
        >
          {v.label}
        </span>
        <button
          onClick={() => removeFile(f.path)}
          className="grid h-7 w-7 place-items-center rounded-lg text-muted opacity-0 transition-all hover:bg-[rgb(var(--line)/0.12)] hover:text-ink group-hover:opacity-100"
        >
          <X size={14} />
        </button>
      </div>
    )
  }

  return (
    <div className="glass reflect overflow-hidden rounded-2xl">
      <div className="flex items-center justify-between border-b border-[rgb(var(--line)/0.08)] px-4 py-3">
        <p className="text-[13px] font-semibold">
          {files.length} fichier{files.length > 1 ? 's' : ''}
        </p>
        <button
          onClick={() => useStore.getState().clearFiles()}
          className="text-xs font-medium text-muted transition-colors hover:text-ink"
        >
          Tout retirer
        </button>
      </div>

      {reorderable ? (
        <Reorder.Group
          axis="y"
          values={files}
          onReorder={setFiles}
          className="divide-y divide-[rgb(var(--line)/0.06)]"
        >
          {files.map((f) => (
            <Reorder.Item key={f.path} value={f} className="bg-transparent">
              <Row f={f} />
            </Reorder.Item>
          ))}
        </Reorder.Group>
      ) : (
        <div className="divide-y divide-[rgb(var(--line)/0.06)]">
          <AnimatePresence initial={false}>
            {files.map((f) => (
              <motion.div
                key={f.path}
                initial={{ opacity: 0, height: 0 }}
                animate={{ opacity: 1, height: 'auto' }}
                exit={{ opacity: 0, height: 0 }}
                transition={{ ease: [0.22, 1, 0.36, 1] }}
              >
                <Row f={f} />
              </motion.div>
            ))}
          </AnimatePresence>
        </div>
      )}
    </div>
  )
}
