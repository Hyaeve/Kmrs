import { create } from 'zustand'
import { ApiError } from '@/lib/api/client'
import { komfApi } from '@/lib/api/komf'
import { seriesApi } from '@/lib/api/series'
import i18n from '@/lib/i18n'
import type { KomfJobEvent } from '@/lib/api/types'
import { showToast } from './toast'

export interface TrackedJob {
  id: string
  label: string
  text: string
  failed: string | null
  done: boolean
}

interface KomfJobsState {
  jobs: Record<string, TrackedJob>
  track: (jobId: string, label: string) => void
  trackMany: (entries: { id: string; label: string }[]) => void
  dismiss: (jobId: string) => void
}

// komf's provider enum values get readable names; unknown ones are title-cased
const PROVIDER_NAMES: Record<string, string> = {
  MANGA_BAKA: 'MangaBaka',
  BOOK_WALKER: 'BookWalker',
  MANGADEX: 'MangaDex',
  MANGA_UPDATES: 'MangaUpdates',
  MANGAUPDATES: 'MangaUpdates',
  ANILIST: 'AniList',
  MAL: 'MyAnimeList',
  COMIC_VINE: 'ComicVine',
  BANGUMI: 'Bangumi',
  YEN_PRESS: 'Yen Press',
  VIZ: 'VIZ',
  WEBTOONS: 'Webtoons',
  NAUTILJON: 'Nautiljon',
  KODANSHA: 'Kodansha',
  HENTAG: 'Hentag',
}

function providerName(raw: string): string {
  const known = PROVIDER_NAMES[raw]
  if (known) return known
  return raw
    .toLowerCase()
    .split(/[_\s]+/)
    .map((w) => w.charAt(0).toUpperCase() + w.slice(1))
    .join(' ')
}

// removal timers are not UI state, so they stay outside the store
const removeTimers = new Map<string, number>()

export const useKomfJobs = create<KomfJobsState>()((set, get) => ({
  jobs: {},

  track: (jobId, label) => {
    if (get().jobs[jobId]) return
    set((s) => ({
      jobs: { ...s.jobs, [jobId]: { id: jobId, label, text: i18n.t('komf.matching'), failed: null, done: false } },
    }))
  },

  trackMany: (entries) => {
    const fresh = entries.filter((e) => !get().jobs[e.id])
    if (fresh.length === 0) return
    const jobs = { ...get().jobs }
    for (const e of fresh) {
      jobs[e.id] = { id: e.id, label: e.label, text: i18n.t('komf.matching'), failed: null, done: false }
    }
    set({ jobs })
  },

  dismiss: (jobId) => {
    // a running job keeps coming back on the firehose (every reconnect replays it);
    // a finished one never reappears, so only running jobs need the suppression
    const job = get().jobs[jobId]
    if (job && !job.done) dismissedIds.add(jobId)
    window.clearTimeout(removeTimers.get(jobId))
    removeTimers.delete(jobId)
    removeJob(jobId)
  },
}))

export function trackKomfJob(jobId: string, label: string) {
  useKomfJobs.getState().track(jobId, label)
}

export function trackKomfJobs(entries: { id: string; label: string }[]) {
  useKomfJobs.getState().trackMany(entries)
}

function updateJob(jobId: string, patch: Partial<TrackedJob>) {
  useKomfJobs.setState((s) => {
    const job = s.jobs[jobId]
    if (!job) return s
    return { jobs: { ...s.jobs, [jobId]: { ...job, ...patch } } }
  })
}

function removeJob(jobId: string) {
  useKomfJobs.setState((s) => {
    if (!s.jobs[jobId]) return s
    const jobs = { ...s.jobs }
    delete jobs[jobId]
    return { jobs }
  })
}

function finishJob(jobId: string, failed: string | null) {
  const job = useKomfJobs.getState().jobs[jobId]
  if (!job || job.done) return
  updateJob(jobId, { done: true, failed })
  if (failed) {
    showToast(`${job.label}: ${failed}`)
  } else {
    showToast(`${job.label}: ${i18n.t('komf.matchCompleted')}`)
    removeTimers.set(
      jobId,
      window.setTimeout(() => {
        removeTimers.delete(jobId)
        removeJob(jobId)
      }, 3000),
    )
  }
}

// the firehose reports every job regardless of who started it; jobs the UI did not
// trigger itself appear via JobCreated, with the series title resolved in the background
const dismissedIds = new Set<string>()

function onJobCreated(jobId: string, seriesId?: string) {
  if (dismissedIds.has(jobId)) return
  if (useKomfJobs.getState().jobs[jobId]) return
  useKomfJobs.getState().track(jobId, i18n.t('komf.seriesFallback'))
  if (!seriesId) return
  void seriesApi
    .get(seriesId)
    .then((s) => updateJob(jobId, { label: s.metadata.title || s.name }))
    .catch(() => {})
}

