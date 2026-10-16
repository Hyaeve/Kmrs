// SSE event stream (/sse/v1/events). EventSource auto-reconnects; callers
// subscribe per event name. SessionExpired must force a re-login.

export const SSE_EVENTS = [
  'LibraryAdded',
  'LibraryChanged',
  'LibraryDeleted',
  'SeriesAdded',
  'SeriesChanged',
  'SeriesDeleted',
  'BookAdded',
  'BookChanged',
  'BookDeleted',
  'ReadListAdded',
  'ReadListChanged',
  'ReadListDeleted',
  'SmartListAdded',
  'SmartListChanged',
  'SmartListDeleted',
  'SmartListThumbnailChanged',
  'CollectionAdded',
  'CollectionChanged',
  'CollectionDeleted',
  'ReadProgressChanged',
  'ReadProgressDeleted',
  'ReadProgressSeriesChanged',
  'ReadProgressSeriesDeleted',
  'ThumbnailBookAdded',
  'ThumbnailBookDeleted',
  'ThumbnailSeriesAdded',
  'ThumbnailSeriesDeleted',
  'ThumbnailSeriesCollectionAdded',
  'ThumbnailSeriesCollectionDeleted',
  'ThumbnailReadListAdded',
  'ThumbnailReadListDeleted',
  'SessionExpired',
  'TaskQueueStatus',
] as const

export type SseEventName = (typeof SSE_EVENTS)[number]
export type SseHandler = (data: unknown) => void

class SseClient {
  private source: EventSource | null = null
  private handlers = new Map<SseEventName, Set<SseHandler>>()
  // one EventSource listener per event name; it dispatches to whatever the
  // handler set holds at fire time, so (un)subscribing never touches the source
  private attached = new Set<SseEventName>()

  connect() {
    if (this.source) return
    this.source = new EventSource('/sse/v1/events')
    for (const name of this.handlers.keys()) this.attach(name)
  }

  disconnect() {
    this.source?.close()
    this.source = null
    this.attached.clear()
  }

  on(name: SseEventName, handler: SseHandler): () => void {
    if (!this.handlers.has(name)) this.handlers.set(name, new Set())
    this.handlers.get(name)!.add(handler)
    if (this.source) this.attach(name)
    return () => {
      const set = this.handlers.get(name)
      if (!set) return
      set.delete(handler)
      if (set.size === 0) this.handlers.delete(name)
    }
  }

  private attach(name: SseEventName) {
    if (!this.source || this.attached.has(name)) return
    this.attached.add(name)
    this.source.addEventListener(name, (ev) => {
      let data: unknown = null
      try {
        data = JSON.parse((ev as MessageEvent).data)
      } catch {
        // heartbeat comments arrive without data
      }
      this.handlers.get(name)?.forEach((h) => h(data))
    })
  }
}

export const sse = new SseClient()
