import { contextBridge, ipcRenderer } from 'electron'

const api = {
  // window chrome
  minimize: () => ipcRenderer.invoke('window:minimize'),
  maximize: () => ipcRenderer.invoke('window:maximize'),
  close: () => ipcRenderer.invoke('window:close'),

  // dialogs
  openFiles: (filters?: Electron.FileFilter[]): Promise<string[]> =>
    ipcRenderer.invoke('dialog:openFiles', filters),
  saveFile: (defaultName: string, filters?: Electron.FileFilter[]): Promise<string | null> =>
    ipcRenderer.invoke('dialog:saveFile', defaultName, filters),
  openDir: (): Promise<string | null> => ipcRenderer.invoke('dialog:openDir'),

  // pdf operations
  fileInfo: (paths: string[]): Promise<FileInfo[]> => ipcRenderer.invoke('pdf:fileInfo', paths),
  merge: (paths: string[], out: string): Promise<OpResult> =>
    ipcRenderer.invoke('pdf:merge', paths, out),
  split: (path: string, ranges: string, outDir: string): Promise<OpResult> =>
    ipcRenderer.invoke('pdf:split', path, ranges, outDir),
  rotate: (path: string, degrees: number, out: string): Promise<OpResult> =>
    ipcRenderer.invoke('pdf:rotate', path, degrees, out),
  imagesToPdf: (paths: string[], out: string): Promise<OpResult> =>
    ipcRenderer.invoke('pdf:imagesToPdf', paths, out),
  convert: (paths: string[], outDir: string): Promise<OpResult[]> =>
    ipcRenderer.invoke('pdf:convert', paths, outDir),
  convertBytes: (paths: string[]): Promise<{ ok: boolean; bytes?: Uint8Array; error?: string }[]> =>
    ipcRenderer.invoke('pdf:convertBytes', paths),
  readBytes: (path: string): Promise<Uint8Array> => ipcRenderer.invoke('fs:readBytes', path),
  writeBytes: (path: string, bytes: Uint8Array): Promise<OpResult> =>
    ipcRenderer.invoke('fs:writeBytes', path, bytes),
  revealInFolder: (path: string): Promise<void> => ipcRenderer.invoke('shell:reveal', path),
}

export type FileInfo = {
  path: string
  name: string
  ext: string
  size: number
  pages?: number
}

export type OpResult = {
  ok: boolean
  outPath?: string
  sizeBefore?: number
  sizeAfter?: number
  error?: string
}

export type PdfApi = typeof api

contextBridge.exposeInMainWorld('pdf', api)
