self.addEventListener('activate', (event) => {
  event.waitUntil(caches.delete('kmweb-shell-v1'))
})
