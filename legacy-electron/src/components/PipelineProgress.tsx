import { useEffect, useRef, useState } from 'react'
import { AnimatePresence, motion } from 'framer-motion'

/**
 * The star of the circuit: a ravenous snow cat crosses the scene devouring
 * every tree and colored snowman (each item = one slice of progress), then —
 * once everything is eaten — squats out one enormous poop that EXPLODES.
 *
 * Driven by a real `progress` fraction (0..1), but eased internally so even an
 * instant pipeline still gets a satisfying show. Calls `onFinished` after the
 * explosion so the caller can reveal the result.
 */

type SceneItem = { kind: 'tree' | 'snowman'; color: string }

// each element represents a percentage slice of the whole run
const SCENE: SceneItem[] = [
  { kind: 'tree', color: '#34d399' },
  { kind: 'snowman', color: '#6366f1' },
  { kind: 'tree', color: '#10b981' },
  { kind: 'snowman', color: '#f472b6' },
  { kind: 'tree', color: '#22c55e' },
  { kind: 'snowman', color: '#38bdf8' },
  { kind: 'tree', color: '#16a34a' },
  { kind: 'snowman', color: '#fbbf24' },
]

const ease = [0.22, 1, 0.36, 1] as const

function Tree({ color, eaten }: { color: string; eaten: boolean }) {
  return (
    <motion.svg
      width="40"
      height="52"
      viewBox="0 0 40 52"
      animate={eaten ? { opacity: 0, scale: 0.2, y: 8 } : { opacity: 1, scale: 1, y: 0 }}
      transition={{ duration: 0.25, ease: 'easeIn' }}
    >
      <rect x="17" y="38" width="6" height="12" rx="2" fill="#a16207" />
      <path d="M20 4 L32 24 H8 Z" fill={color} />
      <path d="M20 16 L34 40 H6 Z" fill={color} opacity="0.92" />
      <circle cx="14" cy="20" r="2" fill="#fff" opacity="0.7" />
      <circle cx="26" cy="34" r="2" fill="#fff" opacity="0.7" />
    </motion.svg>
  )
}

function Snowman({ color, eaten }: { color: string; eaten: boolean }) {
  return (
    <motion.svg
      width="34"
      height="52"
      viewBox="0 0 34 52"
      animate={
        eaten ? { opacity: 0, scale: 0.2, x: 14, rotate: 60 } : { opacity: 1, scale: 1, rotate: 0 }
      }
      transition={{ duration: 0.25, ease: 'easeIn' }}
    >
      <circle cx="17" cy="40" r="11" fill={color} />
      <circle cx="17" cy="22" r="8.5" fill={color} />
      <circle cx="17" cy="10" r="6" fill={color} />
      <circle cx="15" cy="9" r="1.1" fill="#1f2937" />
      <circle cx="19" cy="9" r="1.1" fill="#1f2937" />
      <path d="M17 11 L22 12 L17 13 Z" fill="#f97316" />
      <circle cx="17" cy="20" r="1.2" fill="#1f2937" />
      <circle cx="17" cy="24" r="1.2" fill="#1f2937" />
    </motion.svg>
  )
}

function HungryCat({ chewing }: { chewing: boolean }) {
  return (
    <motion.div animate={chewing ? { rotate: [0, -5, 0, -5, 0] } : {}} transition={{ duration: 0.4, repeat: Infinity }}>
      <svg width="84" height="66" viewBox="0 0 84 66" fill="none">
        <ellipse cx="46" cy="42" rx="30" ry="19" fill="#fafafa" />
        <circle cx="24" cy="32" r="20" fill="#ffffff" />
        <path d="M8 18 L15 32 L1 32 Z" fill="#ffffff" />
        <path d="M40 18 L33 32 L47 32 Z" fill="#ffffff" />
        <path d="M9 21 L13 30 L6 30 Z" fill="#fbcfe8" />
        <path d="M39 21 L35 30 L42 30 Z" fill="#fbcfe8" />
        <path d="M72 44 Q88 37 79 22" stroke="#ffffff" strokeWidth="9" strokeLinecap="round" fill="none" />
        <circle cx="17" cy="30" r="2.6" fill="#1f2937" />
        <circle cx="30" cy="30" r="2.6" fill="#1f2937" />
        {/* wide open, hungry mouth */}
        <motion.path
          d="M14 38 Q24 52 34 38 Z"
          fill="#fb7185"
          animate={chewing ? { scaleY: [1, 0.4, 1] } : {}}
          transition={{ duration: 0.28, repeat: Infinity }}
          style={{ transformOrigin: '24px 40px' }}
        />
        <path d="M1 34 H13 M1 38 H13" stroke="#cbd5e1" strokeWidth="1.3" />
      </svg>
    </motion.div>
  )
}

