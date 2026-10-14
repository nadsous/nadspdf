import { forwardRef, useCallback, type HTMLAttributes } from 'react'
import { cn } from '../lib/utils'

type Props = HTMLAttributes<HTMLDivElement> & {
  /** enable the cursor-tracking specular reflection */
  reflect?: boolean
  /** animated rim light */
  rim?: boolean
}

/**
 * Frosted glass surface. When `reflect` is on it tracks the pointer and lights
 * a soft highlight under the cursor (drives the --mx/--my CSS vars).
 */
export const GlassCard = forwardRef<HTMLDivElement, Props>(
  ({ className, reflect = true, rim = false, children, ...rest }, ref) => {
    const onMove = useCallback((e: React.PointerEvent<HTMLDivElement>) => {
      const el = e.currentTarget
      const r = el.getBoundingClientRect()
      el.style.setProperty('--mx', `${e.clientX - r.left}px`)
      el.style.setProperty('--my', `${e.clientY - r.top}px`)
    }, [])

    return (
      <div
        ref={ref}
        onPointerMove={reflect ? onMove : undefined}
        className={cn('glass rounded-2xl', reflect && 'reflect', rim && 'rim', className)}
        {...rest}
      >
        {children}
      </div>
    )
  },
)
GlassCard.displayName = 'GlassCard'
