/** Parse "1-3,5" into a Set of 1-based page numbers, clamped to [1,count]. */
export function rangesToSet(spec: string, count: number): Set<number> {
  const set = new Set<number>()
  const clean = spec.trim().toLowerCase()
  if (!clean || clean === 'all') {
    for (let i = 1; i <= count; i++) set.add(i)
    return set
  }
  for (const part of clean.split(',')) {
    const token = part.trim()
    if (!token) continue
    if (token.includes('-')) {
      const [a, b] = token.split('-').map((x) => parseInt(x.trim(), 10))
      const lo = Math.max(1, Math.min(a || 1, b || count))
      const hi = Math.min(count, Math.max(a || 1, b || count))
      for (let i = lo; i <= hi; i++) set.add(i)
    } else {
      const n = parseInt(token, 10)
      if (n >= 1 && n <= count) set.add(n)
    }
  }
  return set
}

/** Compress a Set of page numbers into "1-3,5,8-10". Returns "all" if full. */
export function setToRanges(set: Set<number>, count: number): string {
  const pages = [...set].sort((a, b) => a - b)
  if (pages.length === 0) return ''
  if (pages.length === count) return 'all'
  const out: string[] = []
  let start = pages[0]
  let prev = pages[0]
  for (let i = 1; i <= pages.length; i++) {
    const cur = pages[i]
    if (cur === prev + 1) {
      prev = cur
      continue
    }
    out.push(start === prev ? `${start}` : `${start}-${prev}`)
    start = cur
    prev = cur
  }
  return out.join(',')
}
