import {
  Combine,
  FileImage,
  FileOutput,
  Minimize2,
  RotateCw,
  Scissors,
  Trash2,
  Stamp,
  Hash,
  type LucideIcon,
} from 'lucide-react'
import type { FileInfo } from '../../electron/preload'
import {
  imagesToPdfBytes,
  keepPagesBytes,
  mergeBytes,
  pageNumbersBytes,
  removePagesBytes,
  rotateBytes,
  watermarkBytes,
} from './pdfOps'
import { compressPdf, type CompressLevel } from './pdfCompress'

/** A configurable field rendered generically by the pipeline UI. */
export type Field =
  | { key: string; type: 'ranges'; label: string; default: string; placeholder?: string }
  | { key: string; type: 'text'; label: string; default: string; placeholder?: string }
  | { key: string; type: 'number'; label: string; default: number; min?: number; max?: number }
  | { key: string; type: 'toggle'; label: string; default: boolean }
  | {
      key: string
      type: 'select'
      label: string
      default: string
      options: { value: string; label: string; hint?: string }[]
    }

export type Params = Record<string, string | number | boolean>

export type RunInput = {
  current: Uint8Array | null
  files: FileInfo[]
  params: Params
  onProgress?: (fraction: number) => void
}

export type StepKind = {
  type: string
  label: string
  desc: string
  icon: LucideIcon
  /** 'source' turns the loaded files into the initial document; 'transform' edits bytes. */
  category: 'source' | 'transform'
  grad: [string, string]
  /** accepted extensions for source steps */
  accept?: string[]
  multiple?: boolean
  fields: Field[]
  run: (input: RunInput) => Promise<Uint8Array>
}

/** Read the bytes of every loaded file. */
async function readAll(files: FileInfo[]): Promise<Uint8Array[]> {
  return Promise.all(files.map((f) => window.pdf.readBytes(f.path)))
}

/** Resolve the document a transform step should act on: the running result,
 *  or (if this transform is used with no source) the first loaded PDF. */
async function resolveCurrent(input: RunInput): Promise<Uint8Array> {
  if (input.current) return input.current
  const pdf = input.files.find((f) => f.ext === 'pdf')
  if (!pdf) throw new Error('Aucun PDF en entrée — ajoutez d’abord une étape source')
  return window.pdf.readBytes(pdf.path)
}

