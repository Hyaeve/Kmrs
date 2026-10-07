import type { BookDto } from '@/lib/api/types'
import { mediaIssue } from '@/lib/utils/mediaStatus'

/** Reader route for a book, or null when the web reader cannot display it (see mediaIssue). */
export function readRoute(book: Pick<BookDto, 'id' | 'media' | 'deleted'>): string | null {
  if (mediaIssue(book)) return null
  return `/book/${book.id}/read`
}

/** Detail route for a book; oneshots live on the oneshot page keyed by series. */
export function bookDetailRoute(book: Pick<BookDto, 'id' | 'seriesId' | 'oneshot'>): string {
  return book.oneshot ? `/oneshot/${book.seriesId}` : `/book/${book.id}`
}
