import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import '../index.css'
import { Launcher } from './Launcher'
import { startErrorLogging } from '../lib/errors'
import { ErrorBoundary } from '../ui/ErrorBoundary'

startErrorLogging()

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <ErrorBoundary>
      <Launcher />
    </ErrorBoundary>
  </StrictMode>,
)
