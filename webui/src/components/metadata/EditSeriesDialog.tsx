import { useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { seriesApi } from '@/lib/api/series'
import type { ReadingDirection, SeriesDto, SeriesMetadataUpdateDto, SeriesStatus } from '@/lib/api/types'
import { readingDirectionLabel, seriesStatusLabel } from '@/lib/utils/format'
import { Button } from '@/components/ui/Button'
import { Dialog } from '@/components/ui/Dialog'
import { SegmentedControl } from '@/components/ui/SegmentedControl'
import { Skeleton } from '@/components/ui/Skeleton'
import { TextField } from '@/components/ui/TextField'
import { EmptyState } from '@/components/ui/EmptyState'
import { LabelListEditor } from '@/components/admin/users/LabelListEditor'
import { BatchField, FormErrorBanner, LockToggle, PairListEditor, TextAreaField, TextInput, type Pair, type PairErrors } from './fields'
import { isValidBcp47, isValidUrl, mapViolations } from './validation'

const STATUS_VALUES: SeriesStatus[] = ['ONGOING', 'ENDED', 'ABANDONED', 'HIATUS']

const DIRECTIONS: ReadingDirection[] = ['LEFT_TO_RIGHT', 'RIGHT_TO_LEFT', 'VERTICAL', 'WEBTOON']

const asDirection = (v: string): ReadingDirection | '' => (DIRECTIONS as string[]).includes(v) ? (v as ReadingDirection) : ''

const GENERAL_FIELDS = ['title', 'titleSort', 'summary', 'status', 'language', 'publisher', 'ageRating', 'totalBookCount', 'readingDirection']

type Tab = 'general' | 'titles' | 'tags' | 'links' | 'sharing'

const TAB_OF_FIELD: Record<string, Tab> = {
  ...Object.fromEntries(GENERAL_FIELDS.map((f) => [f, 'general' as Tab])),
  alternateTitles: 'titles',
  genres: 'tags',
  tags: 'tags',
  links: 'links',
  sharingLabels: 'sharing',
}

export function EditSeriesDialog({ open, onClose, seriesIds }: { open: boolean; onClose: () => void; seriesIds: string[] }) {
  const { t } = useTranslation('metadata')
  const singleId = seriesIds.length === 1 ? seriesIds[0] : null
  const seriesQuery = useQuery({
    queryKey: ['series', singleId],
    queryFn: () => seriesApi.get(singleId!),
    enabled: open && !!singleId,
  })

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (!o) onClose()
      }}
      title={t('editSeries.dialogTitle', { context: singleId ? 'single' : 'batch', count: seriesIds.length })}
      size="lg"
    >
      {singleId && seriesQuery.isPending && (
        <div className="flex flex-col gap-4 px-5 py-4">
          <Skeleton className="h-10 w-full" />
          <Skeleton className="h-10 w-2/3" />
          <Skeleton className="h-24 w-full" />
        </div>
      )}
      {singleId && seriesQuery.isLoadingError && (
        <EmptyState
          title={t('editSeries.loadError')}
          body={seriesQuery.error.message}
          action={
            <Button variant="secondary" onClick={() => seriesQuery.refetch()}>
              {t('common:action.retry')}
            </Button>
          }
        />
      )}
      {singleId && seriesQuery.data && <SingleSeriesForm key={seriesQuery.data.id} series={seriesQuery.data} onClose={onClose} />}
      {!singleId && open && <BatchSeriesForm key={seriesIds.join(',')} ids={seriesIds} onClose={onClose} />}
    </Dialog>
  )
}

