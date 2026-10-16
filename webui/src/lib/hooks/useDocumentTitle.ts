import { useEffect } from 'react'

// title should come from t() (or a dynamic entity name); the effect re-runs
// when a language switch re-renders the caller with a new string
export function useDocumentTitle(title?: string) {
  useEffect(() => {
    document.title = title ? `${title} · KMReader` : 'KMReader'
  }, [title])
}