function onEvent(event: KomfJobEvent) {
  const jobId = event.jobId
  if (!jobId) return
  switch (event.type) {
    case 'JobCreatedEvent':
      onJobCreated(jobId, event.seriesId)
      break
    case 'JobFinishedEvent':
      // terminal: the firehose never mentions the job again, so the dismiss
      // suppression can go with it
      dismissedIds.delete(jobId)
      finishJob(jobId, event.status === 'FAILED' ? (event.message ?? i18n.t('metadata:identify.matchFailed')) : null)
      break
    case 'ProviderSeriesEvent':
      updateJob(jobId, { text: `${providerName(event.provider)}…` })
      break
    case 'ProviderBookEvent':
      updateJob(jobId, {
        text: `${providerName(event.provider)} · books ${event.bookProgress}/${event.totalBooks}`,
      })
      break
    case 'ProviderCompletedEvent':
      updateJob(jobId, { text: `${providerName(event.provider)} done` })
      break
    case 'ProviderErrorEvent':
    case 'ProcessingErrorEvent':
      updateJob(jobId, { failed: event.message })
      break
    case 'PostProcessingStartEvent':
      updateJob(jobId, { text: i18n.t('komf.postProcessing') })
      break
  }
}

// one long-lived stream for all job activity; a drop is followed by a reconnect,
// like EventSource would
let firehoseStarted = false
let firehoseController: AbortController | null = null
let firehoseRetry: number | undefined

export function startKomfFirehose() {
  if (firehoseStarted) return
  firehoseStarted = true
  void runFirehose()
}

export function stopKomfFirehose() {
  firehoseStarted = false
  firehoseController?.abort()
  window.clearTimeout(firehoseRetry)
}

async function runFirehose() {
  for (;;) {
    // the firehose replays only RUNNING jobs on connect; a job that finished during
    // a disconnect never appears again, so resolve those through the jobs API
    await reconcileUnfinished()
    // stop() during the reconcile left no controller to abort; bail before opening one
    if (!firehoseStarted) return
    firehoseController = new AbortController()
    await streamEvents(firehoseController.signal)
    firehoseController = null
    if (!firehoseStarted) return
    await new Promise<void>((r) => {
      firehoseRetry = window.setTimeout(r, 3000)
    })
  }
}

async function reconcileUnfinished() {
  const ids = Object.values(useKomfJobs.getState().jobs)
    .filter((j) => !j.done)
    .map((j) => j.id)
  await Promise.all(
    ids.map(async (id) => {
      try {
        const job = await komfApi.getJob(id)
        if (job.status !== 'RUNNING') {
          finishJob(id, job.status === 'FAILED' ? (job.message ?? i18n.t('metadata:identify.matchFailed')) : null)
        }
      } catch (e) {
        // only a definitive answer settles the job: 404 means komf has no record of
        // it. A network error (the same drop that killed the stream) just retries
        // after the next reconnect
        if (e instanceof ApiError && e.status === 404) {
          finishJob(id, useKomfJobs.getState().jobs[id]?.failed ?? i18n.t('metadata:identify.matchFailed'))
        }
      }
    }),
  )
}

// a reconnect must run the reconcile first, which EventSource's built-in
// auto-reconnect cannot hook into; the body is read manually instead
async function streamEvents(signal: AbortSignal) {
  try {
    const res = await fetch('/api/v1/komf/jobs/events', {
      signal,
      credentials: 'same-origin',
      headers: { 'X-Requested-With': 'XMLHttpRequest' },
    })
    if (!res.ok || !res.body) throw new Error(`events stream failed: ${res.status}`)
    const reader = res.body.pipeThrough(new TextDecoderStream()).getReader()
    let buffer = ''
    for (;;) {
      const { done, value } = await reader.read()
      if (done) break
      buffer += value
      const frames = buffer.split(/\r?\n\r?\n/)
      buffer = frames.pop() ?? ''
      for (const frame of frames) {
        const event = parseFrame(frame)
        if (event) onEvent(event)
      }
    }
  } catch {
    // stopped, or the connection dropped: the caller decides whether to reconnect
  }
}

// komf relays the discriminator as the SSE event line; the data payload carries
// only the fields. frames without an event line are keep-alives
function parseFrame(frame: string): KomfJobEvent | null {
  let name = ''
  let data = ''
  for (const line of frame.split('\n')) {
    if (line.startsWith('event:')) name = line.slice(6).trim()
    else if (line.startsWith('data:')) data += (data === '' ? '' : '\n') + line.slice(5).trim()
  }
  if (!name) return null
  let fields: Record<string, unknown> = {}
  if (data) {
    try {
      fields = JSON.parse(data) as Record<string, unknown>
    } catch {
      return null
    }
  }
  return { ...fields, type: name } as KomfJobEvent
}
