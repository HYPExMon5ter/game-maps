/// <reference types="vitest/config" />
import tailwindcss from '@tailwindcss/vite'
import react from '@vitejs/plugin-react'
import { defineConfig } from 'vite'

// Tauri expects a fixed dev port and handles the console itself.
export default defineConfig({
  plugins: [react(), tailwindcss()],
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  build: {
    target: 'es2022',
    chunkSizeWarningLimit: 2000,
    // One page per window: the map overlay and the launcher.
    rolldownOptions: { input: { overlay: 'index.html', launcher: 'launcher.html' } },
  },
  worker: { format: 'es' },
  test: { environment: 'node' },
})
