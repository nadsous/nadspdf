import { motion } from 'framer-motion'
import { Workflow, PencilRuler, type LucideIcon } from 'lucide-react'
import { TOOLS } from '../lib/tools'
import { useStore, type Mode } from '../store/useStore'
import { cn } from '../lib/utils'

type ModeEntry = { mode: Mode; label: string; icon: LucideIcon; grad: [string, string] }

const MODES: ModeEntry[] = [
  { mode: 'pipeline', label: 'Circuit', icon: Workflow, grad: ['var(--g1)', 'var(--g3)'] },
  { mode: 'edit', label: 'Éditer', icon: PencilRuler, grad: ['var(--g2)', 'var(--g4)'] },
]

export function Sidebar() {
  const mode = useStore((s) => s.mode)
  const tool = useStore((s) => s.tool)
  const setTool = useStore((s) => s.setTool)
  const setMode = useStore((s) => s.setMode)

  let idx = 0
  const Item = ({
    active,
    onClick,
    icon: Icon,
    label,
    grad,
  }: {
    active: boolean
    onClick: () => void
    icon: LucideIcon
    label: string
    grad: [string, string]
  }) => {
    const i = idx++
    return (
      <motion.button
        onClick={onClick}
        initial={{ opacity: 0, x: -12 }}
        animate={{ opacity: 1, x: 0 }}
        transition={{ delay: 0.03 * i, ease: [0.22, 1, 0.36, 1] }}
        className={cn(
          'no-drag group relative flex items-center gap-3 rounded-xl px-3 py-2.5 text-left transition-colors',
          active ? 'text-ink' : 'text-muted hover:text-ink',
        )}
      >
        {active && (
          <motion.div
            layoutId="active-nav"
            className="absolute inset-0 rounded-xl"
            style={{ background: 'rgb(var(--line) / 0.08)' }}
            transition={{ type: 'spring', stiffness: 480, damping: 38 }}
          />
        )}
        <span
          className="relative grid h-8 w-8 shrink-0 place-items-center rounded-lg transition-transform group-hover:scale-105"
          style={{
            background: active
              ? `linear-gradient(135deg, rgb(${grad[0]}), rgb(${grad[1]}))`
              : 'rgb(var(--line) / 0.06)',
          }}
        >
          <Icon size={16} className={active ? 'text-white' : ''} />
        </span>
        <span className="relative flex-1 text-[13.5px] font-medium">{label}</span>
      </motion.button>
    )
  }

  return (
    <nav className="z-20 flex w-[232px] shrink-0 flex-col gap-1 px-3 py-2">
      <p className="px-3 pb-2 pt-1 text-[11px] font-semibold uppercase tracking-widest text-muted">
        Ateliers
      </p>
      {MODES.map((m) => (
        <Item
          key={m.mode}
          active={mode === m.mode}
          onClick={() => setMode(m.mode)}
          icon={m.icon}
          label={m.label}
          grad={m.grad}
        />
      ))}

      <p className="px-3 pb-2 pt-4 text-[11px] font-semibold uppercase tracking-widest text-muted">
        Outils
      </p>
      {TOOLS.map((t) => (
        <Item
          key={t.id}
          active={mode === 'tool' && tool === t.id}
          onClick={() => setTool(t.id)}
          icon={t.icon}
          label={t.short}
          grad={t.grad}
        />
      ))}
    </nav>
  )
}
