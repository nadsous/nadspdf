import { useRef, useState } from 'react'
import { AnimatePresence, motion, Reorder } from 'framer-motion'
import {
  Plus,
  X,
  GripVertical,
  ArrowRight,
  CheckCircle2,
  AlertTriangle,
  FolderOpen,
  Workflow,
} from 'lucide-react'
import { useStore } from '../store/useStore'
import { DropZone } from './DropZone'
import { FileList } from './FileList'
import { PipelineProgress } from './PipelineProgress'
import {
  STEP_KINDS,
  stepKind,
  runPipeline,
  validatePipeline,
  type Field,
  type Params,
} from '../lib/pipeline'
import { formatBytes } from '../lib/utils'

const ease = [0.22, 1, 0.36, 1] as const
const pdfFilter = [{ name: 'PDF', extensions: ['pdf'] }]

type Done = { ok: boolean; message: string; outPath?: string }

export function PipelineBuilder() {
  const files = useStore((s) => s.files)
  const pipeline = useStore((s) => s.pipeline)
  const addStep = useStore((s) => s.addStep)
  const removeStep = useStore((s) => s.removeStep)
  const updateStep = useStore((s) => s.updateStep)
  const setPipeline = useStore((s) => s.setPipeline)

  const [adding, setAdding] = useState(false)
  const [phase, setPhase] = useState<'idle' | 'running'>('idle')
  const [progress, setProgress] = useState(0)
  const [label, setLabel] = useState('')
  const [done, setDone] = useState<Done | null>(null)
  const result = useRef<Uint8Array | null>(null)

  // source step drives which files are accepted
  const source = pipeline[0] && stepKind(pipeline[0].type)?.category === 'source'
    ? stepKind(pipeline[0].type)!
    : null
  // when no source is chosen yet, accept a broad set and always allow multiple
  // files so nothing is silently dropped before the source step is added.
  const ALL_ACCEPT = ['pdf', 'png', 'jpg', 'jpeg', 'docx', 'xlsx', 'xls', 'ods', 'csv', 'txt', 'rtf', 'html', 'htm']
  const accept = source?.accept ?? ALL_ACCEPT
  const multiple = source ? source.multiple : true
  const reorderableFiles = source ? source.multiple : files.length > 1

  const validationError = validatePipeline(pipeline, files)

  async function run() {
    if (validationError) {
      setDone({ ok: false, message: validationError })
      return
    }
    setDone(null)
    result.current = null
    setProgress(0)
    setLabel(stepKind(pipeline[0].type)?.label ?? '')
    setPhase('running')
    try {
      const bytes = await runPipeline(pipeline, files, (p) => {
        setLabel(p.label)
        setProgress((p.index + p.fraction) / p.total)
      })
      result.current = bytes
      setProgress(1) // let the animation reach its grand finale
    } catch (e) {
      setPhase('idle')
      setDone({ ok: false, message: String(e) })
    }
  }

  // called by the animation once the cat has exploded
  async function finalize() {
    const bytes = result.current
    if (!bytes) {
      setPhase('idle')
      return
    }
    const base = files[0]?.name.replace(/\.[^.]+$/, '') ?? 'document'
    const out = await window.pdf.saveFile(`${base}-circuit.pdf`, pdfFilter)
    if (!out) {
      setPhase('idle')
      return
    }
    const r = await window.pdf.writeBytes(out, bytes)
    setDone({
      ok: r.ok,
      message: r.ok ? `Enregistré · ${formatBytes(r.sizeAfter ?? bytes.length)}` : r.error ?? 'Échec',
      outPath: out,
    })
    setPhase('idle')
  }

  const sources = STEP_KINDS.filter((k) => k.category === 'source')
  const transforms = STEP_KINDS.filter((k) => k.category === 'transform')

  return (
    <div className="mx-auto flex w-full max-w-2xl flex-col gap-5">
      {/* Header */}
      <div className="flex items-start gap-4">
        <div
          className="grid h-12 w-12 shrink-0 place-items-center rounded-2xl"
          style={{ background: 'linear-gradient(135deg, rgb(var(--g1)), rgb(var(--g3)))' }}
        >
          <Workflow size={22} className="text-white" />
        </div>
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">Circuit</h1>
          <p className="mt-0.5 text-sm text-muted text-balance">
            Enchaînez plusieurs opérations : le résultat de chaque étape alimente la suivante, une
            seule sauvegarde à la fin.
          </p>
        </div>
      </div>

      <DropZone
        accept={accept}
        multiple={multiple}
        grad={['var(--g1)', 'var(--g3)']}
        hint={
          source
            ? accept.map((e) => e.toUpperCase()).join(' · ')
            : 'PDF · ajoutez une étape source pour d’autres formats'
        }
      />
      <FileList reorderable={reorderableFiles} />

      {/* Steps */}
      <div className="glass reflect overflow-hidden rounded-2xl">
        <div className="flex items-center justify-between border-b border-[rgb(var(--line)/0.08)] px-4 py-3">
          <p className="text-[13px] font-semibold">
            {pipeline.length === 0 ? 'Aucune étape' : `${pipeline.length} étape${pipeline.length > 1 ? 's' : ''}`}
          </p>
          {pipeline.length > 0 && (
            <button
              onClick={() => useStore.getState().clearPipeline()}
              className="text-xs font-medium text-muted transition-colors hover:text-ink"
            >
              Tout effacer
            </button>
          )}
        </div>

        {pipeline.length === 0 ? (
          <div className="px-4 py-8 text-center text-sm text-muted">
            Ajoutez une première étape ci-dessous.
          </div>
        ) : (
          <Reorder.Group
            axis="y"
            values={pipeline}
            onReorder={(next) => {
              // keep the source step pinned at the top
              const srcIdx = next.findIndex((p) => stepKind(p.type)?.category === 'source')
              if (srcIdx > 0) {
                const arr = [...next]
                const [s] = arr.splice(srcIdx, 1)
                arr.unshift(s)
                setPipeline(arr)
              } else setPipeline(next)
            }}
            className="divide-y divide-[rgb(var(--line)/0.06)]"
          >
            {pipeline.map((step, i) => {
              const kind = stepKind(step.type)
              if (!kind) return null
              const Icon = kind.icon
              return (
                <Reorder.Item key={step.id} value={step} className="bg-transparent">
                  <div className="group px-4 py-3">
                    <div className="flex items-center gap-3">
                      <GripVertical
                        size={15}
                        className="cursor-grab text-muted opacity-40 transition-opacity group-hover:opacity-100"
                      />
                      <span className="grid h-4 w-4 shrink-0 place-items-center text-[11px] font-bold text-muted">
                        {i + 1}
                      </span>
                      <span
                        className="grid h-9 w-9 shrink-0 place-items-center rounded-lg shadow-sm"
                        style={{
                          background: `linear-gradient(135deg, rgb(${kind.grad[0]}), rgb(${kind.grad[1]}))`,
                        }}
                      >
                        <Icon size={16} className="text-white" />
                      </span>
                      <div className="min-w-0 flex-1">
                        <p className="truncate text-[13.5px] font-medium">{kind.label}</p>
                        <p className="truncate text-xs text-muted">{kind.desc}</p>
                      </div>
                      {i === 0 && kind.category === 'source' && (
                        <span className="pill font-semibold text-muted">source</span>
                      )}
                      <button
                        onClick={() => removeStep(step.id)}
                        className="grid h-7 w-7 place-items-center rounded-lg text-muted opacity-0 transition-all hover:bg-[rgb(var(--line)/0.12)] hover:text-ink group-hover:opacity-100"
                      >
                        <X size={14} />
                      </button>
                    </div>

                    {kind.fields.length > 0 && (
                      <div className="mt-3 grid gap-3 pl-[52px] sm:grid-cols-2">
                        {kind.fields.map((f) => (
                          <FieldControl
                            key={f.key}
                            field={f}
                            value={step.params[f.key]}
                            onChange={(v) => updateStep(step.id, { ...step.params, [f.key]: v })}
                          />
                        ))}
                      </div>
                    )}
                  </div>
                </Reorder.Item>
              )
            })}
          </Reorder.Group>
        )}

        {/* add-step control */}
        <div className="border-t border-[rgb(var(--line)/0.08)] p-3">
          <button
            onClick={() => setAdding((v) => !v)}
            className="no-drag flex w-full items-center justify-center gap-2 rounded-xl border border-dashed border-[rgb(var(--line)/0.2)] py-2.5 text-sm font-medium text-muted transition-colors hover:border-[rgb(var(--accent)/0.5)] hover:text-ink"
          >
            <Plus size={15} /> Ajouter une étape
          </button>
          <AnimatePresence>
            {adding && (
              <motion.div
                initial={{ opacity: 0, height: 0 }}
                animate={{ opacity: 1, height: 'auto' }}
                exit={{ opacity: 0, height: 0 }}
                transition={{ ease }}
                className="overflow-hidden"
              >
                <div className="pt-3">
                  {!source && (
                    <>
                      <p className="mb-1.5 px-1 text-[11px] font-semibold uppercase tracking-wider text-muted">
                        Source <span className="text-muted/70">· placée en tête</span>
                      </p>
                      <div className="mb-3 grid grid-cols-2 gap-2">
                        {sources.map((k) => (
                          <StepChip
                            key={k.type}
                            kind={k}
                            onClick={() => {
                              addStep(k.type)
                              setAdding(false)
                            }}
                          />
                        ))}
                      </div>
                    </>
                  )}
                  <p className="mb-1.5 px-1 text-[11px] font-semibold uppercase tracking-wider text-muted">
                    Transformation
                  </p>
                  <div className="grid grid-cols-2 gap-2">
                    {transforms.map((k) => (
                      <StepChip
                        key={k.type}
                        kind={k}
                        onClick={() => {
                          addStep(k.type)
                          setAdding(false)
                        }}
                      />
                    ))}
                  </div>
                </div>
              </motion.div>
            )}
          </AnimatePresence>
        </div>
      </div>

      {/* Running animation */}
      {phase === 'running' && (
        <PipelineProgress progress={progress} label={label} onFinished={finalize} />
      )}

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
              <p className="text-[13.5px] font-semibold">{done.ok ? 'Circuit terminé' : 'Erreur'}</p>
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
      <div className="flex items-center justify-between gap-3 pb-4">
        <p className="text-xs text-muted">{validationError && phase === 'idle' ? validationError : ''}</p>
        <button
          onClick={run}
          disabled={!!validationError || phase === 'running'}
          className="btn-primary no-drag"
        >
          Exécuter le circuit <ArrowRight size={16} />
        </button>
      </div>
    </div>
  )
}

