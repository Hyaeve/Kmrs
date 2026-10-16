import { useParams } from 'react-router-dom'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { librariesApi } from '@/lib/api/libraries'
import { useDocumentTitle } from '@/lib/hooks/useDocumentTitle'
import { HistoryBackButton } from '@/components/ui/BackButton'
import { PageHeader } from '@/components/ui/PageHeader'
import { BrowseTabs } from '@/components/browse/BrowseTabs'
import { BooksGrid } from '@/components/browse/BooksGrid'

export function LibraryBooksPage() {
  const { t } = useTranslation('browse')
  const { libraryId = '' } = useParams()
  const { data: libraries } = useQuery({ queryKey: ['libraries'], queryFn: librariesApi.list })
  const library = libraries?.find((l) => l.id === libraryId)
  const title = library?.name ?? t('libraryFallback')

  useDocumentTitle(title)

  return (
    <div>
      <HistoryBackButton to="/dashboard" className="mb-2 -ml-2" />
      <PageHeader title={title} />
      <BrowseTabs tab="books" />
      <BooksGrid libraryId={libraryId} />
    </div>
  )
}
