import type { ReactNode } from 'react'

export function FormRow({ label, helper, children }: { label: string; helper?: string; children: ReactNode }) {
  return (
    <div className="flex flex-wrap items-center justify-between gap-x-6 gap-y-2.5 py-3 first:pt-0 last:pb-0">
      <div className="min-w-0 max-w-md">
        <p className="text-sm text-ink-2">{label}</p>
        {helper && <p className="mt-0.5 text-xs text-ink-3">{helper}</p>}
      </div>
      {children}
    </div>
  )
}