export const STEP_KINDS: StepKind[] = [
  // ── Sources ────────────────────────────────────────────────────────────
  {
    type: 'convert',
    label: 'Convertir en PDF',
    desc: 'Word/Excel/CSV/HTML → PDF',
    icon: FileOutput,
    category: 'source',
    grad: ['var(--g1)', 'var(--g2)'],
    accept: ['docx', 'xlsx', 'xls', 'ods', 'csv', 'txt', 'rtf', 'html', 'htm', 'pdf'],
    multiple: true,
    fields: [],
    run: async ({ files }) => {
      const res = await window.pdf.convertBytes(files.map((f) => f.path))
      const ok = res.filter((r) => r.ok && r.bytes).map((r) => r.bytes as Uint8Array)
      if (ok.length === 0) throw new Error(res.find((r) => !r.ok)?.error ?? 'Conversion échouée')
      return ok.length === 1 ? ok[0] : mergeBytes(ok)
    },
  },
  {
    type: 'merge',
    label: 'Fusionner',
    desc: 'Combine tous les PDF en un seul',
    icon: Combine,
    category: 'source',
    grad: ['var(--g1)', 'var(--g3)'],
    accept: ['pdf'],
    multiple: true,
    fields: [],
    run: async ({ files }) => mergeBytes(await readAll(files)),
  },
  {
    type: 'images-to-pdf',
    label: 'Assembler des images',
    desc: 'PNG/JPG → un seul PDF',
    icon: FileImage,
    category: 'source',
    grad: ['var(--g4)', 'var(--g1)'],
    accept: ['png', 'jpg', 'jpeg'],
    multiple: true,
    fields: [],
    run: async ({ files }) => {
      const images = await Promise.all(
        files.map(async (f) => ({
          bytes: await window.pdf.readBytes(f.path),
          png: f.ext === 'png',
        })),
      )
      return imagesToPdfBytes(images)
    },
  },

  // ── Transforms ─────────────────────────────────────────────────────────
  {
    type: 'remove',
    label: 'Retirer des pages',
    desc: 'Supprime les pages indiquées',
    icon: Trash2,
    category: 'transform',
    grad: ['var(--g3)', 'var(--g1)'],
    fields: [
      { key: 'ranges', type: 'ranges', label: 'Pages à retirer', default: '', placeholder: 'ex : 2, 5, 8-10' },
    ],
    run: async (input) =>
      removePagesBytes(await resolveCurrent(input), String(input.params.ranges || '')),
  },
  {
    type: 'keep',
    label: 'Garder des pages',
    desc: 'Ne conserve que les pages indiquées',
    icon: Scissors,
    category: 'transform',
    grad: ['var(--g3)', 'var(--g1)'],
    fields: [
      { key: 'ranges', type: 'ranges', label: 'Pages à conserver', default: 'all', placeholder: 'ex : 1-3, 5' },
    ],
    run: async (input) =>
      keepPagesBytes(await resolveCurrent(input), String(input.params.ranges || 'all')),
  },
  {
    type: 'rotate',
    label: 'Pivoter',
    desc: 'Rotation des pages',
    icon: RotateCw,
    category: 'transform',
    grad: ['var(--g2)', 'var(--g4)'],
    fields: [
      {
        key: 'degrees',
        type: 'select',
        label: 'Angle',
        default: '90',
        options: [
          { value: '90', label: '90°' },
          { value: '180', label: '180°' },
          { value: '270', label: '270°' },
        ],
      },
      { key: 'ranges', type: 'ranges', label: 'Pages', default: 'all', placeholder: 'all ou 1-3' },
    ],
    run: async (input) =>
      rotateBytes(
        await resolveCurrent(input),
        Number(input.params.degrees) || 90,
        String(input.params.ranges || 'all'),
      ),
  },
  {
    type: 'watermark',
    label: 'Filigrane',
    desc: 'Texte en diagonale sur chaque page',
    icon: Stamp,
    category: 'transform',
    grad: ['var(--g2)', 'var(--g3)'],
    fields: [
      { key: 'text', type: 'text', label: 'Texte', default: 'CONFIDENTIEL', placeholder: 'Votre texte' },
      { key: 'opacity', type: 'number', label: 'Opacité (%)', default: 18, min: 5, max: 100 },
      { key: 'diagonal', type: 'toggle', label: 'En diagonale', default: true },
    ],
    run: async (input) =>
      watermarkBytes(await resolveCurrent(input), {
        text: String(input.params.text || 'CONFIDENTIEL'),
        opacity: (Number(input.params.opacity) || 18) / 100,
        diagonal: Boolean(input.params.diagonal),
      }),
  },
  {
    type: 'page-numbers',
    label: 'Numéros de page',
    desc: 'Numérotation en pied de page',
    icon: Hash,
    category: 'transform',
    grad: ['var(--g4)', 'var(--g2)'],
    fields: [
      { key: 'format', type: 'text', label: 'Format', default: '{n} / {total}', placeholder: '{n} / {total}' },
      {
        key: 'position',
        type: 'select',
        label: 'Position',
        default: 'bottom-center',
        options: [
          { value: 'bottom-left', label: 'Gauche' },
          { value: 'bottom-center', label: 'Centre' },
          { value: 'bottom-right', label: 'Droite' },
        ],
      },
    ],
    run: async (input) =>
      pageNumbersBytes(await resolveCurrent(input), {
        format: String(input.params.format || '{n} / {total}'),
        position: input.params.position as PageNumberPosition,
      }),
  },
  {
    type: 'compress',
    label: 'Compresser',
    desc: 'Réduit le poids du PDF',
    icon: Minimize2,
    category: 'transform',
    grad: ['var(--g4)', 'var(--g2)'],
    fields: [
      {
        key: 'level',
        type: 'select',
        label: 'Niveau',
        default: 'ebook',
        options: [
          { value: 'printer', label: 'Léger', hint: 'Qualité max' },
          { value: 'ebook', label: 'Moyen', hint: 'Équilibré' },
          { value: 'screen', label: 'Fort', hint: 'Plus petit' },
        ],
      },
    ],
    run: async (input) =>
      compressPdf(
        await resolveCurrent(input),
        (input.params.level as CompressLevel) || 'ebook',
        (d, t) => input.onProgress?.(d / t),
      ),
  },
]

