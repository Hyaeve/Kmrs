import type { BookDto, SeriesDto } from '@/lib/api/types'
import { BookCard } from '@/components/media/BookCard'
import { SeriesCard } from '@/components/media/SeriesCard'
import { useDensityCardWidth } from '@/lib/store/ui'

export function BookRowCard({ book }: { book: BookDto }) {
  const width = useDensityCardWidth()
  return (
    <div className="shrink-0 snap-start" style={{ width }}>
      <BookCard book={book} showSeries />
    </div>
  )
}

export function SeriesRowCard({ series }: { series: SeriesDto }) {
  const width = useDensityCardWidth()
  return (
    <div className="shrink-0 snap-start" style={{ width }}>
      <SeriesCard series={series} />
    </div>
  )
}
