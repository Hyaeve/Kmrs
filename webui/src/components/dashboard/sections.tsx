import type { ReactNode } from 'react'
import { booksApi } from '@/lib/api/books'
import { seriesApi } from '@/lib/api/series'
import type { BookDto, Page, SearchCondition, SeriesDto } from '@/lib/api/types'
import { BookCard } from '@/components/media/BookCard'
import { KeepReadingCard } from '@/components/media/KeepReadingCard'
import { SeriesCard } from '@/components/media/SeriesCard'
import { HorizontalCardSkeleton } from '@/components/ui/Skeleton'
import { BookRowCard, SeriesRowCard } from './row-cards'

const PAGE_SIZE = 20

export interface DashboardSectionDef {
  /** dashboard-ns key, resolved with t() at the render site */
  titleKey: string
  kind: 'book' | 'series'
  /** libraryIds undefined aggregates every library; callers must not pass an empty array */
  fetchPage: (libraryIds: string[] | undefined, page: number) => Promise<Page<BookDto | SeriesDto>>
  renderRow: (item: BookDto | SeriesDto) => ReactNode
  renderGrid: (item: BookDto | SeriesDto) => ReactNode
  /** pending placeholder for the row; defaults to grid card skeletons */
  renderSkeleton?: () => ReactNode
}

interface BookSectionDef {
  titleKey: string
  fetchPage: (libraryIds: string[] | undefined, page: number) => Promise<Page<BookDto>>
  renderRow: (book: BookDto) => ReactNode
  renderGrid: (book: BookDto) => ReactNode
  renderSkeleton?: () => ReactNode
}

interface SeriesSectionDef {
  titleKey: string
  fetchPage: (libraryIds: string[] | undefined, page: number) => Promise<Page<SeriesDto>>
  renderRow: (series: SeriesDto) => ReactNode
  renderGrid: (series: SeriesDto) => ReactNode
}

// per-kind defs keep card props typed; the record erases to the union for shared consumption
const bookSection = (def: BookSectionDef): DashboardSectionDef => ({ kind: 'book', ...def }) as DashboardSectionDef
const seriesSection = (def: SeriesSectionDef): DashboardSectionDef => ({ kind: 'series', ...def }) as DashboardSectionDef

function libraryConditions(libraryIds: string[] | undefined): SearchCondition[] {
  if (!libraryIds || libraryIds.length === 0) return []
  if (libraryIds.length === 1) return [{ libraryId: { operator: 'is', value: libraryIds[0] } }]
  return [{ anyOf: libraryIds.map((id) => ({ libraryId: { operator: 'is', value: id } }) as SearchCondition) }]
}

// parity with the legacy komga UI: the section covers only the last month of releases
function oneMonthAgo(): string {
  const d = new Date()
  d.setMonth(d.getMonth() - 1)
  return d.toISOString()
}

const bookRow = (b: BookDto) => <BookRowCard book={b} />
const bookGrid = (b: BookDto) => <BookCard book={b} showSeries />
const seriesRow = (s: SeriesDto) => <SeriesRowCard series={s} />
const seriesGrid = (s: SeriesDto) => <SeriesCard series={s} />

export const DASHBOARD_SECTIONS = {
  'keep-reading': bookSection({
    titleKey: 'sections.keepReading',
    fetchPage: (libraryIds, page) =>
      booksApi.list({
        search: {
          condition: {
            allOf: [{ readStatus: { operator: 'is', value: 'IN_PROGRESS' } }, ...libraryConditions(libraryIds)],
          },
        },
        page,
        size: PAGE_SIZE,
        sort: ['readProgress.readDate,desc'],
      }),
    renderRow: (b) => <KeepReadingCard book={b} className="snap-start" />,
    renderGrid: bookGrid,
    renderSkeleton: () => <HorizontalCardSkeleton />,
  }),
  'on-deck': bookSection({
    titleKey: 'sections.onDeck',
    fetchPage: (libraryIds, page) =>
      booksApi.ondeck({ libraryId: libraryIds, page, size: PAGE_SIZE }),
    renderRow: bookRow,
    renderGrid: bookGrid,
  }),
  'recently-released-books': bookSection({
    titleKey: 'sections.recentlyReleasedBooks',
    fetchPage: (libraryIds, page) =>
      booksApi.list({
        search: {
          condition: {
            allOf: [{ releaseDate: { operator: 'after', dateTime: oneMonthAgo() } }, ...libraryConditions(libraryIds)],
          },
        },
        page,
        size: PAGE_SIZE,
        sort: ['metadata.releaseDate,desc'],
      }),
    renderRow: bookRow,
    renderGrid: bookGrid,
  }),
  'recently-added-books': bookSection({
    titleKey: 'sections.recentlyAddedBooks',
    fetchPage: (libraryIds, page) => {
      const conditions = libraryConditions(libraryIds)
      return booksApi.list({
        search: conditions.length ? { condition: { allOf: conditions } } : {},
        page,
        size: PAGE_SIZE,
        sort: ['createdDate,desc'],
      })
    },
    renderRow: bookRow,
    renderGrid: bookGrid,
  }),
  'recently-added-series': seriesSection({
    titleKey: 'sections.recentlyAddedSeries',
    fetchPage: (libraryIds, page) =>
      seriesApi.new({ libraryId: libraryIds, oneshot: false, page, size: PAGE_SIZE }),
    renderRow: seriesRow,
    renderGrid: seriesGrid,
  }),
  'recently-updated-series': seriesSection({
    titleKey: 'sections.recentlyUpdatedSeries',
    fetchPage: (libraryIds, page) =>
      seriesApi.updated({ libraryId: libraryIds, oneshot: false, page, size: PAGE_SIZE }),
    renderRow: seriesRow,
    renderGrid: seriesGrid,
  }),
  'recently-read': bookSection({
    titleKey: 'sections.recentlyRead',
    fetchPage: (libraryIds, page) =>
      booksApi.list({
        search: {
          condition: {
            allOf: [{ readStatus: { operator: 'is', value: 'READ' } }, ...libraryConditions(libraryIds)],
          },
        },
        page,
        size: PAGE_SIZE,
        sort: ['readProgress.readDate,desc'],
      }),
    renderRow: bookRow,
    renderGrid: bookGrid,
  }),
} satisfies Record<string, DashboardSectionDef>

export type DashboardSectionKey = keyof typeof DASHBOARD_SECTIONS
