import type { PdfApi } from '../electron/preload'

declare global {
  interface Window {
    pdf: PdfApi
  }
}

export {}
