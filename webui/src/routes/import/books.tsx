import { useMemo, useState } from 'react'
import { Link } from 'react-router-dom'
import { useMutation } from '@tanstack/react-query'
import { Trans, useTranslation } from 'react-i18next'
import { CheckCircle, FolderOpen, MagnifyingGlass, Scan as ScanIcon } from '@phosphor-icons/react'
import i18n from '@/lib/i18n'
import { booksApi } from '@/lib/api/books'
import { seriesApi } from '@/lib/api/series'
import { transientBooksApi } from '@/lib/api/transientBooks'
import type { CopyMode, TransientBookDto } from '@/lib/api/types'
import { cn } from '@/lib/utils/cn'
import { Button } from '@/components/ui/Button'
import { IconButton } from '@/components/ui/IconButton'
import { PageHeader } from '@/components/ui/PageHeader'
import { TextField } from '@/components/ui/TextField'
import { SegmentedControl } from '@/components/ui/SegmentedControl'
import { EmptyState } from '@/components/ui/EmptyState'
import { FilesystemDialog } from '@/components/admin/libraries/FilesystemDialog'
import { SeriesPickerDialog } from '@/components/collections/SeriesPickerDialog'

interface ImportRow {
  id: string
  sourceFile: string
  fileName: string
  size: string
  status: string
  checked: boolean
  seriesId: string | null
  seriesTitle: string | null
  number: string
  destinationName: string
}

function toRow(b: TransientBookDto): ImportRow {
  return {
    id: b.id,
    sourceFile: b.url,
    fileName: b.name,
    size: b.size,
    status: b.status,
    checked: true,
    seriesId: b.seriesId,
    seriesTitle: null,
    number: b.number != null ? String(b.number) : '',
    destinationName: b.name,
  }
}

function statusBadge(status: string): { label: string; className: string } {
  switch (status) {
    case 'READY':
      return { label: i18n.t('import:books.status.analyzed'), className: 'border-accent/40 bg-accent-soft text-accent-strong' }
    case 'ERROR':
    case 'UNSUPPORTED':
      return {
        label: status === 'ERROR' ? i18n.t('cardStatus.error') : i18n.t('cardStatus.unsupported'),
        className: 'border-danger/40 bg-danger/10 text-danger',
      }
    default:
      return { label: i18n.t('import:books.status.notAnalyzed'), className: 'border-line bg-raised text-ink-2' }
  }
}

