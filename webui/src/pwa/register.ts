import { registerSW } from 'virtual:pwa-register'
import { usePwaStore } from '@/lib/store/pwa'

export function registerPwa() {
  const updateServiceWorker = registerSW({
    onNeedRefresh: () => usePwaStore.getState().showUpdate(updateServiceWorker),
  })
}
