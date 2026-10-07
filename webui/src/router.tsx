import { Suspense } from 'react'
import { createBrowserRouter, Navigate } from 'react-router-dom'
import { RequireAuth } from '@/components/layout/RequireAuth'
import { RequireAdmin } from '@/components/layout/RequireAdmin'
import { AppShell } from '@/components/layout/AppShell'
import { LibraryRedirect } from '@/components/layout/LibraryRedirect'
import { RootRedirect } from '@/components/layout/RootRedirect'
import { LoginPage } from '@/routes/login'
import { DashboardPage } from '@/routes/dashboard'
import { DashboardSectionPage } from '@/routes/dashboard-section'
import { BrowseSeriesPage } from '@/routes/browse/series'
import { BrowseBooksPage } from '@/routes/browse/books'
import { BrowseCollectionsPage } from '@/routes/browse/collections'
import { BrowseReadListsPage } from '@/routes/browse/readlists'
import { BrowseSmartListsPage } from '@/routes/browse/smartlists'
import { LibrarySeriesPage } from '@/routes/library/series'
import { LibraryBooksPage } from '@/routes/library/books'
import { SeriesDetailPage } from '@/routes/detail/series'
import { BookDetailPage } from '@/routes/detail/book'
import { CollectionDetailPage } from '@/routes/detail/collection'
import { ReadListDetailPage } from '@/routes/detail/readlist'
import { SmartListDetailPage } from '@/routes/detail/smartlist'
import { OneshotDetailPage } from '@/routes/detail/oneshot'
import { SearchPage } from '@/routes/search'
import { AccountProfilePage } from '@/routes/account/profile'
import { AccountSecurityPage } from '@/routes/account/security'
import { AccountApiKeysPage } from '@/routes/account/api-keys'
import { AccountReaderPage } from '@/routes/account/reader'
import { AccountStatsPage } from '@/routes/account/stats'
import { AdminLibrariesPage } from '@/routes/admin/libraries'
import { AdminUsersPage } from '@/routes/admin/users'
import { AdminSettingsPage } from '@/routes/admin/settings'
import { AdminIntegrationsPage } from '@/routes/admin/integrations'
import { AdminKomfPage } from '@/routes/admin/komf'
import { AdminServerPage } from '@/routes/admin/server'
import { AdminDuplicatesPage } from '@/routes/admin/duplicates'
import { AdminDuplicatePagesPage } from '@/routes/admin/duplicate-pages'
import { AdminMediaAnalysisPage } from '@/routes/admin/media-analysis'
import { AdminMissingPostersPage } from '@/routes/admin/missing-posters'
import { AdminHistoryPage } from '@/routes/admin/history'
import { AdminUpdatesPage } from '@/routes/admin/updates'
import { AdminUiPage } from '@/routes/admin/ui'
import { ImportBooksPage } from '@/routes/import/books'
import { ImportReadListPage } from '@/routes/import/readlist'
import { NotFoundPage } from '@/routes/not-found'
import { ReaderSplash } from '@/components/reader/ReaderSplash'
import { ReaderPage } from '@/routes/read-lazy'

export const router = createBrowserRouter([
  { path: '/login', element: <LoginPage /> },
  {
    element: <RequireAuth />,
    children: [
      {
        path: '/book/:bookId/read',
        element: (
          <Suspense fallback={<ReaderSplash />}>
            <ReaderPage />
          </Suspense>
        ),
      },
      {
        element: <AppShell />,
        children: [
          { path: '/', element: <RootRedirect /> },
          { path: '/dashboard', element: <DashboardPage /> },
          { path: '/dashboard/sections/:sectionKey', element: <DashboardSectionPage /> },
          { path: '/libraries/:libraryId', element: <LibraryRedirect /> },
          { path: '/libraries/:libraryId/recommended', element: <DashboardPage /> },
          { path: '/libraries/:libraryId/sections/:sectionKey', element: <DashboardSectionPage /> },
          { path: '/libraries/:libraryId/series', element: <LibrarySeriesPage /> },
          { path: '/libraries/:libraryId/books', element: <LibraryBooksPage /> },
          { path: '/series', element: <BrowseSeriesPage /> },
          { path: '/books', element: <BrowseBooksPage /> },
          { path: '/collections', element: <BrowseCollectionsPage /> },
          { path: '/collections/:collectionId', element: <CollectionDetailPage /> },
          { path: '/readlists', element: <BrowseReadListsPage /> },
          { path: '/readlists/:readListId', element: <ReadListDetailPage /> },
          { path: '/smart-lists', element: <BrowseSmartListsPage /> },
          { path: '/smart-lists/:smartListId', element: <SmartListDetailPage /> },
          { path: '/series/:seriesId', element: <SeriesDetailPage /> },
          { path: '/oneshot/:seriesId', element: <OneshotDetailPage /> },
          { path: '/book/:bookId', element: <BookDetailPage /> },
          { path: '/search', element: <SearchPage /> },
          { path: '/account', element: <Navigate to="/account/profile" replace /> },
          { path: '/account/profile', element: <AccountProfilePage /> },
          { path: '/account/security', element: <AccountSecurityPage /> },
          { path: '/account/api-keys', element: <AccountApiKeysPage /> },
          { path: '/account/reader', element: <AccountReaderPage /> },
          { path: '/account/stats', element: <AccountStatsPage /> },
          {
            element: <RequireAdmin />,
            children: [
              { path: '/admin', element: <Navigate to="/admin/libraries" replace /> },
              { path: '/admin/libraries', element: <AdminLibrariesPage /> },
              { path: '/admin/users', element: <AdminUsersPage /> },
              { path: '/admin/settings', element: <AdminSettingsPage /> },
              { path: '/admin/integrations', element: <AdminIntegrationsPage /> },
              { path: '/admin/integrations/komf', element: <AdminKomfPage /> },
              { path: '/admin/server', element: <AdminServerPage /> },
              { path: '/admin/duplicates', element: <AdminDuplicatesPage /> },
              { path: '/admin/duplicate-pages', element: <AdminDuplicatePagesPage /> },
              { path: '/admin/media-analysis', element: <AdminMediaAnalysisPage /> },
              { path: '/admin/missing-posters', element: <AdminMissingPostersPage /> },
              { path: '/admin/history', element: <AdminHistoryPage /> },
              { path: '/admin/updates', element: <AdminUpdatesPage /> },
              { path: '/admin/ui', element: <AdminUiPage /> },
              { path: '/import/books', element: <ImportBooksPage /> },
              { path: '/import/readlist', element: <ImportReadListPage /> },
            ],
          },
          { path: '*', element: <NotFoundPage /> },
        ],
      },
    ],
  },
])
