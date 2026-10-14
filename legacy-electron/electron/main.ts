import { app, BrowserWindow, ipcMain, dialog, session } from 'electron'
import path from 'node:path'
import fs from 'node:fs'
import { registerHandlers } from './handlers'

process.env.APP_ROOT = path.join(__dirname, '..')
const VITE_DEV_SERVER_URL = process.env['VITE_DEV_SERVER_URL']
const RENDERER_DIST = path.join(process.env.APP_ROOT, 'dist')

let win: BrowserWindow | null = null

type WinState = { width: number; height: number; x?: number; y?: number; maximized?: boolean }
const stateFile = () => path.join(app.getPath('userData'), 'window-state.json')

function loadWinState(): WinState {
  try {
    const s = JSON.parse(fs.readFileSync(stateFile(), 'utf-8'))
    if (typeof s.width === 'number' && typeof s.height === 'number') return s
  } catch {
    /* first run / unreadable */
  }
  return { width: 1240, height: 820 }
}

function saveWinState() {
  if (!win) return
  try {
    const maximized = win.isMaximized()
    const b = win.getNormalBounds()
    const state: WinState = { width: b.width, height: b.height, x: b.x, y: b.y, maximized }
    fs.writeFileSync(stateFile(), JSON.stringify(state))
  } catch {
    /* ignore */
  }
}

function createWindow() {
  const st = loadWinState()
  win = new BrowserWindow({
    width: st.width,
    height: st.height,
    x: st.x,
    y: st.y,
    minWidth: 940,
    minHeight: 640,
    titleBarStyle: 'hiddenInset',
    titleBarOverlay: false,
    frame: process.platform === 'darwin',
    backgroundColor: '#0a0a0c',
    show: false,
    webPreferences: {
      preload: path.join(__dirname, 'preload.js'),
      contextIsolation: true,
      nodeIntegration: false,
    },
  })

  if (st.maximized) win.maximize()

  win.once('ready-to-show', () => win?.show())
  // persist size/position (debounced) and on close
  let t: NodeJS.Timeout | null = null
  const schedule = () => {
    if (t) clearTimeout(t)
    t = setTimeout(saveWinState, 400)
  }
  win.on('resize', schedule)
  win.on('move', schedule)
  win.on('close', saveWinState)

  if (VITE_DEV_SERVER_URL) {
    win.loadURL(VITE_DEV_SERVER_URL)
  } else {
    win.loadFile(path.join(RENDERER_DIST, 'index.html'))
  }
}

// Window controls (custom frameless chrome on Windows)
ipcMain.handle('window:minimize', () => win?.minimize())
ipcMain.handle('window:maximize', () => {
  if (!win) return
  win.isMaximized() ? win.unmaximize() : win.maximize()
})
ipcMain.handle('window:close', () => win?.close())
ipcMain.handle('dialog:openFiles', async (_e, filters?: Electron.FileFilter[]) => {
  if (!win) return []
  const res = await dialog.showOpenDialog(win, {
    properties: ['openFile', 'multiSelections'],
    filters,
  })
  return res.canceled ? [] : res.filePaths
})
ipcMain.handle('dialog:saveFile', async (_e, defaultName: string, filters?: Electron.FileFilter[]) => {
  if (!win) return null
  const res = await dialog.showSaveDialog(win, { defaultPath: defaultName, filters })
  return res.canceled ? null : res.filePath
})
ipcMain.handle('dialog:openDir', async () => {
  if (!win) return null
  const res = await dialog.showOpenDialog(win, { properties: ['openDirectory'] })
  return res.canceled ? null : res.filePaths[0]
})

app.whenReady().then(() => {
  // allow the Local Font Access API (queryLocalFonts) used for PDF text editing
  session.defaultSession.setPermissionRequestHandler((_wc, permission, cb) => {
    cb(String(permission) === 'local-fonts')
  })
  session.defaultSession.setPermissionCheckHandler(
    (_wc, permission) => String(permission) === 'local-fonts',
  )

  registerHandlers(ipcMain)
  createWindow()
  app.on('activate', () => {
    if (BrowserWindow.getAllWindows().length === 0) createWindow()
  })
})

app.on('window-all-closed', () => {
  if (process.platform !== 'darwin') {
    app.quit()
    win = null
  }
})
