import { useEffect, useRef, useState } from 'react'
import { Link, NavLink, Outlet, useLocation, useMatch, useNavigate, useNavigationType, useSearchParams } from 'react-router-dom'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import {
  BookBookmark,
  BookmarkSimple,
  BookOpen,
  Books,
  CaretDown,
  CaretRight,
  ChartBar,
  ClockCounterClockwise,
  Copy,
  Database,
  FileMagnifyingGlass,
  ChartLineUp,
  Funnel,
  Gauge,
  GearSix,
  HardDrives,
  House,
  ImageBroken,
  Images,
  Key,
  List,
  MagnifyingGlass,
  Monitor,
  Moon,
  PaintBrush,
  Palette,
  Playlist,
  PlugsConnected,
  PushPin,
  Queue,
  Rocket,
  ShieldCheck,
  SignOut,
  Sun,
  TrayArrowDown,
  User,
  UserCircle,
  Users,
  Wrench,
  X,
} from '@phosphor-icons/react'
import { AnimatePresence, motion } from 'motion/react'
import { cn } from '@/lib/utils/cn'
import { librariesApi } from '@/lib/api/libraries'
import { usersApi } from '@/lib/api/users'
import { serverApi } from '@/lib/api/users'
import { isAdmin, useAuthStore } from '@/lib/store/auth'
import { useUiStore } from '@/lib/store/ui'
import { usePinnedLibraries } from '@/lib/store/clientSettings'
import { useTaskQueue } from '@/lib/store/taskQueue'
import { queryClient } from '@/lib/queryClient'
import { taskTypeLabel } from '@/components/admin/server/format'
import { LogoMark } from '@/components/LogoMark'
import { IconButton } from '@/components/ui/IconButton'
import { Tooltip } from '@/components/ui/Tooltip'
import { Menu, MenuItem, MenuLabel, MenuSeparator } from '@/components/ui/Menu'
import { SegmentedControl } from '@/components/ui/SegmentedControl'
import { Switch } from '@/components/ui/Switch'
import { Skeleton } from '@/components/ui/Skeleton'
import { PinLibrariesDialog } from '@/components/layout/PinLibrariesDialog'
import { SidebarLibraryMenu } from '@/components/layout/SidebarLibraryMenu'
import { SearchScopeMenu } from '@/components/search/SearchScopeMenu'
import { useSearchScopeDefault } from '@/components/search/scope'
import { useChanged } from '@/lib/hooks/useChanged'

function NavItem({
  to,
  icon,
  children,
  end,
  onClick,
  trailing,
  actions,
}: {
  to: string
  icon: React.ReactNode
  children: React.ReactNode
  end?: boolean
  onClick?: () => void
  trailing?: React.ReactNode
  /** overlay controls (e.g. the admin actions menu); rendered as siblings of the
      link so no interactive element nests inside the <a> */
  actions?: React.ReactNode
}) {
  const link = (
    <NavLink
      to={to}
      end={end}
      onClick={onClick}
      className={({ isActive }) =>
        cn(
          'flex items-center gap-3 rounded-lg px-3 py-2 text-[13.5px] transition-colors duration-150',
          isActive ? 'bg-accent-soft font-medium text-accent-strong' : 'text-ink-2 hover:bg-raised hover:text-ink',
          actions && 'pr-9',
        )
      }
    >
      <span className="[&_svg]:size-[18px]">{icon}</span>
      <span className="min-w-0 flex-1 truncate">{children}</span>
      {trailing}
    </NavLink>
  )
  if (!actions) return link
  return (
    <div className="group relative">
      {link}
      <div className="absolute inset-y-0 right-2 flex items-center">{actions}</div>
    </div>
  )
}

function NavGroup({
  icon,
  label,
  match,
  children,
}: {
  icon: React.ReactNode
  label: string
  /** path prefixes that belong to the group; navigating into one force-opens it */
  match: string[]
  children: React.ReactNode
}) {
  const { pathname } = useLocation()
  const containsActive = match.some((m) => pathname.startsWith(m))
  const [open, setOpen] = useState(containsActive)

  if (useChanged([containsActive]) && containsActive) setOpen(true)

  return (
    <div>
      <button
        type="button"
        onClick={() => setOpen((v) => !v)}
        className={cn(
          'flex w-full cursor-pointer items-center gap-3 rounded-lg px-3 py-2 text-[13.5px] transition-colors duration-150',
          containsActive ? 'font-medium text-ink' : 'text-ink-2 hover:bg-raised hover:text-ink',
        )}
      >
        <span className="[&_svg]:size-[18px]">{icon}</span>
        <span className="min-w-0 flex-1 truncate text-left">{label}</span>
        <CaretRight className={cn('size-3.5 text-ink-3 transition-transform duration-150', open && 'rotate-90')} />
      </button>
      {open && <div className="mt-0.5 ml-[21px] flex flex-col gap-0.5 border-l border-line pl-1.5">{children}</div>}
    </div>
  )
}

