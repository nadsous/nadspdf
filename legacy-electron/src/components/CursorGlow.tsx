import { useEffect, useRef } from 'react'

/**
 * A large, soft specular highlight that lags behind the cursor with spring
 * easing — the global "reflet" that sweeps across the whole window.
 */
export function CursorGlow() {
  const ref = useRef<HTMLDivElement>(null)
  const pos = useRef({ x: window.innerWidth / 2, y: window.innerHeight / 2 })
  const target = useRef({ x: window.innerWidth / 2, y: window.innerHeight / 2 })
  const raf = useRef(0)

  useEffect(() => {
    const onMove = (e: PointerEvent) => {
      target.current = { x: e.clientX, y: e.clientY }
    }
    window.addEventListener('pointermove', onMove)

    const tick = () => {
      pos.current.x += (target.current.x - pos.current.x) * 0.12
      pos.current.y += (target.current.y - pos.current.y) * 0.12
      if (ref.current) {
        ref.current.style.transform = `translate3d(${pos.current.x - 300}px, ${
          pos.current.y - 300
        }px, 0)`
      }
      raf.current = requestAnimationFrame(tick)
    }
    raf.current = requestAnimationFrame(tick)
    return () => {
      window.removeEventListener('pointermove', onMove)
      cancelAnimationFrame(raf.current)
    }
  }, [])

  return (
    <div
      ref={ref}
      className="pointer-events-none fixed left-0 top-0 z-0 h-[600px] w-[600px] rounded-full blur-[60px] will-change-transform"
      style={{
        background:
          'radial-gradient(circle, rgb(var(--accent) / 0.16), transparent 60%)',
        mixBlendMode: 'screen',
      }}
    />
  )
}
