import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import App from './App.tsx'
import './index.css'
import { startErrorLogging } from './lib/errors'
import { blockWindowMenu } from './lib/windowMenu'

startErrorLogging()
blockWindowMenu()

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <App />
  </StrictMode>,
)
