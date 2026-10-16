import { useEffect, useRef } from 'react'
import { useMatch } from 'react-router-dom'
import { useQueryClient } from '@tanstack/react-query'
import { sse, type SseEventName } from '@/lib/api/sse'
import type { TaskQueueStatus } from '@/lib/api/types'
import { useAuthStore } from '@/lib/store/auth'
import { useTaskQueue } from '@/lib/store/taskQueue'
import { useThumbnailBust } from '@/lib/store/thumbnails'

// a library change can add/remove whole content trees, so it cascades everywhere
const ALL_FAMILIES = ['libraries', 'series', 'books', 'dashboard', 'collections', 'readlists']

const INVALIDATE: Partial<Record<SseEventName, string[]>> = {
  LibraryAdded: ALL_FAMILIES,
  LibraryChanged: ALL_FAMILIES,
  LibraryDeleted: ALL_FAMILIES,
  SeriesAdded: ['series', 'dashboard'],
  SeriesChanged: ['series', 'dashboard'],
  SeriesDeleted: ['series', 'dashboard'],
  BookAdded: ['books', 'series', 'dashboard'],
  BookChanged: ['books', 'series', 'dashboard'],
  BookDeleted: ['books', 'series', 'dashboard'],
  ReadProgressChanged: ['books', 'series', 'dashboard', 'readlists', 'smart-lists'],
  ReadProgressDeleted: ['books', 'series', 'dashboard', 'readlists', 'smart-lists'],
  // series-level mark read/unread also moves the per-book progress shown inside read lists
  ReadProgressSeriesChanged: ['series', 'dashboard', 'readlists', 'smart-lists'],
  ReadProgressSeriesDeleted: ['series', 'dashboard', 'readlists', 'smart-lists'],
  CollectionAdded: ['collections'],
  CollectionChanged: ['collections'],
  CollectionDeleted: ['collections'],
  ReadListAdded: ['readlists'],
  ReadListChanged: ['readlists'],
  ReadListDeleted: ['readlists'],
  SmartListAdded: ['smart-lists'],
  SmartListChanged: ['smart-lists'],
  SmartListDeleted: ['smart-lists'],
}

const THUMB_EVENTS: Record<string, (d: { seriesId?: string; bookId?: string; collectionId?: string; readListId?: string; smartListId?: string }) => string | undefined> = {
  ThumbnailSeriesAdded: (d) => d.seriesId,
  ThumbnailSeriesDeleted: (d) => d.seriesId,
  ThumbnailBookAdded: (d) => d.bookId,
  ThumbnailBookDeleted: (d) => d.bookId,
  ThumbnailSeriesCollectionAdded: (d) => d.collectionId,
  ThumbnailSeriesCollectionDeleted: (d) => d.collectionId,
  ThumbnailReadListAdded: (d) => d.readListId,
  ThumbnailReadListDeleted: (d) => d.readListId,
  SmartListThumbnailChanged: (d) => d.smartListId,
}

/** Wires the SSE stream to react-query invalidation while authenticated. */
export function useSseWiring() {
  const status = useAuthStore((s) => s.status)
  const queryClient = useQueryClient()
  const clear = useAuthStore((s) => s.clear)
  const bump = useThumbnailBust((s) => s.bump)
  const setTaskStatus = useTaskQueue((s) => s.setStatus)
  const readerOpen = useMatch('/book/:bookId/read') !== null

  // refs so events deferred while the reader is open survive the effect re-run
  const pendingKeys = useRef(new Set<string>())
  const pendingThumbs = useRef(new Set<string>())

  useEffect(() => {
    if (status !== 'authenticated') {
      sse.disconnect()
      pendingKeys.current.clear()
      pendingThumbs.current.clear()
      return
    }
    sse.connect()

    // KMReader-style coalescing: scans emit one event per book/series, so each
    // event only re-arms a 5s debounce and the merged families flush once the
    // stream goes quiet. While the reader is open the flush is deferred until it
    // closes, so the session's own progress events don't refetch the reader's
    // queries mid-read.
    const FLUSH_MS = 5000
    let timer: ReturnType<typeof setTimeout> | undefined
    const flush = () => {
      timer = undefined
      for (const family of pendingKeys.current) void queryClient.invalidateQueries({ queryKey: [family] })
      pendingKeys.current.clear()
      for (const id of pendingThumbs.current) bump(id)
      pendingThumbs.current.clear()
    }
    const schedule = () => {
      clearTimeout(timer)
      timer = setTimeout(flush, FLUSH_MS)
    }

    const offs: Array<() => void> = []
    for (const [name, keys] of Object.entries(INVALIDATE)) {
      offs.push(
        sse.on(name as SseEventName, () => {
          for (const key of keys) pendingKeys.current.add(key)
          if (!readerOpen) schedule()
        }),
      )
    }
    for (const [name, pick] of Object.entries(THUMB_EVENTS)) {
      offs.push(
        sse.on(name as SseEventName, (data) => {
          const id = pick(data as Parameters<typeof pick>[0])
          if (id) {
            pendingThumbs.current.add(id)
            if (!readerOpen) schedule()
          }
        }),
      )
    }
    offs.push(
      sse.on('SessionExpired', () => {
        clear()
        queryClient.clear()
      }),
    )
    offs.push(
      sse.on('TaskQueueStatus', (data) => {
        setTaskStatus(data as TaskQueueStatus)
      }),
    )

    // reader closed with deferred events pending
    if (!readerOpen && (pendingKeys.current.size > 0 || pendingThumbs.current.size > 0)) flush()

    return () => {
      clearTimeout(timer)
      offs.forEach((off) => off())
    }
  }, [status, queryClient, clear, bump, setTaskStatus, readerOpen])
}
