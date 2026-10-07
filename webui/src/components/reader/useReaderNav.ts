import { useCallback, useEffect, useRef, useState } from 'react'
import { useNavigate, useSearchParams } from 'react-router-dom'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { booksApi } from '@/lib/api/books'
import { readlistsApi } from '@/lib/api/collections'
import { ApiError } from '@/lib/api/client'
import type { BookDto } from '@/lib/api/types'
import { useReaderSettings } from '@/lib/store/readerSettings'
import { showToast } from '@/lib/store/toast'

/** Navigation shared by the image and EPUB readers: sibling books, history exit, fullscreen. */
export function useReaderNav(bookId: string) {
  const { t } = useTranslation('reader')
  const navigate = useNavigate()
  const [searchParams] = useSearchParams()
  const context = searchParams.get('context')
  const contextId = searchParams.get('contextId')
  const incognito = searchParams.get('incognito') === 'true'

  const siblingQuery = (dir: 'previous' | 'next') => ({
    queryKey: ['books', 'sibling', dir, bookId, context, contextId],
    queryFn: (): Promise<BookDto | null> => {
      const request =
        context === 'READLIST' && contextId ? readlistsApi[dir](contextId, bookId) : booksApi[dir](bookId)
      return request.catch((e) => {
        if (e instanceof ApiError && e.status === 404) return null
        throw e
      })
    },
  })
  const siblingPrevious = useQuery(siblingQuery('previous')).data ?? null
  const siblingNext = useQuery(siblingQuery('next')).data ?? null

  const jumpArmed = useRef<'previous' | 'next' | null>(null)
  const jumpTimer = useRef<number | undefined>(undefined)
  useEffect(() => () => window.clearTimeout(jumpTimer.current), [])

  const [isFullscreen, setIsFullscreen] = useState(false)
  useEffect(() => {
    const onChange = () => setIsFullscreen(!!document.fullscreenElement)
    document.addEventListener('fullscreenchange', onChange)
    return () => document.removeEventListener('fullscreenchange', onChange)
  }, [])
  useEffect(() => {
    if (useReaderSettings.getState().alwaysFullscreen && !document.fullscreenElement) {
      document.documentElement.requestFullscreen().catch(() => {})
    }
  }, [])
  useEffect(
    () => () => {
      if (document.fullscreenElement) document.exitFullscreen().catch(() => {})
    },
    [],
  )
  const toggleFullscreen = useCallback(() => {
    if (document.fullscreenElement) document.exitFullscreen().catch(() => {})
    else document.documentElement.requestFullscreen().catch(() => {})
  }, [])
  const onAlwaysFullscreenChange = useCallback((on: boolean) => {
    useReaderSettings.getState().update({ alwaysFullscreen: on })
    if (on) document.documentElement.requestFullscreen().catch(() => {})
    else if (document.fullscreenElement) document.exitFullscreen().catch(() => {})
  }, [])

  // closing returns to whatever page opened the reader; the book page is only the fallback for direct loads
  const exitReader = useCallback(() => {
    if ((window.history.state?.idx ?? 0) > 0) navigate(-1)
    else navigate(`/book/${bookId}`)
  }, [navigate, bookId])

  // replace keeps the reading session a single history entry, so back from the book page never loops into the reader
  const goToBook = useCallback(() => navigate(`/book/${bookId}`, { replace: true }), [navigate, bookId])

  const contextQuery = useCallback(() => {
    const q = new URLSearchParams()
    if (context && contextId) {
      q.set('context', context)
      q.set('contextId', contextId)
    }
    if (incognito) q.set('incognito', 'true')
    const s = q.toString()
    return s ? `?${s}` : ''
  }, [context, contextId, incognito])

  const goBook = useCallback(
    (dir: 'previous' | 'next') => {
      const sibling = dir === 'next' ? siblingNext : siblingPrevious
      // replace keeps the whole reading session as one history entry, so closing still returns to the page that opened the reader
      if (sibling) navigate(`/book/${sibling.id}/read${contextQuery()}`, { replace: true })
      else exitReader()
    },
    [siblingNext, siblingPrevious, navigate, contextQuery, exitReader],
  )

  // first boundary turn arms the jump, a second one within the toast window commits it
  const boundaryTurn = useCallback(
    (dir: 'previous' | 'next') => {
      if (jumpArmed.current === dir) {
        window.clearTimeout(jumpTimer.current)
        jumpArmed.current = null
        goBook(dir)
        return
      }
      jumpArmed.current = dir
      const hasSibling = dir === 'next' ? !!siblingNext : !!siblingPrevious
      showToast(
        t(
          dir === 'next'
            ? hasSibling
              ? 'toast.boundary.lastNext'
              : 'toast.boundary.lastExit'
            : hasSibling
              ? 'toast.boundary.firstPrevious'
              : 'toast.boundary.firstExit',
        ),
      )
      jumpTimer.current = window.setTimeout(() => (jumpArmed.current = null), 3000)
    },
    [siblingNext, siblingPrevious, goBook, t],
  )

  return {
    context,
    contextId,
    incognito,
    siblingPrevious,
    siblingNext,
    isFullscreen,
    toggleFullscreen,
    onAlwaysFullscreenChange,
    exitReader,
    goToBook,
    goBook,
    boundaryTurn,
  }
}
