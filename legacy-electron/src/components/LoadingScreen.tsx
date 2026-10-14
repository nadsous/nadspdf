import { AnimatePresence, motion } from 'framer-motion'
import logo from '../assets/logo.png'

const FIGURES = [
  { c: '#6366f1' },
  { c: '#38bdf8' },
  { c: '#f472b6' },
  { c: '#2dd4bf' },
  { c: '#fbbf24' },
]

/** A little dancing person (head + body), bouncing in place. */
function Dancer({ color, i, eaten }: { color: string; i: number; eaten: boolean }) {
  return (
    <motion.div
      className="relative"
      initial={{ opacity: 0, y: 20 }}
      animate={
        eaten
          ? { opacity: 0, scale: 0.2, x: 40, rotate: 80 }
          : {
              opacity: 1,
              y: [0, -16, 0],
              rotate: [-8, 8, -8],
            }
      }
      transition={
        eaten
          ? { duration: 0.35, ease: 'easeIn' }
          : { duration: 0.7, repeat: Infinity, ease: 'easeInOut', delay: i * 0.12 }
      }
    >
      <svg width="34" height="48" viewBox="0 0 34 48" fill="none">
        <circle cx="17" cy="10" r="9" fill={color} />
        <rect x="11" y="20" width="12" height="20" rx="6" fill={color} />
        <rect x="6" y="22" width="6" height="14" rx="3" fill={color} opacity="0.85" />
        <rect x="22" y="22" width="6" height="14" rx="3" fill={color} opacity="0.85" />
      </svg>
    </motion.div>
  )
}

/** The snow cat that slides across and "eats" the dancers. */
function SnowCat({ go }: { go: boolean }) {
  return (
    <motion.div
      className="absolute left-0 top-1/2 -translate-y-1/2"
      initial={{ x: -160, opacity: 0 }}
      animate={go ? { x: 360, opacity: 1 } : { x: -160, opacity: 0 }}
      transition={{ duration: 1.6, ease: [0.4, 0, 0.2, 1] }}
    >
      <motion.div
        animate={go ? { rotate: [0, -6, 0, -6, 0] } : {}}
        transition={{ duration: 0.4, repeat: Infinity }}
      >
        <svg width="76" height="60" viewBox="0 0 76 60" fill="none">
          {/* body */}
          <ellipse cx="40" cy="38" rx="28" ry="18" fill="#fafafa" />
          {/* head */}
          <circle cx="22" cy="30" r="18" fill="#ffffff" />
          {/* ears */}
          <path d="M8 18 L14 30 L2 30 Z" fill="#ffffff" />
          <path d="M36 18 L30 30 L42 30 Z" fill="#ffffff" />
          <path d="M9 21 L12 28 L6 28 Z" fill="#fbcfe8" />
          <path d="M35 21 L32 28 L38 28 Z" fill="#fbcfe8" />
          {/* tail */}
          <path
            d="M66 40 Q80 34 72 20"
            stroke="#ffffff"
            strokeWidth="8"
            strokeLinecap="round"
            fill="none"
          />
          {/* eyes */}
          <circle cx="16" cy="28" r="2.4" fill="#1f2937" />
          <circle cx="28" cy="28" r="2.4" fill="#1f2937" />
          {/* mouth (open, eating) */}
          <path d="M16 36 Q22 44 28 36 Z" fill="#fb7185" />
          {/* whiskers */}
          <path d="M2 32 H12 M2 36 H12" stroke="#cbd5e1" strokeWidth="1.2" />
        </svg>
      </motion.div>
    </motion.div>
  )
}

export function LoadingScreen({ show, eating }: { show: boolean; eating: boolean }) {
  return (
    <AnimatePresence>
      {show && (
        <motion.div
          className="fixed inset-0 z-50 grid place-items-center"
          style={{ background: 'rgb(var(--bg))' }}
          initial={{ opacity: 1 }}
          exit={{ opacity: 0, filter: 'blur(8px)' }}
          transition={{ duration: 0.6, ease: [0.22, 1, 0.36, 1] }}
        >
          {/* soft color wash behind */}
          <div
            className="pointer-events-none absolute inset-0 opacity-60 blur-[80px]"
            style={{
              background:
                'radial-gradient(40vmax 40vmax at 30% 40%, rgb(99 102 241 / 0.4), transparent), radial-gradient(36vmax 36vmax at 70% 60%, rgb(244 114 182 / 0.35), transparent)',
            }}
          />

          <div className="relative flex flex-col items-center gap-10">
            <div className="relative flex h-[80px] w-[360px] items-end justify-center gap-4">
              {FIGURES.map((f, i) => (
                <Dancer key={i} color={f.c} i={i} eaten={eating} />
              ))}
              <SnowCat go={eating} />
            </div>

            <div className="flex flex-col items-center gap-3">
              <div className="flex items-center gap-2.5">
                <motion.img
                  src={logo}
                  alt="NADSPDF"
                  className="h-9 w-9 rounded-[9px] shadow-lg"
                  animate={{ y: [0, -4, 0] }}
                  transition={{ duration: 1.4, repeat: Infinity, ease: 'easeInOut' }}
                />
                <span className="text-[17px] font-semibold tracking-tight">NADSPDF</span>
              </div>
              <motion.p
                key={eating ? 'done' : 'load'}
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                className="text-sm text-muted"
              >
                {eating ? 'Le chat des neiges fait le ménage…' : 'Préparation de l’atelier…'}
              </motion.p>
            </div>
          </div>
        </motion.div>
      )}
    </AnimatePresence>
  )
}
