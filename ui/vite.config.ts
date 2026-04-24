import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import path from 'path'

// API base URL for the dev proxy. The default of "" in api/client.ts means
// "same origin" — fine for production (UI + API share the origin), but in
// dev the UI runs on :5173 and the API on :4000, so we need to forward the
// API paths. Override via CRONIQ_API_ORIGIN if you run the server on a
// different host/port while developing the UI.
const API_ORIGIN = process.env.CRONIQ_API_ORIGIN ?? 'http://localhost:4000'

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: {
      '@': path.resolve(__dirname, './src'),
    },
  },
  server: {
    proxy: {
      '/v1': API_ORIGIN,
      '/health': API_ORIGIN,
      '/metrics': API_ORIGIN,
    },
  },
})