function SingleSeriesForm({ series, onClose }: { series: SeriesDto; onClose: () => void }) {
  const { t } = useTranslation('metadata')
  const queryClient = useQueryClient()
  const statusOptions = STATUS_VALUES.map((v) => ({ value: v, label: seriesStatusLabel(v) }))
  const directionOptions: { value: ReadingDirection | ''; label: string }[] = [
    { value: '', label: t('field.notSet') },
    ...DIRECTIONS.map((v) => ({ value: v, label: readingDirectionLabel(v) })),
  ]
  const md = series.metadata
  const [tab, setTab] = useState<Tab>('general')
  const [title, setTitle] = useState(md.title)
  const [titleSort, setTitleSort] = useState(md.titleSort)
  const [summary, setSummary] = useState(md.summary)
  const [status, setStatus] = useState<SeriesStatus>(md.status)
  const [language, setLanguage] = useState(md.language)
  const [direction, setDirection] = useState<ReadingDirection | ''>(asDirection(md.readingDirection))
  const [publisher, setPublisher] = useState(md.publisher)
  const [ageRating, setAgeRating] = useState(md.ageRating != null ? String(md.ageRating) : '')
  const [totalBookCount, setTotalBookCount] = useState(md.totalBookCount != null ? String(md.totalBookCount) : '')
  const [altTitles, setAltTitles] = useState<Pair[]>(md.alternateTitles.map((t) => ({ a: t.label, b: t.title })))
  const [genres, setGenres] = useState<string[]>(md.genres)
  const [tags, setTags] = useState<string[]>(md.tags)
  const [links, setLinks] = useState<Pair[]>(md.links.map((l) => ({ a: l.label, b: l.url })))
  const [sharingLabels, setSharingLabels] = useState<string[]>(md.sharingLabels)
  const [locks, setLocks] = useState({
    title: md.titleLock,
    titleSort: md.titleSortLock,
    summary: md.summaryLock,
    status: md.statusLock,
    language: md.languageLock,
    readingDirection: md.readingDirectionLock,
    publisher: md.publisherLock,
    ageRating: md.ageRatingLock,
    totalBookCount: md.totalBookCountLock,
    alternateTitles: md.alternateTitlesLock,
    genres: md.genresLock,
    tags: md.tagsLock,
    links: md.linksLock,
    sharingLabels: md.sharingLabelsLock,
  })
  type LockKey = keyof typeof locks
  const toggleLock = (key: LockKey) => setLocks((p) => ({ ...p, [key]: !p[key] }))
  // editing a field locks it, mirroring komga: manual edits are protected from metadata refresh
  const autoLock = (key: LockKey) => setLocks((p) => (p[key] ? p : { ...p, [key]: true }))
  const [errors, setErrors] = useState<Record<string, string>>({})
  const [pairErrors, setPairErrors] = useState<{ alternateTitles: PairErrors; links: PairErrors }>({ alternateTitles: {}, links: {} })
  const [formErrors, setFormErrors] = useState<string[]>([])

  const save = useMutation({
    mutationFn: (body: SeriesMetadataUpdateDto) => seriesApi.patchMetadata(series.id, body),
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['series'] })
      queryClient.invalidateQueries({ queryKey: ['books'] })
      queryClient.invalidateQueries({ queryKey: ['dashboard'] })
      onClose()
    },
    onError: (err) => {
      const mapped = mapViolations(err)
      const scalars: Record<string, string> = {}
      const rest = [...mapped.rest]
      for (const [k, v] of Object.entries(mapped.scalars)) {
        if (TAB_OF_FIELD[k]) scalars[k] = v
        else rest.push(`${k}: ${v}`)
      }
      setErrors(scalars)
      setPairErrors((p) => ({
        alternateTitles: { ...p.alternateTitles, ...mapped.pairs.alternateTitles },
        links: { ...p.links, ...mapped.pairs.links },
      }))
      setFormErrors(rest.length > 0 ? rest : [err instanceof Error ? err.message : t('saveFailed')])
      jumpToError([...Object.keys(scalars), ...Object.keys(mapped.pairs)])
    },
  })

  const jumpToError = (fields: string[]) => {
    const first = fields.map((f) => TAB_OF_FIELD[f]).find((tb) => tb && tb !== tab)
    if (first) setTab(first)
  }

  const validate = (): boolean => {
    const e: Record<string, string> = {}
    const te: PairErrors = {}
    const le: PairErrors = {}
    if (!title.trim()) e.title = t('validation.notBlank')
    if (!titleSort.trim()) e.titleSort = t('validation.notBlank')
    if (ageRating.trim() && (!Number.isInteger(Number(ageRating)) || Number(ageRating) < 0))
      e.ageRating = t('validation.wholeNumberMinZero')
    if (totalBookCount.trim() && (!Number.isInteger(Number(totalBookCount)) || Number(totalBookCount) <= 0))
      e.totalBookCount = t('validation.wholeNumberPositive')
    if (language.trim() && !isValidBcp47(language.trim())) e.language = t('validation.bcp47')
    altTitles.forEach((p, i) => {
      const row: { a?: string; b?: string } = {}
      if (!p.a.trim()) row.a = t('validation.notBlank')
      if (!p.b.trim()) row.b = t('validation.notBlank')
      if (row.a || row.b) te[i] = row
    })
    links.forEach((p, i) => {
      const row: { a?: string; b?: string } = {}
      if (!p.a.trim()) row.a = t('validation.notBlank')
      if (!isValidUrl(p.b.trim())) row.b = t('validation.url')
      if (row.a || row.b) le[i] = row
    })
    setErrors(e)
    setPairErrors({ alternateTitles: te, links: le })
    setFormErrors([])
    const failed = [...Object.keys(e), ...(Object.keys(te).length > 0 ? ['alternateTitles'] : []), ...(Object.keys(le).length > 0 ? ['links'] : [])]
    if (failed.length > 0) {
      jumpToError(failed)
      return false
    }
    return true
  }

  const submit = (ev: FormEvent) => {
    ev.preventDefault()
    if (save.isPending || !validate()) return
    save.mutate({
      title: title.trim(),
      titleLock: locks.title,
      titleSort: titleSort.trim(),
      titleSortLock: locks.titleSort,
      summary,
      summaryLock: locks.summary,
      status,
      statusLock: locks.status,
      language: language.trim(),
      languageLock: locks.language,
      publisher: publisher.trim(),
      publisherLock: locks.publisher,
      readingDirection: direction === '' ? null : direction,
      readingDirectionLock: locks.readingDirection,
      ageRating: ageRating.trim() === '' ? null : Number(ageRating),
      ageRatingLock: locks.ageRating,
      totalBookCount: totalBookCount.trim() === '' ? null : Number(totalBookCount),
      totalBookCountLock: locks.totalBookCount,
      genres,
      genresLock: locks.genres,
      tags,
      tagsLock: locks.tags,
      sharingLabels,
      sharingLabelsLock: locks.sharingLabels,
      alternateTitles: altTitles.map((p) => ({ label: p.a.trim(), title: p.b.trim() })),
      alternateTitlesLock: locks.alternateTitles,
      links: links.map((p) => ({ label: p.a.trim(), url: p.b.trim() })),
      linksLock: locks.links,
    })
  }

  const clearError = (key: string) =>
    setErrors((p) => {
      const next = { ...p }
      delete next[key]
      return next
    })

  return (
    <form onSubmit={submit}>
      <div className="flex flex-col gap-5 px-5 py-4">
        <FormErrorBanner messages={formErrors} />
        <SegmentedControl<Tab>
          options={[
            { value: 'general', label: t('tabs.general') },
            { value: 'titles', label: t('tabs.titles') },
            { value: 'tags', label: t('tabs.tags') },
            { value: 'links', label: t('tabs.links') },
            { value: 'sharing', label: t('tabs.sharing') },
          ]}
          value={tab}
          onChange={setTab}
        />

        {tab === 'general' && (
          <>
            <div className="grid gap-4 sm:grid-cols-2">
              <TextField
                label={t('field.title')}
                required
                value={title}
                onChange={(e) => { setTitle(e.target.value); autoLock('title'); clearError('title') }}
                error={errors.title}
                trailing={<LockToggle locked={locks.title} onChange={() => toggleLock('title')} label={t('field.title')} />}
              />
              <TextField
                label={t('field.sortTitle')}
                required
                value={titleSort}
                onChange={(e) => { setTitleSort(e.target.value); autoLock('titleSort'); clearError('titleSort') }}
                error={errors.titleSort}
                trailing={<LockToggle locked={locks.titleSort} onChange={() => toggleLock('titleSort')} label={t('field.sortTitle')} />}
              />
            </div>
            <TextAreaField
              label={t('field.summary')}
              value={summary}
              onChange={(v) => { setSummary(v); autoLock('summary') }}
              error={errors.summary}
              trailing={<LockToggle locked={locks.summary} onChange={() => toggleLock('summary')} label={t('field.summary')} />}
            />
            <div className="grid gap-4 sm:grid-cols-2">
              <div className="flex flex-col gap-2">
                <div className="flex items-center justify-between gap-2">
                  <span className="text-[13px] font-medium text-ink-2">{t('field.status')}</span>
                  <LockToggle locked={locks.status} onChange={() => toggleLock('status')} label={t('field.status')} />
                </div>
                <SegmentedControl<SeriesStatus> options={statusOptions} value={status} onChange={(v) => { setStatus(v); autoLock('status') }} className="flex flex-wrap" />
              </div>
              <div className="flex flex-col gap-2">
                <div className="flex items-center justify-between gap-2">
                  <span className="text-[13px] font-medium text-ink-2">{t('field.readingDirection')}</span>
                  <LockToggle locked={locks.readingDirection} onChange={() => toggleLock('readingDirection')} label={t('field.readingDirection')} />
                </div>
                <SegmentedControl<ReadingDirection | ''>
                  options={directionOptions}
                  value={direction}
                  onChange={(v) => { setDirection(v); autoLock('readingDirection') }}
                  className="flex flex-wrap"
                />
              </div>
            </div>
            <div className="grid gap-4 sm:grid-cols-2">
              <TextField
                label={t('field.publisher')}
                value={publisher}
                onChange={(e) => { setPublisher(e.target.value); autoLock('publisher'); clearError('publisher') }}
                error={errors.publisher}
                trailing={<LockToggle locked={locks.publisher} onChange={() => toggleLock('publisher')} label={t('field.publisher')} />}
              />
              <TextField
                label={t('field.language')}
                placeholder="en, ja, zh-Hans…"
                value={language}
                onChange={(e) => { setLanguage(e.target.value); autoLock('language'); clearError('language') }}
                error={errors.language}
                helper={errors.language ? undefined : t('editSeries.languageHelper')}
                trailing={<LockToggle locked={locks.language} onChange={() => toggleLock('language')} label={t('field.language')} />}
              />
              <TextField
                label={t('field.ageRating')}
                inputMode="numeric"
                placeholder={t('field.notSet')}
                value={ageRating}
                onChange={(e) => { setAgeRating(e.target.value); autoLock('ageRating'); clearError('ageRating') }}
                error={errors.ageRating}
                trailing={<LockToggle locked={locks.ageRating} onChange={() => toggleLock('ageRating')} label={t('field.ageRating')} />}
              />
              <TextField
                label={t('field.totalBookCount')}
                inputMode="numeric"
                placeholder={t('field.notSet')}
                value={totalBookCount}
                onChange={(e) => { setTotalBookCount(e.target.value); autoLock('totalBookCount'); clearError('totalBookCount') }}
                error={errors.totalBookCount}
                trailing={<LockToggle locked={locks.totalBookCount} onChange={() => toggleLock('totalBookCount')} label={t('field.totalBookCount')} />}
              />
            </div>
          </>
        )}

        {tab === 'titles' && (
          <PairListEditor
            label={t('field.alternateTitles')}
            pairs={altTitles}
            onChange={(p) => { setAltTitles(p); autoLock('alternateTitles') }}
            aLabel={t('field.label')}
            bLabel={t('field.title')}
            aPlaceholder="ja"
            bPlaceholder="ベルセルク"
            addLabel={t('add.title')}
            errors={pairErrors.alternateTitles}
            trailing={<LockToggle locked={locks.alternateTitles} onChange={() => toggleLock('alternateTitles')} label={t('field.alternateTitles')} />}
          />
        )}

        {tab === 'tags' && (
          <div className="grid gap-4 sm:grid-cols-2">
            <LabelListEditor
              label={t('field.genres')}
              values={genres}
              onChange={(v) => { setGenres(v); autoLock('genres') }}
              placeholder={t('placeholder.addGenre')}
              trailing={<LockToggle locked={locks.genres} onChange={() => toggleLock('genres')} label={t('field.genres')} />}
            />
            <LabelListEditor
              label={t('field.tags')}
              values={tags}
              onChange={(v) => { setTags(v); autoLock('tags') }}
              placeholder={t('placeholder.addTag')}
              trailing={<LockToggle locked={locks.tags} onChange={() => toggleLock('tags')} label={t('field.tags')} />}
            />
          </div>
        )}

        {tab === 'links' && (
          <PairListEditor
            label={t('field.webLinks')}
            pairs={links}
            onChange={(p) => { setLinks(p); autoLock('links') }}
            aLabel={t('field.label')}
            bLabel={t('field.url')}
            aPlaceholder={t('placeholder.linkLabel')}
            bPlaceholder="https://…"
            addLabel={t('add.link')}
            errors={pairErrors.links}
            trailing={<LockToggle locked={locks.links} onChange={() => toggleLock('links')} label={t('field.webLinks')} />}
          />
        )}

        {tab === 'sharing' && (
          <LabelListEditor
            label={t('field.sharingLabels')}
            values={sharingLabels}
            onChange={(v) => { setSharingLabels(v); autoLock('sharingLabels') }}
            placeholder={t('placeholder.addLabel')}
            trailing={<LockToggle locked={locks.sharingLabels} onChange={() => toggleLock('sharingLabels')} label={t('field.sharingLabels')} />}
          />
        )}
      </div>

      <div className="flex justify-end gap-2 border-t border-line px-5 py-3.5">
        <Button type="button" variant="ghost" onClick={onClose}>
          {t('common:action.cancel')}
        </Button>
        <Button type="submit" variant="primary" loading={save.isPending}>
          {t('action.saveChanges')}
        </Button>
      </div>
    </form>
  )
}