function StepChip({ kind, onClick }: { kind: (typeof STEP_KINDS)[number]; onClick: () => void }) {
  const Icon = kind.icon
  return (
    <button
      onClick={onClick}
      className="no-drag flex items-center gap-2.5 rounded-xl border border-[rgb(var(--line)/0.1)] bg-[rgb(var(--line)/0.04)] px-3 py-2.5 text-left transition-colors hover:border-[rgb(var(--accent)/0.5)] hover:bg-[rgb(var(--line)/0.08)]"
    >
      <span
        className="grid h-8 w-8 shrink-0 place-items-center rounded-lg"
        style={{ background: `linear-gradient(135deg, rgb(${kind.grad[0]}), rgb(${kind.grad[1]}))` }}
      >
        <Icon size={15} className="text-white" />
      </span>
      <span className="min-w-0">
        <span className="block truncate text-[13px] font-semibold">{kind.label}</span>
        <span className="block truncate text-[11px] text-muted">{kind.desc}</span>
      </span>
    </button>
  )
}

function FieldControl({
  field,
  value,
  onChange,
}: {
  field: Field
  value: Params[string]
  onChange: (v: Params[string]) => void
}) {
  const labelCls = 'mb-1 block text-[11px] font-semibold uppercase tracking-wider text-muted'
  const inputCls =
    'w-full rounded-lg border border-[rgb(var(--line)/0.1)] bg-[rgb(var(--line)/0.05)] px-3 py-2 text-sm outline-none transition-colors focus:border-[rgb(var(--accent)/0.6)]'

  if (field.type === 'select') {
    return (
      <div>
        <label className={labelCls}>{field.label}</label>
        <div className="flex gap-1.5">
          {field.options.map((o) => {
            const active = String(value) === o.value
            return (
              <button
                key={o.value}
                onClick={() => onChange(o.value)}
                className={`flex-1 rounded-lg border px-2 py-2 text-center text-xs font-semibold transition-colors ${
                  active
                    ? 'border-[rgb(var(--accent)/0.6)] bg-[rgb(var(--accent)/0.12)] text-ink'
                    : 'border-[rgb(var(--line)/0.1)] text-muted hover:text-ink'
                }`}
              >
                {o.label}
              </button>
            )
          })}
        </div>
      </div>
    )
  }

  if (field.type === 'toggle') {
    return (
      <div className="flex items-center justify-between rounded-lg border border-[rgb(var(--line)/0.1)] bg-[rgb(var(--line)/0.05)] px-3 py-2">
        <span className="text-[13px] font-medium">{field.label}</span>
        <button
          onClick={() => onChange(!value)}
          className={`relative h-5 w-9 rounded-full transition-colors ${
            value ? 'bg-[rgb(var(--accent))]' : 'bg-[rgb(var(--line)/0.2)]'
          }`}
        >
          <motion.span
            className="absolute top-0.5 h-4 w-4 rounded-full bg-white shadow"
            animate={{ left: value ? 18 : 2 }}
            transition={{ type: 'spring', stiffness: 500, damping: 34 }}
          />
        </button>
      </div>
    )
  }

  if (field.type === 'number') {
    return (
      <div>
        <label className={labelCls}>{field.label}</label>
        <input
          type="number"
          value={Number(value)}
          min={field.min}
          max={field.max}
          onChange={(e) => onChange(Number(e.target.value))}
          className={inputCls}
        />
      </div>
    )
  }

  // text + ranges
  return (
    <div>
      <label className={labelCls}>{field.label}</label>
      <input
        value={String(value ?? '')}
        placeholder={field.placeholder}
        onChange={(e) => onChange(e.target.value)}
        className={inputCls}
      />
    </div>
  )
}
