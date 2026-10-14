import { Moon, Sun, Sparkles, Trees, Flower2, type LucideIcon } from 'lucide-react'
import type { Theme } from '../store/useStore'

export type ThemeDef = {
  id: Theme
  label: string
  hint: string
  /** whether the base is light (affects nothing structural, just for the picker) */
  light: boolean
  /** icon shown in the theme selector */
  icon: LucideIcon
  /** two rgb triplets used to draw the swatch in the picker */
  swatch: [string, string]
}

export const THEMES: ThemeDef[] = [
  { id: 'dark', label: 'Dark', hint: 'Nuit Apple', light: false, icon: Moon, swatch: ['99 102 241', '56 189 248'] },
  { id: 'light', label: 'Light', hint: 'Clair neutre', light: true, icon: Sun, swatch: ['129 140 248', '125 211 252'] },
  { id: 'loubina', label: 'Lou Bi Na', hint: 'Rose · violet, nuit', light: false, icon: Sparkles, swatch: ['139 92 246', '236 72 153'] },
  { id: 'yoyochill', label: 'YoYo Chill', hint: 'Forêt & myrtille', light: false, icon: Trees, swatch: ['16 185 129', '124 92 214'] },
  { id: 'loulinhan', label: 'Lou Lin Han', hint: 'Blanc washi, Japon', light: true, icon: Flower2, swatch: ['244 154 180', '63 94 148'] },
]

export const themeById = (id: Theme) => THEMES.find((t) => t.id === id) ?? THEMES[0]