function SidebarContent({ onNavigate }: { onNavigate?: () => void }) {
  const { t } = useTranslation('layout')
  const { data: libraries, isLoading } = useQuery({ queryKey: ['libraries'], queryFn: librariesApi.list })
  const user = useAuthStore((s) => s.user)
  const { pinned } = usePinnedLibraries()
  const [pinDialogOpen, setPinDialogOpen] = useState(false)
  const librariesCollapsed = useUiStore((s) => s.sidebarLibrariesCollapsed)
  const setLibrariesCollapsed = useUiStore((s) => s.setSidebarLibrariesCollapsed)

  const pinnedIndex = new Map((pinned ?? []).map((id, i) => [id, i]))
  const pinRank = (id: string) => pinnedIndex.get(id) ?? Number.MAX_SAFE_INTEGER
  const sortedLibraries = libraries?.slice().sort((a, b) => pinRank(a.id) - pinRank(b.id))

  return (
    <div className="flex h-full flex-col">
      <div className="flex h-24 items-center justify-center">
        <LogoMark className="size-12" />
      </div>

      <nav className="flex min-h-0 flex-1 flex-col gap-0.5 overflow-y-auto px-3 py-3">
        <NavItem to="/dashboard" icon={<House />} onClick={onNavigate}>
          {t('nav.dashboard')}
        </NavItem>
        <NavItem to="/search" icon={<MagnifyingGlass />} onClick={onNavigate}>
          {t('common:action.search')}
        </NavItem>

        <div className="mt-5 mb-1.5 flex items-center justify-between px-3">
          <button
            type="button"
            onClick={() => setLibrariesCollapsed(!librariesCollapsed)}
            className="flex cursor-pointer items-center gap-1 text-[11px] font-medium tracking-[0.08em] text-ink-3 uppercase transition-colors hover:text-ink-2"
          >
            {t('nav.libraries')}
            <CaretRight
              className={cn('size-3 transition-transform duration-150', !librariesCollapsed && 'rotate-90')}
            />
          </button>
          {libraries && libraries.length > 0 && (
            <IconButton
              label={t('nav.managePinned')}
              className="-my-1 -mr-[5px] size-6 rounded-md [&_svg]:size-3.5"
              onClick={() => setPinDialogOpen(true)}
            >
              <PushPin />
            </IconButton>
          )}
        </div>
        {!librariesCollapsed && (
          <>
            {isLoading && (
              <div className="flex flex-col gap-2 px-3">
                <Skeleton className="h-6 w-3/4" />
                <Skeleton className="h-6 w-1/2" />
              </div>
            )}
            {sortedLibraries?.map((lib) => (
              <NavItem
                key={lib.id}
                to={`/libraries/${lib.id}/series`}
                icon={<BookBookmark />}
                onClick={onNavigate}
                trailing={
                  pinnedIndex.has(lib.id) ? (
                    <PushPin weight="fill" className="size-3 shrink-0 text-ink-3" aria-label={t('nav.pinned')} />
                  ) : undefined
                }
                actions={isAdmin(user) ? <SidebarLibraryMenu library={lib} /> : undefined}
              >
                {lib.name}
              </NavItem>
            ))}
            {libraries?.length === 0 && <p className="px-3 text-[13px] text-ink-3">{t('nav.noLibraries')}</p>}
          </>
        )}

        <div className="mt-5 mb-1.5 px-3 text-[11px] font-medium tracking-[0.08em] text-ink-3 uppercase">
          {t('nav.curated')}
        </div>
        <NavItem to="/collections" icon={<Books />} onClick={onNavigate}>
          {t('nav.collections')}
        </NavItem>
        <NavItem to="/readlists" icon={<BookmarkSimple />} onClick={onNavigate}>
          {t('nav.readLists')}
        </NavItem>
        <NavItem to="/smart-lists" icon={<Funnel />} onClick={onNavigate}>
          {t('nav.smartLists')}
        </NavItem>

        {isAdmin(user) && (
          <>
            <div className="mt-5 mb-1.5 px-3 text-[11px] font-medium tracking-[0.08em] text-ink-3 uppercase">
              {t('nav.import')}
            </div>
            <NavItem to="/import/books" icon={<TrayArrowDown />} onClick={onNavigate}>
              {t('nav.importBooks')}
            </NavItem>
            <NavItem to="/import/readlist" icon={<Playlist />} onClick={onNavigate}>
              {t('nav.importReadlist')}
            </NavItem>

            <div className="mt-5 mb-1.5 px-3 text-[11px] font-medium tracking-[0.08em] text-ink-3 uppercase">
              {t('nav.administration')}
            </div>
            <NavGroup
              icon={<Database />}
              label={t('nav.server')}
              match={['/admin/libraries', '/admin/users', '/admin/settings', '/admin/integrations', '/admin/server', '/admin/updates', '/admin/ui']}
            >
              <NavItem to="/admin/libraries" icon={<HardDrives />} onClick={onNavigate}>
                {t('nav.libraries')}
              </NavItem>
              <NavItem to="/admin/users" icon={<Users />} onClick={onNavigate}>
                {t('nav.users')}
              </NavItem>
              <NavItem to="/admin/settings" icon={<GearSix />} onClick={onNavigate}>
                {t('nav.settings')}
              </NavItem>
              <NavItem to="/admin/integrations" icon={<PlugsConnected />} onClick={onNavigate}>
                {t('nav.integrations')}
              </NavItem>
              <NavItem to="/admin/server" icon={<Gauge />} onClick={onNavigate}>
                {t('nav.server')}
              </NavItem>
              <NavItem to="/admin/updates" icon={<Rocket />} onClick={onNavigate}>
                {t('nav.updates')}
              </NavItem>
              <NavItem to="/admin/ui" icon={<Palette />} onClick={onNavigate}>
                {t('nav.uiSettings')}
              </NavItem>
            </NavGroup>
            <NavGroup
              icon={<Wrench />}
              label={t('nav.maintenance')}
              match={['/admin/duplicates', '/admin/duplicate-pages', '/admin/media-analysis', '/admin/missing-posters', '/admin/history']}
            >
              <NavItem to="/admin/duplicates" icon={<Copy />} onClick={onNavigate}>
                {t('nav.duplicates')}
              </NavItem>
              <NavItem to="/admin/duplicate-pages" icon={<Images />} onClick={onNavigate}>
                {t('nav.duplicatePages')}
              </NavItem>
              <NavItem to="/admin/media-analysis" icon={<FileMagnifyingGlass />} onClick={onNavigate}>
                {t('nav.mediaAnalysis')}
              </NavItem>
              <NavItem to="/admin/missing-posters" icon={<ImageBroken />} onClick={onNavigate}>
                {t('nav.missingPosters')}
              </NavItem>
              <NavItem to="/admin/history" icon={<ClockCounterClockwise />} onClick={onNavigate}>
                {t('nav.history')}
              </NavItem>
            </NavGroup>
          </>
        )}

        <div className="mt-3">
          <NavGroup icon={<UserCircle />} label={t('nav.account')} match={['/account']}>
            <NavItem to="/account/profile" icon={<User />} onClick={onNavigate}>
              {t('nav.profile')}
            </NavItem>
            <NavItem to="/account/security" icon={<ShieldCheck />} onClick={onNavigate}>
              {t('nav.security')}
            </NavItem>
            <NavItem to="/account/api-keys" icon={<Key />} onClick={onNavigate}>
              {t('nav.apiKeys')}
            </NavItem>
            <NavItem to="/account/reader" icon={<BookOpen />} onClick={onNavigate}>
              {t('nav.reader')}
            </NavItem>
            <NavItem to="/account/stats" icon={<ChartBar />} onClick={onNavigate}>
              {t('nav.statistics')}
            </NavItem>
            <NavItem to="/account/trackers" icon={<ChartLineUp />} onClick={onNavigate}>
              {t('nav.trackers')}
            </NavItem>
          </NavGroup>
        </div>
      </nav>

      <PinLibrariesDialog open={pinDialogOpen} onOpenChange={setPinDialogOpen} />
      <SidebarFooter />
    </div>
  )
}

