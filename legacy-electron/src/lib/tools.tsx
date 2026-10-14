import {
  FileOutput,
  Minimize2,
  Combine,
  Scissors,
  RotateCw,
  Images,
  FileImage,
} from 'lucide-react'
import type { ToolId } from '../store/useStore'

export type ToolDef = {
  id: ToolId
  label: string
  short: string
  desc: string
  icon: typeof Combine
  /** gradient stops (css vars) for the tool's accent */
  grad: [string, string]
  accept: string[]
  multiple: boolean
}

export const TOOLS: ToolDef[] = [
  {
    id: 'convert',
    label: 'Convertir en PDF',
    short: 'Convertir',
    desc: 'Word, Excel, CSV, texte et HTML transformés en PDF, sans logiciel externe.',
    icon: FileOutput,
    grad: ['var(--g1)', 'var(--g2)'],
    accept: ['docx', 'xlsx', 'xls', 'ods', 'csv', 'txt', 'rtf', 'html', 'htm'],
    multiple: true,
  },
  {
    id: 'compress',
    label: 'Compresser',
    short: 'Compresser',
    desc: 'Réduisez le poids de vos PDF en trois niveaux de qualité.',
    icon: Minimize2,
    grad: ['var(--g4)', 'var(--g2)'],
    accept: ['pdf'],
    multiple: false,
  },
  {
    id: 'merge',
    label: 'Fusionner',
    short: 'Fusionner',
    desc: 'Combinez plusieurs PDF en un seul document, dans votre ordre.',
    icon: Combine,
    grad: ['var(--g1)', 'var(--g3)'],
    accept: ['pdf'],
    multiple: true,
  },
  {
    id: 'split',
    label: 'Diviser',
    short: 'Diviser',
    desc: 'Extrayez des pages précises ou scindez un document.',
    icon: Scissors,
    grad: ['var(--g3)', 'var(--g1)'],
    accept: ['pdf'],
    multiple: false,
  },
  {
    id: 'rotate',
    label: 'Pivoter',
    short: 'Pivoter',
    desc: 'Faites pivoter toutes les pages de 90°, 180° ou 270°.',
    icon: RotateCw,
    grad: ['var(--g2)', 'var(--g4)'],
    accept: ['pdf'],
    multiple: false,
  },
  {
    id: 'pdf-to-images',
    label: 'PDF → Images',
    short: 'PDF → Images',
    desc: 'Exportez chaque page en PNG haute définition.',
    icon: Images,
    grad: ['var(--g3)', 'var(--g4)'],
    accept: ['pdf'],
    multiple: false,
  },
  {
    id: 'images-to-pdf',
    label: 'Images → PDF',
    short: 'Images → PDF',
    desc: 'Assemblez vos images PNG et JPG en un seul PDF.',
    icon: FileImage,
    grad: ['var(--g4)', 'var(--g1)'],
    accept: ['png', 'jpg', 'jpeg'],
    multiple: true,
  },
]

export const toolById = (id: ToolId) => TOOLS.find((t) => t.id === id)!
