import { useMemo, useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Check, CircleNotch, MagnifyingGlass } from '@phosphor-icons/react'
import type { EpubFontFamily } from '@/lib/store/readerSettings'
import { Button } from '@/components/ui/Button'
import { Dialog } from '@/components/ui/Dialog'
import { TextField } from '@/components/ui/TextField'
import { cn } from '@/lib/utils/cn'
import { isLocalFont, localFontName, localFontValue, querySystemFontFamilies } from './fonts'

interface LocalFontPickerProps {
  value: EpubFontFamily
  onSelect: (value: EpubFontFamily) => void
}

export function LocalFontPicker({ value, onSelect }: LocalFontPickerProps) {
  const { t } = useTranslation('reader')
  const [open, setOpen] = useState(false)
  const [status, setStatus] = useState<'idle' | 'loading' | 'ready' | 'manual'>('idle')
  const [canRetry, setCanRetry] = useState(false)
  const [families, setFamilies] = useState<string[]>([])
  const [search, setSearch] = useState('')
  const [manualName, setManualName] = useState('')
  const selectedName = isLocalFont(value) ? localFontName(value) : null

  const filteredFamilies = useMemo(() => {
    const query = search.trim().toLowerCase()
    if (!query) return families
    return families.filter((family) => family.toLowerCase().includes(query))
  }, [families, search])

  const loadFamilies = async () => {
    setStatus('loading')
    setCanRetry(false)
    try {
      const result = await querySystemFontFamilies()
      if (!result || result.length === 0) {
        setStatus('manual')
        return
      }
      setFamilies(result)
      setStatus('ready')
    } catch {
      setCanRetry(true)
      setStatus('manual')
    }
  }

  const openPicker = () => {
    setSearch('')
    setManualName(selectedName ?? '')
    setOpen(true)
    if (status === 'idle' || (status === 'manual' && canRetry)) void loadFamilies()
  }

  const selectFamily = (family: string) => {
    const name = family.trim()
    if (!name) return
    onSelect(localFontValue(name))
    setOpen(false)
  }

  return (
    <>
      <button
        type="button"
        onClick={openPicker}
        className={cn(
          'cursor-pointer rounded-md border px-2.5 py-1 text-[13px] transition-colors',
          selectedName
            ? 'border-accent text-white'
            : 'border-white/20 text-white/60 hover:border-white/40 hover:text-white/80',
        )}
      >
        {selectedName ?? t('epub.fontSystem')}
      </button>

      <Dialog open={open} onOpenChange={setOpen} title={t('epub.fontSystemTitle')} size="sm" fill>
        {status === 'ready' ? (
          <>
            <div className="shrink-0 px-5 pt-4">
              <div className="relative">
                <MagnifyingGlass className="pointer-events-none absolute top-1/2 left-3 size-4 -translate-y-1/2 text-ink-3" />
                <input
                  type="search"
                  value={search}
                  onChange={(e) => setSearch(e.target.value)}
                  placeholder={t('epub.fontSystemSearch')}
                  aria-label={t('epub.fontSystemSearch')}
                  autoFocus
                  className="h-9 w-full rounded-lg border border-line bg-surface pr-3 pl-9 text-base text-ink transition-colors placeholder:text-ink-3 focus:border-accent/70 focus:outline-none"
                />
              </div>
            </div>
            <div className="mt-4 min-h-48 flex-1 overflow-y-auto border-t border-line p-2">
              {filteredFamilies.length === 0 ? (
                <p className="px-3 py-8 text-center text-sm text-ink-3">{t('epub.fontSystemEmpty')}</p>
              ) : (
                filteredFamilies.map((family) => (
                  <button
                    key={family}
                    type="button"
                    aria-pressed={selectedName === family}
                    onClick={() => selectFamily(family)}
                    className="flex w-full cursor-pointer items-center justify-between gap-3 rounded-lg px-3 py-2 text-left text-sm text-ink hover:bg-raised"
                  >
                    <span className="truncate" style={{ fontFamily: family }}>
                      {family}
                    </span>
                    {selectedName === family && <Check className="size-4 shrink-0 text-accent" weight="bold" />}
                  </button>
                ))
              )}
            </div>
          </>
        ) : status === 'manual' ? (
          <form
            className="space-y-4 px-5 py-4"
            onSubmit={(e) => {
              e.preventDefault()
              selectFamily(manualName)
            }}
          >
            <TextField
              label={t('epub.fontSystemName')}
              helper={t('epub.fontSystemManual')}
              value={manualName}
              onChange={(e) => setManualName(e.target.value)}
              autoComplete="off"
              spellCheck={false}
              autoFocus
            />
            <div className="flex justify-end gap-2">
              {canRetry && (
                <Button type="button" variant="ghost" onClick={() => void loadFamilies()}>
                  {t('common:action.retry')}
                </Button>
              )}
              <Button type="submit" variant="primary" disabled={!manualName.trim()}>
                {t('common:action.confirm')}
              </Button>
            </div>
          </form>
        ) : (
          <div className="flex min-h-48 items-center justify-center gap-2 text-sm text-ink-3">
            <CircleNotch className="size-4 animate-spin" aria-hidden />
            {t('common:state.loading')}
          </div>
        )}
      </Dialog>
    </>
  )
}
