import { useState, useCallback } from 'react'
import { motion } from 'framer-motion'
import { UploadCloud } from 'lucide-react'
import { GlassCard } from './GlassCard'
import { useStore } from '../store/useStore'
import { toolById } from '../lib/tools'
import { ext as getExt } from '../lib/utils'

type Props = {
  /** override accepted extensions (defaults to the active tool's) */
  accept?: string[]
  multiple?: boolean
  grad?: [string, string]
  /** disabled when a pipeline has no source step yet */
  hint?: string
}

export function DropZone({ accept, multiple, grad, hint }: Props = {}) {
  const tool = useStore((s) => s.tool)
  const addFiles = useStore((s) => s.addFiles)
  const toolDef = toolById(tool)
  const def = {
    accept: accept ?? toolDef.accept,
    multiple: multiple ?? toolDef.multiple,
    grad: grad ?? toolDef.grad,
  }
  const [hover, setHover] = useState(false)

  const ingest = useCallback(
    async (paths: string[]) => {
      const allowed = paths.filter((p) => def.accept.includes(getExt(p)))
      if (allowed.length === 0) return
      const infos = await window.pdf.fileInfo(def.multiple ? allowed : allowed.slice(0, 1))
      addFiles(infos)
    },
    [def, addFiles],
  )

  const onDrop = useCallback(
    (e: React.DragEvent) => {
      e.preventDefault()
      setHover(false)
      const paths = Array.from(e.dataTransfer.files)
        .map((f) => (f as File & { path: string }).path)
        .filter(Boolean)
      ingest(paths)
    },
    [ingest],
  )

  const pick = useCallback(async () => {
    const filters = [
      { name: 'Fichiers acceptés', extensions: def.accept },
    ]
    const paths = await window.pdf.openFiles(filters)
    if (paths.length) ingest(paths)
  }, [def, ingest])

  return (
    <GlassCard
      rim
      onClick={pick}
      onDragOver={(e) => {
        e.preventDefault()
        setHover(true)
      }}
      onDragLeave={() => setHover(false)}
      onDrop={onDrop}
      className="group cursor-pointer overflow-hidden p-10 transition-transform duration-500 ease-spring"
    >
      <motion.div
        animate={{ scale: hover ? 1.02 : 1 }}
        transition={{ type: 'spring', stiffness: 300, damping: 26 }}
        className="flex flex-col items-center justify-center gap-4 py-6 text-center"
      >
        <motion.div
          animate={{ y: hover ? -6 : 0 }}
          transition={{ type: 'spring', stiffness: 280, damping: 18 }}
          className="grid h-16 w-16 place-items-center rounded-2xl"
          style={{
            background: `linear-gradient(135deg, rgb(${def.grad[0]} / 0.9), rgb(${def.grad[1]} / 0.9))`,
            boxShadow: '0 16px 40px -12px rgb(var(--accent) / 0.5)',
          }}
        >
          <UploadCloud size={28} className="text-white" />
        </motion.div>
        <div className="space-y-1">
          <p className="text-base font-semibold">
            {hover ? 'Déposez ici' : 'Glissez vos fichiers ou cliquez'}
          </p>
          <p className="text-sm text-muted">
            {hint ?? def.accept.map((e) => e.toUpperCase()).join(' · ')}
          </p>
        </div>
      </motion.div>
    </GlassCard>
  )
}
