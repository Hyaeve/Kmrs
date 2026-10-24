import { defineConfig } from 'vite'
import react from '@vitejs/plugin-react'
import tailwindcss from '@tailwindcss/vite'
import { VitePWA } from 'vite-plugin-pwa'

export default defineConfig(({ mode }) => ({
  plugins: [
    react(),
    tailwindcss(),
    VitePWA({
      strategies: 'generateSW',
      registerType: 'prompt',
      includeAssets: ['favicon.svg', 'apple-touch-icon.png'],
      manifest: {
        name: 'KMReader',
        short_name: 'KMReader',
        description: 'A reader for your Komga library',
        start_url: '/',
        scope: '/',
        display: 'standalone',
        background_color: '#ffffff',
        theme_color: '#0b0b0d',
        icons: [
          { src: '/pwa-192.png', sizes: '192x192', type: 'image/png' },
          { src: '/pwa-512.png', sizes: '512x512', type: 'image/png' },
          { src: '/pwa-maskable-192.png', sizes: '192x192', type: 'image/png', purpose: 'maskable' },
          { src: '/pwa-maskable-512.png', sizes: '512x512', type: 'image/png', purpose: 'maskable' },
          { src: '/pwa-monochrome-512.png', sizes: '512x512', type: 'image/png', purpose: 'monochrome' },
        ],
      },
      workbox: {
        globPatterns: ['**/*.{js,css,html,woff2}'],
        maximumFileSizeToCacheInBytes: 4 * 1024 * 1024,
        cleanupOutdatedCaches: true,
        importScripts: ['sw-cleanup.js'],
        navigateFallback: 'index.html',
        navigateFallbackDenylist: [
          /^\/api(?:\/|$)/,
          /^\/opds(?:\/|$)/,
          /^\/sse(?:\/|$)/,
          /^\/oauth2(?:\/|$)/,
          /^\/actuator(?:\/|$)/,
          /^\/kobo(?:\/|$)/,
          /^\/koreader(?:\/|$)/,
          /^\/v3(?:\/|$)/,
          /^\/debug(?:\/|$)/,
          /^\/login\/oauth2(?:\/|$)/,
        ],
      },
      devOptions: {
        enabled: mode === 'pwa',
      },
    }),
  ],
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
}))
