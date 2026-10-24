import '@fontsource-variable/geist'
import '@fontsource-variable/geist-mono'
import '@fontsource-variable/newsreader'
import './index.css'
import './lib/i18n'

import { StrictMode } from 'react'
import { createRoot } from 'react-dom/client'
import { RouterProvider } from 'react-router-dom'
import { QueryClientProvider } from '@tanstack/react-query'
import { MotionConfig } from 'motion/react'
import { router } from './router'
import { queryClient } from './lib/queryClient'
import { Toaster } from './components/Toaster'
import { TooltipProvider } from './components/ui/Tooltip'
import { KomfJobsPanel } from './components/KomfJobsPanel'
import { PwaUpdatePrompt } from './components/PwaUpdatePrompt'
import { applyTheme, useUiStore } from './lib/store/ui'

applyTheme(useUiStore.getState().theme)
useUiStore.subscribe((s) => applyTheme(s.theme))

if ((import.meta.env.PROD || import.meta.env.MODE === 'pwa') && 'serviceWorker' in navigator) {
  void import('./pwa/register').then(({ registerPwa }) => registerPwa())
}
window.matchMedia('(prefers-color-scheme: light)').addEventListener('change', () => {
  if (useUiStore.getState().theme === 'system') applyTheme('system')
})

createRoot(document.getElementById('root')!).render(
  <StrictMode>
    <QueryClientProvider client={queryClient}>
      <TooltipProvider>
        <MotionConfig reducedMotion="user">
          <RouterProvider router={router} />
          <Toaster />
          <KomfJobsPanel />
          <PwaUpdatePrompt />
        </MotionConfig>
      </TooltipProvider>
    </QueryClientProvider>
  </StrictMode>,
)
