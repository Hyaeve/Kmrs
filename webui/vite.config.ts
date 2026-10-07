import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'

export default defineConfig({
  plugins: [react(), tailwindcss()],
  resolve: {
    alias: { '@': '/src' },
  },
  server: {
    port: 5173,
    proxy: {
      '/api': { target: 'http://localhost:25600', changeOrigin: false },
      '/sse': { target: 'http://localhost:25600', changeOrigin: false },
      '/actuator': { target: 'http://localhost:25600', changeOrigin: false },
    },
  },
  preview: {
    proxy: {
      '/api': { target: 'http://localhost:25600', changeOrigin: false },
      '/sse': { target: 'http://localhost:25600', changeOrigin: false },
      '/actuator': { target: 'http://localhost:25600', changeOrigin: false },
    },
  },
  build: {
    outDir: 'dist',
    chunkSizeWarningLimit: 1200,
  },
})
