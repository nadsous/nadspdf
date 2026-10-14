/**
 * Animated grainy gradient-mesh backdrop.
 * Four blurred color blobs drift slowly; the grain overlay comes from `.grain`.
 */
export function GrainGradient() {
  return (
    <div className="pointer-events-none fixed inset-0 z-0 overflow-hidden">
      <div className="absolute inset-0" style={{ background: 'rgb(var(--bg))' }} />

      <div
        className="absolute -left-[10%] -top-[15%] h-[55vmax] w-[55vmax] rounded-full blur-[90px] animate-float-slow"
        style={{
          background:
            'radial-gradient(circle at 30% 30%, rgb(var(--g1) / 0.55), transparent 60%)',
        }}
      />
      <div
        className="absolute -right-[12%] top-[5%] h-[48vmax] w-[48vmax] rounded-full blur-[100px] animate-float-slow-2"
        style={{
          background:
            'radial-gradient(circle at 60% 40%, rgb(var(--g2) / 0.45), transparent 60%)',
        }}
      />
      <div
        className="absolute bottom-[-18%] left-[15%] h-[50vmax] w-[50vmax] rounded-full blur-[100px] animate-float-slow"
        style={{
          background:
            'radial-gradient(circle at 50% 50%, rgb(var(--g3) / 0.4), transparent 62%)',
        }}
      />
      <div
        className="absolute bottom-[-10%] right-[8%] h-[42vmax] w-[42vmax] rounded-full blur-[110px] animate-float-slow-2"
        style={{
          background:
            'radial-gradient(circle at 40% 60%, rgb(var(--g4) / 0.4), transparent 60%)',
        }}
      />

      {/* vignette to ground the corners */}
      <div
        className="absolute inset-0"
        style={{
          background:
            'radial-gradient(120% 90% at 50% 0%, transparent 55%, rgb(var(--bg) / 0.7) 100%)',
        }}
      />
    </div>
  )
}