function SidebarFooter() {
  const { t } = useTranslation('layout')
  const user = useAuthStore((s) => s.user)
  const clear = useAuthStore((s) => s.clear)
  const navigate = useNavigate()
  const { data: info } = useQuery({ queryKey: ['server-info'], queryFn: serverApi.info, staleTime: Infinity })

  const logout = async () => {
    try {
      await usersApi.logout()
    } finally {
      clear()
      queryClient.clear()
      navigate('/login')
    }
  }

  return (
    <div className="border-t border-line p-3">
      <Menu
        align="start"
        side="top"
        matchTriggerWidth
        trigger={
          <button className="flex w-full cursor-pointer items-center gap-2.5 rounded-lg px-2 py-2 text-left transition-colors hover:bg-raised">
            <span className="flex size-8 shrink-0 items-center justify-center rounded-full bg-accent-soft text-sm font-semibold text-accent-strong">
              {user?.email?.[0]?.toUpperCase() ?? '?'}
            </span>
            <span className="min-w-0 flex-1 truncate text-[13px] text-ink-2">{user?.email}</span>
            <CaretDown className="size-3.5 text-ink-3" />
          </button>
        }
      >
        <MenuLabel>{user?.email}</MenuLabel>
        <MenuItem onSelect={logout}>
          <SignOut className="size-4" /> {t('nav.signOut')}
        </MenuItem>
        {info?.build?.version && (
          <>
            <MenuSeparator />
            <MenuLabel>kmrs {info.build.version}</MenuLabel>
          </>
        )}
      </Menu>
    </div>
  )
}

