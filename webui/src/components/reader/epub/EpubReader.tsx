import { useCallback, useEffect, useRef, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { CircleNotch, Warning } from '@phosphor-icons/react'
import { HttpFetcher, Link, Locator, Manifest, Publication } from '@readium/shared'
import { EpubNavigator, getScriptMode, type EpubNavigatorListeners } from '@readium/navigator'
import { booksApi } from '@/lib/api/books'
import { fontsApi } from '@/lib/api/fonts'
import type { BookDto } from '@/lib/api/types'
import { canDownload, useAuthStore } from '@/lib/store/auth'
import { useReaderSettings } from '@/lib/store/readerSettings'
import { urls } from '@/lib/utils/urls'
import { useDocumentTitle } from '@/lib/hooks/useDocumentTitle'
import { Button } from '@/components/ui/Button'
import { EmptyState } from '@/components/ui/EmptyState'
import { useWindowKeys } from '@/components/reader/keys'
import { useReaderNav } from '@/components/reader/useReaderNav'
import { alignLocatorHref, buildEpubPreferences, CJK_OPTIMAL_LINE_LENGTH, EPUB_BACKDROP, manifestEntries, positionOf, progressionLocator, resolvePositionLocator, type TocEntry } from './preferences'
import { EpubChrome } from './EpubChrome'
import { EpubSettingsPanel } from './EpubSettingsPanel'
import { EpubTocDrawer } from './EpubTocDrawer'

// stored on the progression record and shown as the device in sync clients
const DEVICE = { id: 'kmweb', name: 'kmweb' }

export function EpubReader({ book }: { book: BookDto }) {
  const bookId = book.id
  const { t } = useTranslation('reader')
  const nav = useReaderNav(bookId)
  const user = useAuthStore((s) => s.user)
  const backdrop = EPUB_BACKDROP[useReaderSettings((s) => s.epubTheme)]

  const containerRef = useRef<HTMLDivElement>(null)
  const navRef = useRef<EpubNavigator | null>(null)
  const [phase, setPhase] = useState<'loading' | 'ready' | 'error'>('loading')
  const [error, setError] = useState<string | null>(null)
  const [toc, setToc] = useState<TocEntry[]>([])
  const [landmarks, setLandmarks] = useState<TocEntry[]>([])
  const [pageList, setPageList] = useState<TocEntry[]>([])
  const [positions, setPositions] = useState<Locator[]>([])
  const [currentLocator, setCurrentLocator] = useState<Locator | null>(null)
  const [position, setPosition] = useState(1)
  const [chromeVisible, setChromeVisible] = useState(false)
  const [tocOpen, setTocOpen] = useState(false)
  const [settingsOpen, setSettingsOpen] = useState(false)

  const progressTimer = useRef<number | undefined>(undefined)
  const pendingLocator = useRef<Locator | null>(null)

  useDocumentTitle(book.metadata.title || book.name)

  // scroll mode turns pages by native scrolling; page-turn keys only apply when paginated
  const turnPage = useCallback(
    (dir: 'forward' | 'backward') => {
      if (useReaderSettings.getState().epubScroll) return
      const n = navRef.current
      if (!n) return
      if (dir === 'forward') n.goForward(true, (ok) => !ok && nav.boundaryTurn('next'))
      else n.goBackward(true, (ok) => !ok && nav.boundaryTurn('previous'))
    },
    [nav],
  )

  useEffect(() => {
    let cancelled = false
    let instance: EpubNavigator | null = null

    const flushProgress = () => {
      window.clearTimeout(progressTimer.current)
      const locator = pendingLocator.current
      pendingLocator.current = null
      if (locator && !nav.incognito) {
        booksApi
          .updateProgression(bookId, {
            modified: new Date().toISOString(),
            device: DEVICE,
            locator: progressionLocator(locator),
          })
          .catch(() => {})
      }
    }

    const scheduleProgress = (locator: Locator) => {
      if (nav.incognito) return
      pendingLocator.current = locator
      window.clearTimeout(progressTimer.current)
      progressTimer.current = window.setTimeout(flushProgress, 500)
    }

    async function setup() {
      // absolute URL: HttpFetcher resolves link hrefs against its base URL, relative paths get joined wrong
      const manifestUrl = new URL(`/api/v1/books/${bookId}/manifest`, window.location.origin).toString()
      const fetcher = new HttpFetcher(undefined, manifestUrl)
      const manifestJson = await fetcher.get(new Link({ href: manifestUrl })).readAsJSON()
      const manifest = Manifest.deserialize(manifestJson)
      if (!manifest) throw new Error('Invalid manifest')
      manifest.setSelfLink(manifestUrl)
      const publication = new Publication({ manifest, fetcher })
      const cjk = getScriptMode(manifest.metadata).startsWith('cjk')

      const rawPositions = await booksApi.positions(bookId)
      const positions = rawPositions.positions
        .map((p) => Locator.deserialize(p))
        .filter((l): l is Locator => !!l)
        .map((l) => alignLocatorHref(l, publication))
      if (positions.length === 0) throw new Error('No positions')

      let initialPosition: Locator | undefined
      if (!nav.incognito) {
        const progression = await booksApi.progression(bookId).catch(() => undefined)
        const stored = progression?.locator ? Locator.deserialize(progression.locator) : undefined
        if (stored)
          initialPosition = resolvePositionLocator(alignLocatorHref(stored, publication), positions)
      }
      // server fonts are optional; a server without the fonts API degrades to publisher fonts
      const serverFonts = await fontsApi.families().catch(() => [] as string[])
      if (cancelled) return

      // the interface marks every listener required; tap/click/handleLocator return false to keep the default behavior
      const listeners: EpubNavigatorListeners = {
        frameLoaded: () => {},
        positionChanged: (locator) => {
          setCurrentLocator(locator)
          setPosition(positionOf(locator, positions))
          scheduleProgress(locator)
        },
        timelineItemChanged: () => {},
        tap: () => false,
        click: () => false,
        zoom: () => {},
        miscPointer: () => setChromeVisible((v) => !v),
        scroll: () => {},
        customEvent: () => {},
        handleLocator: () => false,
        textSelected: () => {},
        contentProtection: () => {},
        contextMenu: () => {},
        peripheral: (data) => {
          if (data.type === 'forward') turnPage('forward')
          else if (data.type === 'backward') turnPage('backward')
        },
      }

      instance = new EpubNavigator(containerRef.current!, publication, listeners, positions, initialPosition, {
        preferences: buildEpubPreferences(useReaderSettings.getState()),
        defaults: { optimalLineLength: cjk ? CJK_OPTIMAL_LINE_LENGTH : null },
        injectables: {
          rules: [
            {
              resources: [/\.x?html$/],
              append: serverFonts.map((family) => ({
                id: `kmweb-font-${family}`,
                as: 'link' as const,
                rel: 'stylesheet',
                target: 'head' as const,
                url: fontsApi.cssUrl(family),
              })),
            },
          ],
          // url injectables are validated against this list; the fonts API is same-origin
          allowedDomains: [window.location.origin],
        },
        keyboardPeripherals: [
          { type: 'forward', keyCombos: [{ keyCode: 39 }, { keyCode: 32 }] },
          { type: 'backward', keyCombos: [{ keyCode: 37 }, { keyCode: 32, shift: true }] },
        ],
      })
      await instance.load()
      if (cancelled) {
        await instance.destroy()
        return
      }
      navRef.current = instance
      setToc(manifestEntries(manifestJson, 'toc'))
      setLandmarks(manifestEntries(manifestJson, 'landmarks'))
      setPageList(manifestEntries(manifestJson, 'pageList'))
      setPositions(positions)
      setPhase('ready')
    }

    setup().catch((e) => {
      if (!cancelled) {
        setError(e instanceof Error ? e.message : String(e))
        setPhase('error')
      }
    })

    return () => {
      cancelled = true
      navRef.current = null
      flushProgress()
      if (instance) void instance.destroy()
    }
    // nav.incognito and nav.boundaryTurn are stable for the lifetime of this book's reader
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [bookId])

  // live-apply preference changes (theme, layout, typography)
  useEffect(() => {
    if (phase !== 'ready') return
    return useReaderSettings.subscribe((s) => {
      navRef.current?.submitPreferences(buildEpubPreferences(s)).catch(() => {})
    })
  }, [phase])

  const goToPosition = useCallback(
    (n: number) => {
      const target = positions[Math.min(Math.max(1, n), positions.length) - 1]
      if (target) navRef.current?.go(target, true, () => {})
    },
    [positions],
  )

  const goTocHref = useCallback((href: string) => {
    navRef.current?.goLink(new Link({ href }), true, () => {})
    setTocOpen(false)
  }, [])

  useWindowKeys((e) => {
    if (e.key === 'Escape') {
      if (tocOpen) setTocOpen(false)
      else if (settingsOpen) setSettingsOpen(false)
      else if (chromeVisible) setChromeVisible(false)
      else nav.exitReader()
      return
    }
    switch (e.key) {
      case 'ArrowRight':
      case ' ':
        turnPage('forward')
        break
      case 'ArrowLeft':
        turnPage('backward')
        break
      case 'f':
        nav.toggleFullscreen()
        break
      case 'm':
        setChromeVisible((v) => !v)
        break
      case 's':
        setSettingsOpen((v) => !v)
        break
      case 't':
        setTocOpen((v) => !v)
        break
    }
  })

  if (phase === 'error') {
    return (
      <div className="fixed inset-0 flex items-center justify-center bg-bg">
        <EmptyState
          icon={<Warning weight="duotone" />}
          title={t('epub.loadFailed')}
          body={error ?? undefined}
          action={<Button onClick={nav.goToBook}>{t('error.backToBook')}</Button>}
        />
      </div>
    )
  }

  const title = book.metadata.title || book.name
  const number = book.metadata.number
  const pill = number ? `#${number} - ${title} / ${book.seriesTitle}` : `${title} / ${book.seriesTitle}`
  const currentHref = currentLocator?.href

  const downloadFile = () => {
    const a = document.createElement('a')
    a.href = urls.bookFile(bookId)
    a.download = book.name
    a.click()
  }

  return (
    <div className="fixed inset-0 overflow-hidden select-none" style={{ backgroundColor: backdrop }}>
      {/* the navigator rewrites the container's size itself and observes its parent; mx-auto splits the slack into even side margins */}
      <div ref={containerRef} className="absolute inset-0 mx-auto" />

      {phase === 'loading' && (
        <div className="absolute inset-0 z-10 flex flex-col items-center justify-center gap-4 bg-black text-white">
          <CircleNotch className="size-8 animate-spin text-white/70" />
          <p className="max-w-md truncate px-4 text-sm text-white/70">{book.metadata.title || book.name}</p>
        </div>
      )}

      {phase === 'ready' && (
        <>
          <EpubChrome
            visible={chromeVisible}
            title={pill}
            position={position}
            positionsCount={positions.length}
            incognito={nav.incognito}
            isFullscreen={nav.isFullscreen}
            canDownloadFile={canDownload(user)}
            hasPreviousBook={!!nav.siblingPrevious}
            hasNextBook={!!nav.siblingNext}
            onClose={nav.exitReader}
            onGoToPosition={goToPosition}
            onPreviousBook={() => nav.goBook('previous')}
            onNextBook={() => nav.goBook('next')}
            onToggleToc={() => setTocOpen((v) => !v)}
            onToggleSettings={() => setSettingsOpen((v) => !v)}
            onToggleFullscreen={nav.toggleFullscreen}
            onDownload={downloadFile}
            onGoToBook={nav.goToBook}
          />

          <EpubTocDrawer
            open={tocOpen}
            toc={toc}
            landmarks={landmarks}
            pageList={pageList}
            currentHref={currentHref}
            onGo={goTocHref}
            onClose={() => setTocOpen(false)}
          />

          <EpubSettingsPanel
            open={settingsOpen}
            onAlwaysFullscreenChange={nav.onAlwaysFullscreenChange}
            onClose={() => setSettingsOpen(false)}
          />
        </>
      )}
    </div>
  )
}
