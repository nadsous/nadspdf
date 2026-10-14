import { create } from 'zustand'
import type { FileInfo } from '../../electron/preload'
import { defaultParams, stepKind, type Params, type PipelineStep } from '../lib/pipeline'

export type ToolId =
  | 'convert'
  | 'compress'
  | 'merge'
  | 'split'
  | 'rotate'
  | 'pdf-to-images'
  | 'images-to-pdf'

/** Which workspace surface is active. */
export type Mode = 'tool' | 'pipeline' | 'edit'

export type Theme = 'dark' | 'light' | 'loubina' | 'yoyochill' | 'loulinhan'

let stepSeq = 0
const newId = () => `step-${Date.now()}-${stepSeq++}`

const THEME_KEY = 'nadspdf.theme'
const THEMES: Theme[] = ['dark', 'light', 'loubina', 'yoyochill', 'loulinhan']
function readStoredTheme(): Theme {
  try {
    const t = localStorage.getItem(THEME_KEY) as Theme | null
    if (t && THEMES.includes(t)) return t
  } catch {
    /* localStorage unavailable */
  }
  return 'dark'
}

type State = {
  theme: Theme
  mode: Mode
  tool: ToolId
  files: FileInfo[]
  pipeline: PipelineStep[]
  setTheme: (t: Theme) => void
  setMode: (m: Mode) => void
  setTool: (t: ToolId) => void
  setFiles: (f: FileInfo[]) => void
  addFiles: (f: FileInfo[]) => void
  removeFile: (path: string) => void
  reorder: (from: number, to: number) => void
  clearFiles: () => void
  // pipeline
  addStep: (type: string) => void
  removeStep: (id: string) => void
  updateStep: (id: string, params: Params) => void
  setPipeline: (p: PipelineStep[]) => void
  reorderSteps: (from: number, to: number) => void
  clearPipeline: () => void
}

export const useStore = create<State>((set) => ({
  theme: readStoredTheme(),
  mode: 'tool',
  tool: 'merge',
  files: [],
  pipeline: [],
  setTheme: (theme) => {
    document.documentElement.dataset.theme = theme
    try {
      localStorage.setItem(THEME_KEY, theme)
    } catch {
      /* ignore */
    }
    set({ theme })
  },
  setMode: (mode) => set({ mode, files: [] }),
  setTool: (tool) => set({ mode: 'tool', tool, files: [] }),
  setFiles: (files) => set({ files }),
  addFiles: (f) =>
    set((s) => {
      const seen = new Set(s.files.map((x) => x.path))
      return { files: [...s.files, ...f.filter((x) => !seen.has(x.path))] }
    }),
  removeFile: (path) => set((s) => ({ files: s.files.filter((f) => f.path !== path) })),
  reorder: (from, to) =>
    set((s) => {
      const next = [...s.files]
      const [m] = next.splice(from, 1)
      next.splice(to, 0, m)
      return { files: next }
    }),
  clearFiles: () => set({ files: [] }),

  addStep: (type) =>
    set((s) => {
      const kind = stepKind(type)
      if (!kind) return s
      const step = { id: newId(), type, params: defaultParams(kind) }
      if (kind.category === 'source') {
        // only one source, always kept first
        if (s.pipeline.some((p) => stepKind(p.type)?.category === 'source')) return s
        return { pipeline: [step, ...s.pipeline] }
      }
      return { pipeline: [...s.pipeline, step] }
    }),
  removeStep: (id) => set((s) => ({ pipeline: s.pipeline.filter((p) => p.id !== id) })),
  updateStep: (id, params) =>
    set((s) => ({
      pipeline: s.pipeline.map((p) => (p.id === id ? { ...p, params } : p)),
    })),
  setPipeline: (pipeline) => set({ pipeline }),
  reorderSteps: (from, to) =>
    set((s) => {
      const next = [...s.pipeline]
      const [m] = next.splice(from, 1)
      next.splice(to, 0, m)
      return { pipeline: next }
    }),
  clearPipeline: () => set({ pipeline: [] }),
}))
