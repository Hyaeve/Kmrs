import { lazy } from 'react'

// lazy-isolated so the router module exports no components (react-refresh rule)
export const ReaderPage = lazy(() => import('@/routes/read').then((m) => ({ default: m.ReaderPage })))