type BatchKey = 'status' | 'readingDirection' | 'publisher' | 'language' | 'ageRating' | 'genres' | 'tags' | 'sharingLabels'

function BatchSeriesForm({ ids, onClose }: { ids: string[]; onClose: () => void }) {
  const { t } = useTranslation('metadata')
  const queryClient = useQueryClient()
  const statusOptions = STATUS_VALUES.map((v) => ({ value: v, label: seriesStatusLabel(v) }))
  const directionOptions: { value: ReadingDirection | ''; label: string }[] = [
    { value: '', label: t('field.notSet') },
    ...DIRECTIONS.map((v) => ({ value: v, label: readingDirectionLabel(v) })),
  ]
  const [enabled, setEnabled] = useState<Partial<Record<BatchKey, boolean>>>({})
  const [status, setStatus] = useState<SeriesStatus>('ONGOING')
  const [direction, setDirection] = useState<ReadingDirection | ''>('')
  const [publisher, setPublisher] = useState('')
  const [language, setLanguage] = useState('')
  const [ageRating, setAgeRating] = useState('')
  const [genres, setGenres] = useState<string[]>([])
  const [tags, setTags] = useState<string[]>([])
  const [sharingLabels, setSharingLabels] = useState<string[]>([])
  const [sharedLocks, setSharedLocks] = useState<Partial<Record<BatchKey, boolean>>>({})
  const [errors, setErrors] = useState<Record<string, string>>({})
  const [formErrors, setFormErrors] = useState<string[]>([])

  const toggle = (key: BatchKey) => (v: boolean) => {
    setEnabled((p) => ({ ...p, [key]: v }))
    if (v) setSharedLocks((p) => ({ ...p, [key]: p[key] ?? true }))
  }
  const toggleSharedLock = (key: BatchKey) => (v: boolean) => setSharedLocks((p) => ({ ...p, [key]: v }))
  const anyEnabled = Object.values(enabled).some(Boolean)

  const save = useMutation({
    mutationFn: async (body: SeriesMetadataUpdateDto) => {
      await Promise.all(ids.map((id) => seriesApi.patchMetadata(id, body)))
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ['series'] })
      queryClient.invalidateQueries({ queryKey: ['books'] })
      queryClient.invalidateQueries({ queryKey: ['dashboard'] })
      onClose()
    },
    onError: (err) => {
      const mapped = mapViolations(err)
      setErrors(mapped.scalars)
      setFormErrors(mapped.rest.length > 0 ? mapped.rest : [err instanceof Error ? err.message : t('saveFailed')])
    },
  })

  const submit = (ev: FormEvent) => {
    ev.preventDefault()
    if (save.isPending || !anyEnabled) return
    const e: Record<string, string> = {}
    if (enabled.ageRating && ageRating.trim() && (!Number.isInteger(Number(ageRating)) || Number(ageRating) < 0))
      e.ageRating = t('validation.wholeNumberMinZero')
    if (enabled.language && language.trim() && !isValidBcp47(language.trim())) e.language = t('validation.bcp47')
    setErrors(e)
    setFormErrors([])
    if (Object.keys(e).length > 0) return

    const body: SeriesMetadataUpdateDto = {}
    if (enabled.status) {
      body.status = status
      body.statusLock = sharedLocks.status ?? true
    }
    if (enabled.readingDirection) {
      body.readingDirection = direction === '' ? null : direction
      body.readingDirectionLock = sharedLocks.readingDirection ?? true
    }
    if (enabled.publisher) {
      body.publisher = publisher.trim()
      body.publisherLock = sharedLocks.publisher ?? true
    }
    if (enabled.language) {
      body.language = language.trim()
      body.languageLock = sharedLocks.language ?? true
    }
    if (enabled.ageRating) {
      body.ageRating = ageRating.trim() === '' ? null : Number(ageRating)
      body.ageRatingLock = sharedLocks.ageRating ?? true
    }
    if (enabled.genres) {
      body.genres = genres
      body.genresLock = sharedLocks.genres ?? true
    }
    if (enabled.tags) {
      body.tags = tags
      body.tagsLock = sharedLocks.tags ?? true
    }
    if (enabled.sharingLabels) {
      body.sharingLabels = sharingLabels
      body.sharingLabelsLock = sharedLocks.sharingLabels ?? true
    }
    save.mutate(body)
  }

  return (
    <form onSubmit={submit}>
      <div className="flex flex-col gap-3 px-5 py-4">
        <FormErrorBanner messages={formErrors} />
        <p className="text-sm text-ink-3">{t('editSeries.batchHint', { count: ids.length })}</p>

        <BatchField label={t('field.status')} enabled={!!enabled.status} onEnabledChange={toggle('status')} locked={sharedLocks.status ?? true} onLockedChange={toggleSharedLock('status')}>
          <SegmentedControl<SeriesStatus> options={statusOptions} value={status} onChange={setStatus} className="flex flex-wrap" />
        </BatchField>

        <BatchField label={t('field.readingDirection')} enabled={!!enabled.readingDirection} onEnabledChange={toggle('readingDirection')} hint={t('editSeries.hint.directionNotSet')} locked={sharedLocks.readingDirection ?? true} onLockedChange={toggleSharedLock('readingDirection')}>
          <SegmentedControl<ReadingDirection | ''> options={directionOptions} value={direction} onChange={setDirection} className="flex flex-wrap" />
        </BatchField>

        <BatchField label={t('field.publisher')} enabled={!!enabled.publisher} onEnabledChange={toggle('publisher')} locked={sharedLocks.publisher ?? true} onLockedChange={toggleSharedLock('publisher')}>
          <TextInput value={publisher} onChange={(e) => setPublisher(e.target.value)} aria-label={t('field.publisher')} />
        </BatchField>

        <BatchField label={t('field.language')} enabled={!!enabled.language} onEnabledChange={toggle('language')} hint={t('editSeries.hint.languageEmpty')} locked={sharedLocks.language ?? true} onLockedChange={toggleSharedLock('language')}>
          <TextInput
            value={language}
            onChange={(e) => { setLanguage(e.target.value); setErrors((p) => { const next = { ...p }; delete next.language; return next }) }}
            aria-label={t('field.language')}
            placeholder="en, ja, zh-Hans…"
            error={!!errors.language}
          />
          {errors.language && <p className="mt-1 text-[13px] text-danger">{errors.language}</p>}
        </BatchField>

        <BatchField label={t('field.ageRating')} enabled={!!enabled.ageRating} onEnabledChange={toggle('ageRating')} hint={t('editSeries.hint.ageRatingEmpty')} locked={sharedLocks.ageRating ?? true} onLockedChange={toggleSharedLock('ageRating')}>
          <TextInput
            value={ageRating}
            onChange={(e) => { setAgeRating(e.target.value); setErrors((p) => { const next = { ...p }; delete next.ageRating; return next }) }}
            aria-label={t('field.ageRating')}
            inputMode="numeric"
            error={!!errors.ageRating}
          />
          {errors.ageRating && <p className="mt-1 text-[13px] text-danger">{errors.ageRating}</p>}
        </BatchField>

        <BatchField label={t('field.genres')} enabled={!!enabled.genres} onEnabledChange={toggle('genres')} hint={t('editSeries.hint.genresReplace')} locked={sharedLocks.genres ?? true} onLockedChange={toggleSharedLock('genres')}>
          <LabelListEditor label={t('field.genres')} values={genres} onChange={setGenres} placeholder={t('placeholder.addGenre')} />
        </BatchField>

        <BatchField label={t('field.tags')} enabled={!!enabled.tags} onEnabledChange={toggle('tags')} hint={t('editSeries.hint.tagsReplace')} locked={sharedLocks.tags ?? true} onLockedChange={toggleSharedLock('tags')}>
          <LabelListEditor label={t('field.tags')} values={tags} onChange={setTags} placeholder={t('placeholder.addTag')} />
        </BatchField>

        <BatchField label={t('field.sharingLabels')} enabled={!!enabled.sharingLabels} onEnabledChange={toggle('sharingLabels')} hint={t('editSeries.hint.sharingLabelsReplace')} locked={sharedLocks.sharingLabels ?? true} onLockedChange={toggleSharedLock('sharingLabels')}>
          <LabelListEditor label={t('field.sharingLabels')} values={sharingLabels} onChange={setSharingLabels} placeholder={t('placeholder.addLabel')} />
        </BatchField>
      </div>

      <div className="flex justify-end gap-2 border-t border-line px-5 py-3.5">
        <Button type="button" variant="ghost" onClick={onClose}>
          {t('common:action.cancel')}
        </Button>
        <Button type="submit" variant="primary" loading={save.isPending} disabled={!anyEnabled}>
          {t('action.saveChanges')}
        </Button>
      </div>
    </form>
  )
}
