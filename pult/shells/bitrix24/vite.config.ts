import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// SPA-оболочка Битрикс24 (Story 1.1). Бюджет критического пути —
// < 200 КБ gzip (AD-30), гейт CI: platform/scripts/bundle-size.mjs.
export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    // dev-режим открывается и вне iframe Б24 (локальная отладка).
    host: true,
  },
  build: {
    sourcemap: false,
  },
})
