import { useEffect, useState } from 'react'
import { GrainGradient } from './components/GrainGradient'
import { CursorGlow } from './components/CursorGlow'
import { TitleBar } from './components/TitleBar'
import { Sidebar } from './components/Sidebar'
import { Workspace } from './components/Workspace'
import { PipelineBuilder } from './components/PipelineBuilder'
import { PdfEditor } from './components/PdfEditor'
import { LoadingScreen } from './components/LoadingScreen'
import { useStore } from './store/useStore'

export default function App() {
  const [loading, setLoading] = useState(true)
  const [eating, setEating] = useState(false)
  const mode = useStore((s) => s.mode)

  useEffect(() => {
    // choreographed intro: dancers → snow cat eats → reveal app
    const t1 = setTimeout(() => setEating(true), 1700)
    const t2 = setTimeout(() => setLoading(false), 3400)
    return () => {
      clearTimeout(t1)
      clearTimeout(t2)
    }
  }, [])

  return (
    <div className="grain relative flex h-full flex-col">
      <GrainGradient />
      <CursorGlow />
      <LoadingScreen show={loading} eating={eating} />

      <div className="relative z-10 flex h-full flex-col">
        <TitleBar />
        <div className="flex min-h-0 flex-1">
          <Sidebar />
          <main className="min-h-0 flex-1 overflow-y-auto px-8 py-6">
            {mode === 'pipeline' ? (
              <PipelineBuilder />
            ) : mode === 'edit' ? (
              <PdfEditor />
            ) : (
              <Workspace />
            )}
          </main>
        </div>
      </div>
    </div>
  )
}
