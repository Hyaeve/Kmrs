import type { ReactNode } from 'react'

interface StatCardProps {
  icon: ReactNode
  label: string
  value: ReactNode
  sub?: ReactNode
}

export function StatCard({ icon, label, value, sub }: StatCardProps) {
  return (
    <div className="rounded-xl border border-line bg-surface p-4">
      <div className="flex items-center gap-1.5 text-xs text-ink-3">
        {icon}
        {label}
      </div>
      <p className="mt-2 truncate text-xl font-semibold text-ink">{value}</p>
      {sub && <p className="mt-0.5 truncate text-xs text-ink-3">{sub}</p>}
    </div>
  )
}
