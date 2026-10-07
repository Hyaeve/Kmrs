import { useTranslation } from 'react-i18next'
import { Dialog } from '@/components/ui/Dialog'

const GROUPS: { titleKey: string; rows: [string, string][] }[] = [
  {
    titleKey: 'shortcuts.navigation',
    rows: [
      ['← / →', 'shortcuts.turnPage'],
      ['↑ / ↓', 'shortcuts.turnPageVertical'],
      ['Home / End', 'shortcuts.firstLastPage'],
      ['Space / PgUp / PgDn', 'shortcuts.scrollWebtoon'],
    ],
  },
  {
    titleKey: 'chrome.settings',
    rows: [
      ['L / R / V / W', 'shortcuts.readingDirection'],
      ['C', 'shortcuts.cycleScale'],
      ['D', 'shortcuts.cycleLayout'],
      ['P', 'shortcuts.cyclePadding'],
      ['N', 'shortcuts.cycleMargin'],
      ['F', 'shortcuts.toggleFullscreen'],
    ],
  },
  {
    titleKey: 'shortcuts.menus',
    rows: [
      ['M', 'shortcuts.toggleToolbars'],
      ['S', 'chrome.settings'],
      ['T', 'shortcuts.thumbnails'],
      ['H', 'shortcuts.thisHelp'],
      ['Esc', 'shortcuts.closeExit'],
    ],
  },
]

interface ShortcutsHelpProps {
  open: boolean
  onOpenChange: (open: boolean) => void
}

export function ShortcutsHelp({ open, onOpenChange }: ShortcutsHelpProps) {
  const { t } = useTranslation('reader')
  return (
    <Dialog open={open} onOpenChange={onOpenChange} title={t('shortcuts.title')} size="lg">
      <div className="grid gap-6 p-5 md:grid-cols-3">
        {GROUPS.map((group) => (
          <section key={group.titleKey}>
            <h3 className="mb-2 text-[11px] font-medium tracking-wide text-ink-3 uppercase">{t(group.titleKey)}</h3>
            <div className="space-y-1">
              {group.rows.map(([keys, descriptionKey]) => (
                <div key={descriptionKey} className="flex items-center justify-between gap-3 py-1">
                  <span className="text-[13px] text-ink-2">{t(descriptionKey)}</span>
                  <kbd className="rounded-md border border-line bg-raised px-1.5 py-0.5 font-mono text-[11px] whitespace-nowrap text-ink">
                    {keys}
                  </kbd>
                </div>
              ))}
            </div>
          </section>
        ))}
      </div>
    </Dialog>
  )
}
