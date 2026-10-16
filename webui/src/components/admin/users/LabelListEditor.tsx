import { useId, useState, type ReactNode } from 'react'
import { useTranslation } from 'react-i18next'
import { Plus, X } from '@phosphor-icons/react'
import { Button } from '@/components/ui/Button'

interface LabelListEditorProps {
  label: string
  values: string[]
  onChange: (values: string[]) => void
  placeholder?: string
  /** rendered at the right of the label row (e.g. a LockToggle) */
  trailing?: ReactNode
}

export function LabelListEditor({ label, values, onChange, placeholder, trailing }: LabelListEditorProps) {
  const { t } = useTranslation('admin-users')
  const inputId = useId()
  const [draft, setDraft] = useState('')

  const add = () => {
    const v = draft.trim()
    setDraft('')
    if (!v || values.includes(v)) return
    onChange([...values, v])
  }

  return (
    <div className="flex flex-col gap-2">
      <div className="flex items-center justify-between gap-2">
        <label htmlFor={inputId} className="text-[13px] font-medium text-ink-2">
          {label}
        </label>
        {trailing}
      </div>
      <div className="flex gap-2">
        <input
          id={inputId}
          value={draft}
          onChange={(e) => setDraft(e.target.value)}
          onKeyDown={(e) => {
            if (e.key === 'Enter') {
              e.preventDefault()
              add()
            }
          }}
          placeholder={placeholder}
          className="h-10 w-full rounded-lg border border-line bg-surface px-3 text-base text-ink transition-colors placeholder:text-ink-3 focus:border-accent/70 focus:outline-none"
        />
        <Button type="button" onClick={add} disabled={!draft.trim()}>
          <Plus className="size-4" />
          {t('common:action.add')}
        </Button>
      </div>
      {values.length > 0 && (
        <div className="flex flex-wrap gap-1.5">
          {values.map((v) => (
            <span
              key={v}
              className="inline-flex items-center gap-1 rounded-full border border-line bg-raised py-1 pr-1.5 pl-3 text-xs text-ink-2"
            >
              {v}
              <button
                type="button"
                onClick={() => onChange(values.filter((x) => x !== v))}
                aria-label={t('labelList.remove', { label: v })}
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