function useSlashFocus(ref: React.RefObject<HTMLInputElement | null>) {
  useEffect(() => {
    const onKey = (e: globalThis.KeyboardEvent) => {
      if (e.key === '/' && !(e.target instanceof HTMLInputElement) && !(e.target instanceof HTMLTextAreaElement)) {
        e.preventDefault()
        ref.current?.focus()
      }
    }
    window.addEventListener('keydown', onKey)
    return () => window.removeEventListener('keydown', onKey)
  }, [ref])
}

function TopSearchBox() {
  const { t } = useTranslation('search')
  const navigate = useNavigate()
  const { value: scope } = useSearchScopeDefault()
  const [q, setQ] = useState('')
  const inputRef = useRef<HTMLInputElement>(null)
  useSlashFocus(inputRef)

  return (
    <form
      className="relative w-full max-w-md min-w-0"
      onSubmit={(e) => {
        e.preventDefault()
        const trimmed = q.trim()
        if (!trimmed) return
        const params = new URLSearchParams({ q: trimmed })
        if (scope !== 'all') params.set('scope', scope)
        navigate(`/search?${params}`)
      }}
    >
      <MagnifyingGlass className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-ink-3" />
      <input
        ref={inputRef}
        type="search"
        value={q}
        onChange={(e) => setQ(e.target.value)}
        placeholder={t('placeholderTop')}
        className="h-9 w-full rounded-lg border border-line bg-surface pr-12 pl-9 text-base text-ink transition-colors placeholder:text-ink-3 focus:border-accent/70 focus:outline-none"
      />
      <kbd className="pointer-events-none absolute top-1/2 right-3 -translate-y-1/2 rounded border border-line bg-raised px-1.5 text-[11px] text-ink-3">
        /
      </kbd>
    </form>
  )
}

// on /search the header box is the page's only search input: it edits the ?q=
// param live so page and box can never diverge
function SyncedSearchBox() {
  const { t } = useTranslation('search')
  const [searchParams, setSearchParams] = useSearchParams()
  const qRaw = searchParams.get('q') ?? ''
  const [input, setInput] = useState(qRaw)
  // last value this field pushed to the URL; divergence means the change came
  // from outside (back/forward) and must be synced back in
  const lastPushed = useRef(qRaw)
  const inputRef = useRef<HTMLInputElement>(null)
  useSlashFocus(inputRef)

  useEffect(() => {
    if (qRaw !== lastPushed.current) {
      lastPushed.current = qRaw
      setInput(qRaw)
    }
  }, [qRaw])

  useEffect(() => {
    const v = input.trim()
    if (v === lastPushed.current.trim()) return
    const timer = setTimeout(() => {
      lastPushed.current = v
      setSearchParams(
        (prev) => {
          const next = new URLSearchParams(prev)
          if (v) next.set('q', v)
          else next.delete('q')
          return next
        },
        { replace: true },
      )
    }, 400)
    return () => clearTimeout(timer)
  }, [input, setSearchParams])

  return (
    <div className="relative w-full max-w-lg min-w-0">
      <MagnifyingGlass className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-ink-3" />
      <input
        ref={inputRef}
        type="search"
        autoFocus
        value={input}
        onChange={(e) => setInput(e.target.value)}
        placeholder={t('placeholderPage')}
        aria-label={t('common:action.search')}
        className="h-9 w-full rounded-lg border border-line bg-surface pr-12 pl-9 text-base text-ink transition-colors placeholder:text-ink-3 focus:border-accent/70 focus:outline-none"
      />
      {input ? (
        <button
          type="button"
          aria-label={t('clear')}
          onClick={() => setInput('')}
          className="absolute top-1/2 right-3 -translate-y-1/2 cursor-pointer text-ink-3 transition-colors hover:text-ink"
        >
          <X className="size-4" />
        </button>
      ) : (
        <kbd className="pointer-events-none absolute top-1/2 right-3 -translate-y-1/2 rounded border border-line bg-raised px-1.5 text-[11px] text-ink-3">
          /
        </kbd>
      )}
    </div>
  )
}