type PageNumberPosition = 'bottom-center' | 'bottom-right' | 'bottom-left'

export const stepKind = (type: string) => STEP_KINDS.find((s) => s.type === type)

/** A concrete, configured step inside a user's pipeline. */
export type PipelineStep = {
  id: string
  type: string
  params: Params
}

/** Build default params for a step kind. */
export function defaultParams(kind: StepKind): Params {
  const p: Params = {}
  for (const f of kind.fields) p[f.key] = f.default
  return p
}

export type StepProgress = {
  index: number
  total: number
  label: string
  fraction: number // 0..1 within the current step
}

/**
 * Run every step in order, threading bytes from one to the next.
 * `onStep` reports coarse progress (which step) plus fine progress for
 * long steps like compression — the caller maps this to the loading animation.
 */
export async function runPipeline(
  steps: PipelineStep[],
  files: FileInfo[],
  onStep?: (p: StepProgress) => void,
): Promise<Uint8Array> {
  let current: Uint8Array | null = null
  for (let i = 0; i < steps.length; i++) {
    const kind = stepKind(steps[i].type)
    if (!kind) throw new Error(`Étape inconnue : ${steps[i].type}`)
    onStep?.({ index: i, total: steps.length, label: kind.label, fraction: 0 })
    current = await kind.run({
      current,
      files,
      params: steps[i].params,
      onProgress: (f) => onStep?.({ index: i, total: steps.length, label: kind.label, fraction: f }),
    })
    onStep?.({ index: i, total: steps.length, label: kind.label, fraction: 1 })
  }
  if (!current) throw new Error('Circuit vide')
  return current
}

/**
 * Validate a pipeline against the loaded files; returns a human message or
 * null. Blocks every combination the runner can't honour so the UI can keep
 * the user on a logical path.
 */
export function validatePipeline(steps: PipelineStep[], files: FileInfo[]): string | null {
  if (steps.length === 0) return 'Ajoutez au moins une étape'
  if (files.length === 0) return 'Ajoutez des fichiers'

  const first = stepKind(steps[0].type)

  // a source is only meaningful once, and only in first position
  for (let i = 1; i < steps.length; i++) {
    if (stepKind(steps[i].type)?.category === 'source')
      return 'Une seule étape « source » est permise, en première position'
  }

  if (first?.category === 'source') {
    // every loaded file must be a type this source accepts
    const bad = files.find((f) => !first.accept!.includes(f.ext))
    if (bad) return `« ${bad.name} » n’est pas compatible avec l’étape « ${first.label} »`
  } else {
    // transform-first: the runner only feeds it one PDF
    if (!files.every((f) => f.ext === 'pdf'))
      return 'Sans étape source, seuls des PDF peuvent être traités'
    if (files.length > 1)
      return 'Plusieurs fichiers : commencez par une étape source (ex. « Fusionner »)'
  }

  // required parameters per step
  for (const s of steps) {
    if (s.type === 'remove' && !String(s.params.ranges ?? '').trim())
      return 'Étape « Retirer des pages » : indiquez les pages à retirer'
    if (s.type === 'keep' && !String(s.params.ranges ?? '').trim())
      return 'Étape « Garder des pages » : indiquez les pages à conserver'
    if (s.type === 'watermark' && !String(s.params.text ?? '').trim())
      return 'Étape « Filigrane » : saisissez un texte'
  }
  return null
}
