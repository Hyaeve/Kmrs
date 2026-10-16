import { useEffect, useId, useMemo, useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { keepPreviousData, useQuery } from '@tanstack/react-query'
import { Plus, X } from '@phosphor-icons/react'
import { referentialApi } from '@/lib/api/referential'
import { cn } from '@/lib/utils/cn'
import { entityKindLabel, KNOWN_ENTITY_KINDS } from '@/lib/utils/notableEntities'
import { Button } from '@/components/ui/Button'
import { IconButton } from '@/components/ui/IconButton'
import { Menu, MenuItem, MenuSeparator } from '@/components/ui/Menu'
import { TextInput, type Pair, type PairErrors } from './fields'

/**
 * Per-kind editor for notable entities (komga parity): one combobox per kind, new kinds from
 * the existing kinds or free text. The flat `pairs` (a = kind, b = name) stays the source of
 * truth so server violation indices and the submit mapping keep working unchanged.
 */
export function NotableEntitiesEditor({
  label,
  pairs,
  onChange,
  errors,
  trailing,
}: {
  label: string
  pairs: Pair[]
  onChange: (pairs: Pair[]) => void
  errors?: PairErrors
  /** rendered at the right of the label row (e.g. a LockToggle) */
  trailing?: ReactNode
}) {
  const { t } = useTranslation('metadata')
  const baseId = useId()
  // captured on mount so the group order does not reshuffle while editing; a kind whose last
  // name is removed keeps its (empty) group until the tab is left, matching komga
  const [kinds, setKinds] = useState<string[]>(() => {
    const out: string[] = []
    for (const p of pairs) if (!out.includes(p.a)) out.push(p.a)
    return out
  })

  const setKindNames = (kind: string, names: string[]) =>
    onChange([...pairs.filter((p) => p.a !== kind), ...names.map((n) => ({ a: kind, b: n }))])

  const errorsByKind = useMemo(() => {
    const out = new Map<string, string[]>()
    for (const [i, row] of Object.entries(errors ?? {})) {
      const kind = pairs[Number(i)]?.a
      if (kind === undefined) continue
      const messages = [row.a, row.b].filter((m): m is string => !!m)
      if (messages.length > 0) out.set(kind, [...(out.get(kind) ?? []), ...messages])
    }
    return out
  }, [errors, pairs])

  return (
    <div className="flex flex-col gap-3">
      <div className="relative flex items-center justify-between gap-2">
        <span className="text-[13px] font-medium text-ink-2">{label}</span>
        <div className="flex items-center gap-1">
          <AddKindControl existing={kinds} onAdd={(k) => setKinds((prev) => [...prev, k])} />
          {trailing}
        </div>
      </div>
      {kinds.length === 0 && <p className="text-sm text-ink-3">{t('entities.empty')}</p>}
      {kinds.map((kind, i) => (
        <div key={kind}>
          <label htmlFor={`${baseId}-${i}`} className="mb-1 block text-xs text-ink-3">
            {entityKindLabel(kind)}
          </label>
          <KindNamesCombobox
            inputId={`${baseId}-${i}`}
            kind={kind}
            values={pairs.filter((p) => p.a === kind).map((p) => p.b)}
            onChange={(names) => setKindNames(kind, names)}
          />
          {errorsByKind.get(kind)?.map((m, j) => (
            <p key={j} className="mt-1 text-[13px] text-danger">
              {m}
            </p>
          ))}
        </div>
      ))}
    </div>
  )
}

/** Multi-value input for the names of one kind; typing searches the server for existing names. */
function KindNamesCombobox({
  inputId,
  kind,
  values,
  onChange,
}: {
  inputId: string
  kind: string
  values: string[]
  onChange: (names: string[]) => void
}) {
  const { t } = useTranslation('metadata')
  const listId = useId()
  const [search, setSearch] = useState('')
  const [open, setOpen] = useState(false)
  // -1 = no suggestion highlighted, Enter then accepts the typed text (free create)
  const [highlight, setHighlight] = useState(-1)
  // debounce keeps the server-side search from firing per keystroke
  const [term, setTerm] = useState('')
  useEffect(() => {
    const id = setTimeout(() => setTerm(search.trim()), 300)
    return () => clearTimeout(id)
  }, [search])

  const namesQuery = useQuery({
    queryKey: ['referential', 'notableEntityNames', kind, term],
    queryFn: () => referentialApi.notableEntityNames(kind, term),
    enabled: term.length > 0,
    placeholderData: keepPreviousData,
    staleTime: 60_000,
  })
  const suggestions = useMemo(
    () => (term ? (namesQuery.data ?? []).filter((n) => !values.includes(n)) : []),
    [namesQuery.data, values, term],
  )
  const showList = open && term.length > 0
  const noMatch = showList && !namesQuery.isPending && suggestions.length === 0

  const add = (name: string) => {
    const v = name.trim()
    setSearch('')
    setHighlight(-1)
    if (v && !values.includes(v)) onChange([...values, v])
  }

  return (
    <div>
      <div className="relative">
        <TextInput
          id={inputId}
          role="combobox"
          aria-expanded={showList}
          aria-controls={listId}
          aria-autocomplete="list"
          autoComplete="off"
          value={search}
          placeholder={t('placeholder.entityName')}
          onFocus={() => setOpen(true)}
          onBlur={() => setOpen(false)}
          onChange={(e) => {
            setSearch(e.target.value)
            setHighlight(-1)
          }}
          onKeyDown={(e) => {
            if (e.key === 'Enter') {
              // the form would submit without preventing the default
              e.preventDefault()
              add((highlight >= 0 && suggestions[highlight]) || search)
            } else if (e.key === 'Escape') {
              setSearch('')
              setHighlight(-1)
            } else if (e.key === 'ArrowDown' || e.key === 'ArrowUp') {
              if (suggestions.length === 0) return
              e.preventDefault()
              setOpen(true)
              setHighlight((h) => {
                if (e.key === 'ArrowDown') return h + 1 >= suggestions.length ? 0 : h + 1
                return h <= 0 ? suggestions.length - 1 : h - 1
              })
            } else if (e.key === 'Backspace' && search === '' && values.length > 0) {
              onChange(values.slice(0, -1))
            }
          }}
        />
        {showList && (
          <div
            id={listId}
            role="listbox"
            // keep the input focused while an option is clicked
            onMouseDown={(e) => e.preventDefault()}
            className="absolute inset-x-0 top-full z-10 mt-1 max-h-56 overflow-y-auto rounded-lg border border-line bg-raised p-1 shadow-pop"
          >
            {namesQuery.isPending && <p className="px-2.5 py-1.5 text-sm text-ink-3">{t('common:state.loading')}</p>}
            {suggestions.map((n, i) => (
              <button
                key={n}
                type="button"
                role="option"
                aria-selected={i === highlight}
                onMouseEnter={() => setHighlight(i)}
                onClick={() => add(n)}
                className={cn(
                  'block w-full cursor-pointer rounded-md px-2.5 py-1.5 text-left text-sm',
                  i === highlight ? 'bg-accent-soft text-accent-strong' : 'text-ink hover:bg-overlay',
                )}
              >
                {n}
              </button>
            ))}
            {noMatch && (
              <button
                type="button"
                onClick={() => add(search)}
                className="block w-full cursor-pointer rounded-md px-2.5 py-1.5 text-left text-sm text-ink hover:bg-overlay"
              >
                {t('entities.noMatch', { search: term })}
              </button>
            )}
          </div>
        )}
      </div>
      {values.length > 0 && (
        <div className="mt-2 flex flex-wrap gap-1.5">
          {values.map((v) => (
            <span
              key={v}
              className="inline-flex items-center gap-1 rounded-full border border-line bg-raised py-1 pr-1.5 pl-3 text-xs text-ink-2"
            >
              {v}
              <button
                type="button"
                onClick={() => onChange(values.filter((x) => x !== v))}
                aria-label={t('entities.removeName', { name: v })}
                className="cursor-pointer rounded-full p-0.5 text-ink-3 transition-colors hover:bg-overlay hover:text-ink"
              >
                <X className="size-3" />
              </button>
            </span>
          ))}
        </div>
      )}
    </div>
  )
}

/** "Add kind" menu plus an inline free-text row for custom kinds (a dialog would nest inside the edit dialog). */
function AddKindControl({ existing, onAdd }: { existing: string[]; onAdd: (kind: string) => void }) {
  const { t } = useTranslation('metadata')
  const [customOpen, setCustomOpen] = useState(false)
  const [custom, setCustom] = useState('')
  const kindsQuery = useQuery({
    queryKey: ['referential', 'notableEntityKinds'],
    queryFn: () => referentialApi.notableEntityKinds(),
    staleTime: 60_000,
  })
  const otherKinds = useMemo(() => {
    const defined = new Set([...(kindsQuery.data ?? []), ...KNOWN_ENTITY_KINDS])
    return [...defined].filter((k) => !existing.includes(k))
  }, [kindsQuery.data, existing])

  const confirmCustom = () => {
    const v = custom.trim()
    if (v && !existing.includes(v)) onAdd(v)
    setCustom('')
    setCustomOpen(false)
  }

  return (
    <>
      <Menu
        align="end"
        trigger={
          <Button type="button" size="sm">
            <Plus className="size-4" />
            {t('add.kind')}
          </Button>
        }
      >
        {otherKinds.map((k) => (
          <MenuItem key={k} onSelect={() => onAdd(k)}>
            {entityKindLabel(k)}
          </MenuItem>
        ))}
        {otherKinds.length > 0 && <MenuSeparator />}
        <MenuItem onSelect={() => setCustomOpen(true)}>
          <Plus className="size-4" />
          {t('entities.customKind')}
        </MenuItem>
      </Menu>
      {customOpen && (
        // mousedown prevention keeps the input focused when the add button is clicked, so
        // onBlur only fires when focus really leaves the row
        <div
          onMouseDown={(e) => e.preventDefault()}
          className="absolute inset-x-0 top-full z-10 mt-1 flex items-center gap-1 rounded-lg border border-line bg-raised p-1.5 shadow-pop"
        >
          <TextInput
            autoFocus
            value={custom}
            placeholder={t('entities.customKindLabel')}
            aria-label={t('entities.customKindLabel')}
            onChange={(e) => setCustom(e.target.value)}
            onBlur={() => {
              setCustom('')
              setCustomOpen(false)
            }}
            onKeyDown={(e) => {
              if (e.key === 'Enter') {
                e.preventDefault()
                confirmCustom()
              } else if (e.key === 'Escape') {
                setCustom('')
                setCustomOpen(false)
              }
            }}
            className="h-8 text-sm"
          />
          <IconButton label={t('common:action.add')} disabled={!custom.trim()} onClick={confirmCustom} className="size-8">
            <Plus className="size-4" />
          </IconButton>
        </div>
      )}
    </>
  )
}