// TaskQueueStatus is only pushed on the admin stream, so non-admins keep a
// zero count and the badge never renders for them.
function TaskQueueBadge() {
  const { t } = useTranslation('layout')
  const status = useTaskQueue((s) => s.status)
  const count = status?.count ?? 0
  if (count === 0) return null
  const entries = Object.entries(status?.countByType ?? {}).sort((a, b) => b[1] - a[1])
  const label = t('queuedTasks', { count })
  return (
    <Tooltip
      side="bottom"
      content={
        <div className="flex flex-col gap-0.5">
          <div className="font-medium">{label}</div>
          {entries.map(([type, n]) => (
            <div key={type} className="flex items-center justify-between gap-4">
              <span>{taskTypeLabel(type)}</span>
              <span className="tabular-nums">{n}</span>
            </div>
          ))}
        </div>
      }
    >
      <Link
        to="/admin/server"
        aria-label={label}
        className="inline-flex h-9 shrink-0 items-center gap-1.5 rounded-lg px-2 text-ink-2 transition-colors duration-150 hover:bg-raised hover:text-ink"
      >
        <Queue className="size-5" />
        <span className="text-[13px] font-medium tabular-nums">{count}</span>
      </Link>
    </Tooltip>
  )
}

function ThemeButton() {
  const { t } = useTranslation('layout')
  const theme = useUiStore((s) => s.theme)
  const setTheme = useUiStore((s) => s.setTheme)
  const next = theme === 'light' ? 'dark' : theme === 'dark' ? 'system' : 'light'
  return (
    <IconButton label={t('theme.current', { name: t(`theme.${theme}`) })} onClick={() => setTheme(next)}>
      {theme === 'light' ? <Sun className="size-5" /> : theme === 'dark' ? <Moon className="size-5" /> : <Monitor className="size-5" />}
    </IconButton>
  )
}

function AppearanceMenu() {
  const { t } = useTranslation('layout')
  const cardStyle = useUiStore((s) => s.cardStyle)
  const gridDensity = useUiStore((s) => s.gridDensity)
  const blurUnreadCovers = useUiStore((s) => s.blurUnreadCovers)
  const setCardStyle = useUiStore((s) => s.setCardStyle)
  const setGridDensity = useUiStore((s) => s.setGridDensity)
  const setBlurUnreadCovers = useUiStore((s) => s.setBlurUnreadCovers)

  return (
    <Menu
      trigger={
        <IconButton label={t('appearance.label')}>
          <PaintBrush className="size-5" />
        </IconButton>
      }
    >
      <div className="w-64 space-y-3 p-2.5">
        <div>
          <MenuLabel>{t('appearance.cardStyle')}</MenuLabel>
          <SegmentedControl
            size="sm"
            className="w-full [&>button]:flex-1"
            options={[
              { value: 'standard', label: t('appearance.cardStyleStandard') },
              { value: 'overlay', label: t('appearance.cardStyleOverlay') },
              { value: 'cover', label: t('appearance.cardStyleCoverOnly') },
            ]}
            value={cardStyle}
            onChange={setCardStyle}
          />
        </div>
        <div>
          <MenuLabel>{t('appearance.gridDensity')}</MenuLabel>
          <SegmentedControl
            size="sm"
            className="w-full [&>button]:flex-1"
            options={[
              { value: 'compact', label: t('appearance.densityCompact') },
              { value: 'standard', label: t('appearance.densityStandard') },
              { value: 'cozy', label: t('appearance.densityCozy') },
            ]}
            value={gridDensity}
            onChange={setGridDensity}
          />
        </div>
        <div className="flex items-center justify-between gap-3 px-2.5 pt-1 pb-1">
          <span className="text-[13px] text-ink-2">{t('appearance.blurUnread')}</span>
          <Switch
            checked={blurUnreadCovers}
            onCheckedChange={setBlurUnreadCovers}
            label={t('appearance.blurUnreadLabel')}
          />
        </div>
      </div>
    </Menu>
  )
}

