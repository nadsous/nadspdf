import {
  FileText,
  FileSpreadsheet,
  Presentation,
  FileType2,
  ImageIcon,
  FileCode2,
  File,
  type LucideIcon,
} from 'lucide-react'

export type FileVisual = {
  icon: LucideIcon
  /** gradient stops for the icon tile */
  from: string
  to: string
  label: string
}

const MAP: Record<string, FileVisual> = {
  pdf: { icon: FileType2, from: '#f43f5e', to: '#e11d48', label: 'PDF' },
  doc: { icon: FileText, from: '#3b82f6', to: '#2563eb', label: 'Word' },
  docx: { icon: FileText, from: '#3b82f6', to: '#2563eb', label: 'Word' },
  rtf: { icon: FileText, from: '#3b82f6', to: '#2563eb', label: 'RTF' },
  txt: { icon: FileText, from: '#64748b', to: '#475569', label: 'Texte' },
  odt: { icon: FileText, from: '#0ea5e9', to: '#0284c7', label: 'ODT' },
  xls: { icon: FileSpreadsheet, from: '#22c55e', to: '#16a34a', label: 'Excel' },
  xlsx: { icon: FileSpreadsheet, from: '#22c55e', to: '#16a34a', label: 'Excel' },
  ods: { icon: FileSpreadsheet, from: '#10b981', to: '#059669', label: 'ODS' },
  ppt: { icon: Presentation, from: '#f97316', to: '#ea580c', label: 'PowerPoint' },
  pptx: { icon: Presentation, from: '#f97316', to: '#ea580c', label: 'PowerPoint' },
  odp: { icon: Presentation, from: '#fb923c', to: '#f97316', label: 'ODP' },
  png: { icon: ImageIcon, from: '#a855f7', to: '#7c3aed', label: 'PNG' },
  jpg: { icon: ImageIcon, from: '#ec4899', to: '#db2777', label: 'JPG' },
  jpeg: { icon: ImageIcon, from: '#ec4899', to: '#db2777', label: 'JPG' },
  html: { icon: FileCode2, from: '#eab308', to: '#ca8a04', label: 'HTML' },
}

export function fileVisual(ext: string): FileVisual {
  return MAP[ext.toLowerCase()] ?? { icon: File, from: '#94a3b8', to: '#64748b', label: ext.toUpperCase() }
}