export function ImportBooksPage() {
  const { t } = useTranslation('import')
  const [path, setPath] = useState('')
  const [browseOpen, setBrowseOpen] = useState(false)
  const [rows, setRows] = useState<ImportRow[]>([])
  const [scanned, setScanned] = useState(false)
  const [copyMode, setCopyMode] = useState<CopyMode>('HARDLINK')
  const [pickerRow, setPickerRow] = useState<string | null>(null)
  const [analyzingIds, setAnalyzingIds] = useState<Set<string>>(new Set())
  const [submitAttempted, setSubmitAttempted] = useState(false)
  const [importedCount, setImportedCount] = useState<number | null>(null)

  const patchRow = (id: string, patch: Partial<ImportRow>) =>
    setRows((prev) => prev.map((r) => (r.id === id ? { ...r, ...patch } : r)))

  const scanMutation = useMutation({
    mutationFn: (p: string) => transientBooksApi.scan(p),
    onSuccess: (books) => {
      setRows(books.map(toRow))
      setScanned(true)
      setImportedCount(null)
      setSubmitAttempted(false)
    },
  })

  const analyzeMutation = useMutation({
    mutationFn: (id: string) => transientBooksApi.analyze(id),
    onMutate: (id) => setAnalyzingIds((prev) => new Set(prev).add(id)),
    onSettled: (_d, _e, id) =>
      setAnalyzingIds((prev) => {
        const next = new Set(prev)
        next.delete(id)
        return next
      }),
    onSuccess: (book) => {
      patchRow(book.id, {
        status: book.status,
        number: book.number != null ? String(book.number) : '',
        seriesId: book.seriesId,
        seriesTitle: null,
      })
      // the DTO carries only the series id; resolve its title for display
      if (book.seriesId) {
        seriesApi
          .get(book.seriesId)
          .then((s) => patchRow(book.id, { seriesTitle: s.metadata.title || s.name }))
          .catch(() => {})
      }
    },
  })

  const importMutation = useMutation({
    mutationFn: booksApi.importBooks,
    onSuccess: (_d, variables) => setImportedCount(variables.books.length),
  })

  const checkedRows = useMemo(() => rows.filter((r) => r.checked), [rows])
  const missingSeries = useMemo(() => checkedRows.filter((r) => !r.seriesId), [checkedRows])

  const submit = () => {
    setSubmitAttempted(true)
    if (checkedRows.length === 0 || missingSeries.length > 0) return
    importMutation.mutate({
      copyMode,
      books: checkedRows.map((r) => ({
        sourceFile: r.sourceFile,
        seriesId: r.seriesId!,
        destinationName: r.destinationName.trim() || undefined,
      })),
    })
  }

  const reset = () => {
    setRows([])
    setScanned(false)
    setImportedCount(null)
    setSubmitAttempted(false)
    scanMutation.reset()
    importMutation.reset()
  }

  return (
    <div>
      <PageHeader title={t('books.title')} subtitle={t('books.subtitle')} />

      <div className="mb-6 max-w-2xl rounded-xl border border-line bg-surface px-5 py-4">
        <div className="flex flex-wrap items-end gap-3">
          <TextField
            label={t('books.folderLabel')}
            value={path}
            onChange={(e) => setPath(e.target.value)}
            placeholder="/data/incoming"
            className="min-w-56 flex-1 font-mono"
          />
          <Button variant="secondary" onClick={() => setBrowseOpen(true)}>
            <FolderOpen className="size-4" /> {t('books.browse')}
          </Button>
          <Button
            variant="primary"
            disabled={!path.trim()}
            loading={scanMutation.isPending}
            onClick={() => scanMutation.mutate(path.trim())}
          >
            <ScanIcon className="size-4" /> {t('books.scan')}
          </Button>
        </div>
        {scanMutation.error && <p className="mt-3 text-sm text-danger">{scanMutation.error.message}</p>}
      </div>

      {importedCount !== null ? (
        <div className="max-w-2xl rounded-xl border border-accent/40 bg-accent-soft px-5 py-6">
          <div className="flex items-center gap-2">
            <CheckCircle className="size-5 text-accent-strong" />
            <h2 className="text-[15px] font-semibold text-ink">{t('books.queued', { count: importedCount })}</h2>
          </div>
          <p className="mt-2 text-sm text-ink-2">
            <Trans
              i18nKey="books.queuedBody"
              ns="import"
              components={{
                link: <Link to="/admin/server" className="text-accent-strong underline-offset-2 hover:underline" />,
              }}
            />
          </p>
          <div className="mt-4">
            <Button variant="secondary" size="sm" onClick={reset}>
              {t('books.importMore')}
            </Button>
          </div>
        </div>
      ) : scanned && rows.length === 0 && !scanMutation.isPending ? (
        <EmptyState title={t('books.emptyTitle')} body={t('books.emptyBody')} />
      ) : rows.length > 0 ? (
        <>
          <div className="mb-4 flex flex-wrap items-center gap-x-6 gap-y-3">
            <SegmentedControl
              size="sm"
              value={copyMode}
              onChange={setCopyMode}
              options={[
                { value: 'MOVE', label: t('books.copyMode.move') },
                { value: 'COPY', label: t('books.copyMode.copy') },
                { value: 'HARDLINK', label: t('books.copyMode.hardlink') },
              ]}
            />
            <p className="text-sm text-ink-3">
              {t('books.selectedCount', { count: rows.length, checked: checkedRows.length })}
            </p>
            <div className="ml-auto">
              <Button
                variant="primary"
                size="sm"
                loading={importMutation.isPending}
                disabled={checkedRows.length === 0}
                onClick={submit}
              >
                {t('books.importSelected')}
              </Button>
            </div>
          </div>
          {submitAttempted && missingSeries.length > 0 && (
            <p className="mb-4 text-sm text-danger">{t('books.missingSeries', { count: missingSeries.length })}</p>
          )}
          {analyzeMutation.error && <p className="mb-4 text-sm text-danger">{analyzeMutation.error.message}</p>}
          {importMutation.error && <p className="mb-4 text-sm text-danger">{importMutation.error.message}</p>}

          <div className="overflow-x-auto rounded-xl border border-line bg-surface">
            <table className="w-full min-w-[860px] text-sm">
              <thead>
                <tr className="text-left text-xs text-ink-3">
                  <th className="px-3 py-2.5 font-medium">
                    <input
                      type="checkbox"
                      checked={checkedRows.length === rows.length && rows.length > 0}
                      onChange={(e) => setRows((prev) => prev.map((r) => ({ ...r, checked: e.target.checked })))}
                      aria-label={t('books.selectAll')}
                      className="size-4 cursor-pointer accent-accent"
                    />
                  </th>
                  <th className="px-3 py-2.5 font-medium">{t('books.table.sourceFile')}</th>
                  <th className="px-3 py-2.5 font-medium">{t('books.table.series')}</th>
                  <th className="px-3 py-2.5 font-medium">{t('books.table.number')}</th>
                  <th className="px-3 py-2.5 font-medium">{t('books.table.destinationName')}</th>
                  <th className="px-3 py-2.5 text-right font-medium">{t('books.table.analyze')}</th>
                </tr>
              </thead>
              <tbody>
                {rows.map((r) => {
                  const badge = statusBadge(r.status)
                  const missing = submitAttempted && r.checked && !r.seriesId
                  return (
                    <tr key={r.id} className="border-t border-line">
                      <td className="px-3 py-2.5">
                        <input
                          type="checkbox"
                          checked={r.checked}
                          onChange={(e) => patchRow(r.id, { checked: e.target.checked })}
                          aria-label={t('books.importFile', { fileName: r.fileName })}
                          className="size-4 cursor-pointer accent-accent"
                        />
                      </td>
                      <td className="max-w-72 px-3 py-2.5">
                        <p className="truncate text-ink" title={r.sourceFile}>
                          {r.fileName}
                        </p>
                        <p className="mt-0.5 flex items-center gap-2 text-xs text-ink-3">
                          <span>{r.size}</span>
                          <span
                            className={cn(
                              'rounded-full border px-2 py-0.5 text-[11px] font-medium whitespace-nowrap',
                              badge.className,
                            )}
                          >
                            {badge.label}
                          </span>
                        </p>
                      </td>
                      <td className="max-w-56 px-3 py-2.5">
                        {r.seriesId ? (
                          <div className="flex items-center gap-1.5">
                            <p className="min-w-0 truncate text-ink">{r.seriesTitle ?? t('books.selectedSeries')}</p>
                            <Button variant="ghost" size="sm" onClick={() => setPickerRow(r.id)}>
                              {t('books.change')}
                            </Button>
                          </div>
                        ) : (
                          <button
                            type="button"
                            onClick={() => setPickerRow(r.id)}
                            className={cn(
                              'cursor-pointer text-sm transition-colors',
                              missing ? 'text-danger hover:text-danger' : 'text-accent-strong hover:text-accent',
                            )}
                          >
                            {t('books.selectSeries')}
                          </button>
                        )}
                      </td>
                      <td className="px-3 py-2.5">
                        <input
                          type="text"
                          inputMode="decimal"
                          value={r.number}
                          onChange={(e) => patchRow(r.id, { number: e.target.value })}
                          placeholder="#"
                          className="h-8 w-20 rounded-lg border border-line bg-surface px-2 text-base text-ink transition-colors placeholder:text-ink-3 focus:border-accent/70 focus:outline-none"
                        />
                      </td>
                      <td className="px-3 py-2.5">
                        <input
                          type="text"
                          value={r.destinationName}
                          onChange={(e) => patchRow(r.id, { destinationName: e.target.value })}
                          className="h-8 w-full min-w-48 rounded-lg border border-line bg-surface px-2 font-mono text-base text-ink transition-colors focus:border-accent/70 focus:outline-none"
                        />
                      </td>
                      <td className="px-3 py-2.5 text-right">
                        <IconButton
                          label={t('books.analyze')}
                          disabled={analyzingIds.has(r.id)}
                          onClick={() => analyzeMutation.mutate(r.id)}
                        >
                          <MagnifyingGlass className={cn('size-4', analyzingIds.has(r.id) && 'animate-pulse')} />
                        </IconButton>
                      </td>
                    </tr>
                  )
                })}
              </tbody>
            </table>
          </div>
        </>
      ) : null}

      <FilesystemDialog open={browseOpen} onOpenChange={setBrowseOpen} onSelect={setPath} />
      <SeriesPickerDialog
        open={pickerRow !== null}
        onOpenChange={(open) => !open && setPickerRow(null)}
        title={t('books.pickerTitle')}
        confirmLabel={t('books.pickerConfirm')}
        mode="single"
        onConfirm={(selected) => {
          const s = selected[0]
          if (pickerRow && s) patchRow(pickerRow, { seriesId: s.id, seriesTitle: s.metadata.title || s.name })
          setPickerRow(null)
        }}
      />
    </div>
  )
}
