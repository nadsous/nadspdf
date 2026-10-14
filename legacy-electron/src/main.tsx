import React from 'react'
import ReactDOM from 'react-dom/client'
import App from './App'
import { useStore } from './store/useStore'
import './index.css'

// apply the persisted/default theme before first paint
document.documentElement.dataset.theme = useStore.getState().theme

ReactDOM.createRoot(document.getElementById('root')!).render(
  <React.StrictMode>
    <App />
  </React.StrictMode>,
)