/** The grand finale: a poop that swells then bursts into particles. */
function PoopExplosion({ boom }: { boom: boolean }) {
  const bits = Array.from({ length: 14 })
  return (
    <div className="pointer-events-none absolute inset-0 grid place-items-center">
      <AnimatePresence>
        {!boom ? (
          <motion.div
            key="poop"
            initial={{ scale: 0, y: 10, opacity: 0 }}
            animate={{ scale: [0, 1.15, 1], y: 0, opacity: 1 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.5, ease }}
            className="text-5xl"
          >
            💩
          </motion.div>
        ) : (
          <div key="boom" className="relative">
            <motion.div
              initial={{ scale: 1, opacity: 1 }}
              animate={{ scale: 3.4, opacity: 0 }}
              transition={{ duration: 0.6, ease: 'easeOut' }}
              className="text-6xl"
            >
              💥
            </motion.div>
            {bits.map((_, i) => {
              const angle = (i / bits.length) * Math.PI * 2
              return (
                <motion.span
                  key={i}
                  className="absolute left-1/2 top-1/2 text-2xl"
                  initial={{ x: 0, y: 0, opacity: 1, scale: 0.6 }}
                  animate={{
                    x: Math.cos(angle) * 130,
                    y: Math.sin(angle) * 130,
                    opacity: 0,
                    scale: 1.1,
                    rotate: 180,
                  }}
                  transition={{ duration: 0.8, ease: 'easeOut' }}
                >
                  💩
                </motion.span>
              )
            })}
          </div>
        )}
      </AnimatePresence>
    </div>
  )
}

export function PipelineProgress({
  progress,
  label,
  onFinished,
}: {
  progress: number // real target fraction 0..1
  label: string
  onFinished?: () => void
}) {
  const [shown, setShown] = useState(0)
  const [phase, setPhase] = useState<'eating' | 'poop' | 'boom'>('eating')
  const target = useRef(progress)
  const finished = useRef(false)
  target.current = progress

  // ease the displayed progress toward the real target (min visible speed)
  useEffect(() => {
    let raf = 0
    let last = performance.now()
    const tick = (now: number) => {
      const dt = Math.min(0.05, (now - last) / 1000)
      last = now
      setShown((s) => {
        const goal = target.current
        // climb at a comfortable pace; never jump instantly
        const next = s + Math.max(goal - s, 0) * Math.min(1, dt * 3) + dt * 0.12
        return Math.min(goal, next)
      })
      raf = requestAnimationFrame(tick)
    }
    raf = requestAnimationFrame(tick)
    return () => cancelAnimationFrame(raf)
  }, [])

  // orchestrate the finale once everything is eaten. Scheduled exactly once
  // (guarded by a ref) — we must NOT return a cleanup here, or the next frame's
  // `shown` update would re-run this effect and clearTimeout our own finale.
  const finaleStarted = useRef(false)
  const timers = useRef<ReturnType<typeof setTimeout>[]>([])
  useEffect(() => () => timers.current.forEach(clearTimeout), [])
  useEffect(() => {
    if (finaleStarted.current) return
    if (progress >= 1 && shown >= 0.985) {
      finaleStarted.current = true
      setPhase('poop')
      timers.current.push(setTimeout(() => setPhase('boom'), 700))
      timers.current.push(
        setTimeout(() => {
          if (!finished.current) {
            finished.current = true
            onFinished?.()
          }
        }, 1500),
      )
    }
  }, [progress, shown, onFinished])

  const eatenCount = Math.round(shown * SCENE.length)
  const percent = Math.min(100, Math.round(shown * 100))

  return (
    <div className="glass reflect relative overflow-hidden rounded-2xl p-5">
      {/* snowy ground wash */}
      <div
        className="pointer-events-none absolute inset-0 opacity-40 blur-2xl"
        style={{
          background:
            'radial-gradient(30vmax 20vmax at 20% 80%, rgb(56 189 248 / 0.25), transparent), radial-gradient(30vmax 20vmax at 80% 70%, rgb(244 114 182 / 0.2), transparent)',
        }}
      />

      <div className="relative mb-3 flex items-center justify-between">
        <p className="text-[13px] font-semibold">{label || 'Exécution du circuit'}</p>
        <span className="tabular-nums text-xs font-semibold text-muted">{percent}%</span>
      </div>

      {/* the scene */}
      <div className="relative flex h-[64px] items-end">
        {/* items row */}
        <div className="flex w-full items-end justify-around px-8">
          {SCENE.map((item, i) =>
            item.kind === 'tree' ? (
              <Tree key={i} color={item.color} eaten={i < eatenCount} />
            ) : (
              <Snowman key={i} color={item.color} eaten={i < eatenCount} />
            ),
          )}
        </div>

        {/* the cat, gliding along with progress */}
        <motion.div
          className="absolute bottom-0"
          style={{ left: `calc(${percent}% - 42px)` }}
          animate={{ y: phase === 'poop' || phase === 'boom' ? 6 : [0, -3, 0] }}
          transition={
            phase === 'eating'
              ? { duration: 0.5, repeat: Infinity, ease: 'easeInOut' }
              : { duration: 0.3 }
          }
        >
          <HungryCat chewing={phase === 'eating' && percent > 0 && percent < 100} />
        </motion.div>

        {(phase === 'poop' || phase === 'boom') && <PoopExplosion boom={phase === 'boom'} />}
      </div>

      {/* thin progress rail */}
      <div className="relative mt-3 h-1.5 overflow-hidden rounded-full bg-[rgb(var(--line)/0.08)]">
        <motion.div
          className="h-full rounded-full"
          style={{
            width: `${percent}%`,
            background: 'linear-gradient(90deg, rgb(var(--g1)), rgb(var(--g3)))',
          }}
        />
      </div>
    </div>
  )
}
