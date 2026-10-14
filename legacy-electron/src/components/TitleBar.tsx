import { useEffect, useRef, useState } from 'react'
import { AnimatePresence, motion } from 'framer-motion'
import { Minus, Square, X, Palette, Check } from 'lucide-react'
import { useStore } from '../store/useStore'
import { THEMES, themeById } from '../lib/themes'
import logo from '../assets/logo.png'

export function TitleBar() {
  const theme = useStore((s) => s.theme)
  const setTheme = useStore((s) => s.setTheme)
  const isMac = navigator.userAgent.includes('Mac')
  const [open, setOpen] = useState(false)
  const ref = useRef<HTMLDivElement>(null)

  useEffect(() => {
    if (!open) return
    const onClick = (e: MouseEvent) => {
      if (ref.current && !ref.current.contains(e.target as Node)) setOpen(false)
    }
    window.addEventListener('mousedown', onClick)
    return () => window.removeEventListener('mousedown', onClick)
  }, [open])

  const current = themeById(theme)

  return (
    <header className="drag-region relative z-30 flex h-11 shrink-0 items-center justify-between px-4">
      <div className="flex items-center gap-2.5 pl-16">
        <div className="flex items-center gap-2">
          <img src={logo} alt="NADSPDF" className="h-5 w-5 rounded-[5px] shadow-sm" />
          <span className="text-[13px] font-semibold tracking-tight">NADSPDF</span>
          <span className="pill !py-0.5 !text-[10px] font-semibold text-muted">1.0.0 beta</span>
        </div>
      </div>

      <div className="no-drag flex items-center gap-1">
        <div ref={ref} className="relative">
          <button
            onClick={() => setOpen((v) => !v)}
            className="flex h-7 items-center gap-1.5 rounded-lg px-2 text-muted transition-colors hover:bg-[rgb(var(--line)/0.1)] hover:text-ink"
            title="Thème"
          >
            <span
              className="grid h-4 w-4 place-items-center rounded-full text-white ring-1 ring-[rgb(var(--line)/0.2)]"
              style={{
                background: `linear-gradient(135deg, rgb(${current.swatch[0]}), rgb(${current.swatch[1]}))`,
              }}
            >
              <current.icon size={9} />
            </span>
            <Palette size={14} />
          </button>

          <AnimatePresence>
            {open && (
              <motion.div
                initial={{ opacity: 0, y: -6, scale: 0.97 }}
                animate={{ opacity: 1, y: 0, scale: 1 }}
                exit={{ opacity: 0, y: -6, scale: 0.97 }}
                transition={{ type: 'spring', stiffness: 420, damping: 30 }}
                className="glass reflect absolute right-0 top-9 z-40 w-56 overflow-hidden rounded-2xl p-1.5"
              >
                <p className="px-2.5 pb-1 pt-1.5 text-[10px] font-semibold uppercase tracking-widest text-muted">
                  Thème
                </p>
                {THEMES.map((t) => {
                  const active = t.id === theme
                  const Icon = t.icon
                  return (
                    <button
                      key={t.id}
                      onClick={() => {
                        setTheme(t.id)
                        setOpen(false)
                      }}
                      className={`flex w-full items-center gap-2.5 rounded-xl px-2.5 py-2 text-left transition-colors ${
                        active ? 'bg-[rgb(var(--line)/0.1)]' : 'hover:bg-[rgb(var(--line)/0.06)]'
                      }`}
                    >
                      <span
                        className="grid h-6 w-6 shrink-0 place-items-center rounded-lg text-white ring-1 ring-[rgb(var(--line)/0.15)]"
                        style={{
                          background: `linear-gradient(135deg, rgb(${t.swatch[0]}), rgb(${t.swatch[1]}))`,
                        }}
                      >
                        <Icon size={13} />
                      </span>
                      <span className="min-w-0 flex-1">
                        <span className="block truncate text-[13px] font-semibold">{t.label}</span>
                        <span className="block truncate text-[11px] text-muted">{t.hint}</span>
                      </span>
                      {active && <Check size={14} className="shrink-0 text-ink" />}
                    </button>
                  )
                })}
              </motion.div>
            )}
          </AnimatePresence>
        </div>

        {!isMac && (
          <div className="ml-1 flex items-center gap-0.5">
            <button
              onClick={() => window.pdf.minimize()}
              className="grid h-7 w-9 place-items-center rounded-lg text-muted transition-colors hover:bg-[rgb(var(--line)/0.1)] hover:text-ink"
            >
              <Minus size={15} />
            </button>
            <button
              onClick={() => window.pdf.maximize()}
              className="grid h-7 w-9 place-items-center rounded-lg text-muted transition-colors hover:bg-[rgb(var(--line)/0.1)] hover:text-ink"
            >
              <Square size={12} />
            </button>
            <button
              onClick={() => window.pdf.close()}
              className="grid h-7 w-9 place-items-center rounded-lg text-muted transition-colors hover:bg-red-500 hover:text-white"
            >
              <X size={15} />
            </button>
          </div>
        )}
      </div>
    </header>
  )
}