export function AppShell() {
  const { t } = useTranslation('layout')
  const [drawerOpen, setDrawerOpen] = useState(false)
  const navigate = useNavigate()
  const location = useLocation()
  const onSearchPage = location.pathname.startsWith('/search')
  const navType = useNavigationType()
  // back/forward remounts the whole grid at once and replaying every card's entrance
  // reads as a flash; the previous key tells cold open (also POP) apart, and it only
  // differs from location.key on the render before the key update commits
  const [prevKey, setPrevKey] = useState<string>()
  const [popNavigation, setPopNavigation] = useState(false)
  if (prevKey !== location.key) {
    setPopNavigation(prevKey !== undefined && navType === 'POP')
    setPrevKey(location.key)
  }

  // browsing a library steers the default search scope to that library; leaving
  // snaps back to the default, unless the user picked a different scope while there
  const libraryId = useMatch('/libraries/:libraryId/*')?.params.libraryId
  const setSearchScope = useUiStore((s) => s.setSearchScope)
  const steeredFrom = useRef(libraryId)
  useEffect(() => {
    if (libraryId) setSearchScope(libraryId)
    else if (steeredFrom.current && useUiStore.getState().searchScope === steeredFrom.current) setSearchScope('pinned')
    steeredFrom.current = libraryId
  }, [libraryId, setSearchScope])

  return (
    <div className="flex min-h-dvh bg-bg">
      {/* desktop sidebar */}
      <aside className="sticky top-0 hidden h-dvh w-60 shrink-0 safe-top safe-bottom border-r border-line bg-surface lg:block">
        <SidebarContent />
      </aside>

      {/* mobile drawer */}
      <AnimatePresence>
        {drawerOpen && (
          <>
            <motion.div
              className="fixed inset-0 z-30 bg-black/60 lg:hidden"
              initial={{ opacity: 0 }}
              animate={{ opacity: 1 }}
              exit={{ opacity: 0 }}
              onClick={() => setDrawerOpen(false)}
            />
            <motion.aside
              className="fixed inset-y-0 left-0 z-30 w-64 safe-top safe-bottom border-r border-line bg-surface lg:hidden"
              initial={{ x: -280 }}
              animate={{ x: 0 }}
              exit={{ x: -280 }}
              transition={{ type: 'spring', stiffness: 380, damping: 36 }}
            >
              <SidebarContent onNavigate={() => setDrawerOpen(false)} />
            </motion.aside>
          </>
        )}
      </AnimatePresence>

      <div className="flex min-w-0 flex-1 flex-col">
        <header className="sticky top-0 z-20 flex h-15 shrink-0 box-content safe-top items-center gap-3 border-b border-line bg-bg/80 px-4 backdrop-blur-md md:px-8">
          <IconButton label={t('menu')} className="lg:hidden" onClick={() => setDrawerOpen(true)}>
            <List className="size-5" />
          </IconButton>
          {onSearchPage ? (
            <div className="flex min-w-0 flex-1 items-center gap-2">
              <SyncedSearchBox />
              <SearchScopeMenu />
            </div>
          ) : (
            <>
              <div className="hidden min-w-0 flex-1 items-center gap-2 md:flex">
                <TopSearchBox />
                <SearchScopeMenu />
              </div>
              <div className="flex-1 md:hidden" />
              <IconButton label={t('common:action.search')} className="md:hidden" onClick={() => navigate('/search')}>
                <MagnifyingGlass className="size-5" />
              </IconButton>
            </>
          )}
          <TaskQueueBadge />
          <ThemeButton />
          <AppearanceMenu />
        </header>

        <main
          data-pop-navigation={popNavigation || undefined}
          className="min-w-0 flex-1 px-4 pt-6 pb-[max(1.5rem,env(safe-area-inset-bottom))] md:px-8 md:pt-8 md:pb-[max(2rem,env(safe-area-inset-bottom))]"
        >
          <Outlet />
        </main>
      </div>
    </div>
  )
}
