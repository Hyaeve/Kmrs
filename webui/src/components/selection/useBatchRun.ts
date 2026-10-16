import { useCallback, useRef, useState } from 'react'
import i18n from '@/lib/i18n'

export type BatchStatus = 'idle' | 'running' | 'success' | 'error'

export interface BatchRunState {
  status: BatchStatus
  progress: { done: number; total: number } | null
  message: string | null
}

const IDLE: BatchRunState = { status: 'idle', progress: null, message: null }

export interface BatchMessage {
  /** i18n key; resolved with count=total on success, count=total plus failed on failure */
  key: string
  context?: string
}

export interface BatchMessages {
  success: BatchMessage
  failure: BatchMessage
}

export function useBatchRun() {
  const [state, setState] = useState<BatchRunState>(IDLE)
  const running = useRef(false)

  /** Runs fn per id sequentially so the server is not hammered; returns the failure count. */
  const run = useCallback(async (ids: readonly string[], fn: (id: string) => Promise<unknown>, messages: BatchMessages) => {
    if (running.current) return ids.length
    running.current = true
    const total = ids.length
    let failed = 0
    setState({ status: 'running', progress: { done: 0, total }, message: null })
    for (let i = 0; i < ids.length; i++) {
      try {
        await fn(ids[i])
      } catch {
        failed++
      }
      setState({ status: 'running', progress: { done: i + 1, total }, message: null })
    }
    running.current = false
    if (failed === 0)
      setState({
        status: 'success',
        progress: null,
        message: i18n.t(messages.success.key, { count: total, context: messages.success.context }),
      })
    else
      setState({
        status: 'error',
        progress: null,
        message: i18n.t(messages.failure.key, { count: total, failed, total }),
      })
    return failed
  }, [])

  /** Result for one-shot operations (dialogs) that bypass the loop. */
  const setResult = useCallback((status: 'success' | 'error', message: string) => {
    setState({ status, progress: null, message })
  }, [])

  const reset = useCallback(() => setState(IDLE), [])

  return { state, run, setResult, reset }
}
