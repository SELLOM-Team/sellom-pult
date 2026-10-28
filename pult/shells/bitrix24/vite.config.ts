import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'

// SPA-оболочка Битрикс24 (Story 1.1 — каркас; 1.2 — шаг 0). Бюджет
// критического пути — < 200 КБ gzip (AD-30), гейт CI: platform/scripts/bundle-size.mjs.
export default defineConfig({
  plugins: [react()],
  server: {
    port: 5173,
    // dev-режим открывается и вне iframe Б24 (локальная отладка).
    host: true,
    // /health, /ready и контракты /v1 — на pult-gateway: без прокси они
    // попадают в SPA-fallback Vite (index.html, 200) и клиент API принял бы
    // это за живой шлюз.
    proxy: {
      '/health': 'http://localhost:8080',
      '/ready': 'http://localhost:8080',
      '/v1': 'http://localhost:8080',
    },
  },
  build: {
    sourcemap: false,
  },
})
