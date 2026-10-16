import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Clock, Cpu, GitBranch, HardDrives, Memory } from '@phosphor-icons/react'
import { actuatorApi } from '@/lib/api/settings'
import { serverApi } from '@/lib/api/users'
import { formatBytes } from '@/lib/utils/format'
import { StatCard } from './StatCard'
import { formatDuration } from './format'
import { useServerStats } from '../stats'

export function StatsGrid() {
  const { t } = useTranslation('admin-settings')
  const info = useQuery({ queryKey: ['server-info'], queryFn: serverApi.info, staleTime: Infinity })
  const health = useQuery({ queryKey: ['admin', 'health'], queryFn: actuatorApi.health, refetchInterval: 30_000 })
  const stats = useServerStats()

  const process = stats.data?.process
  const uptimeSecs = process?.uptimeSeconds
  const cpuPct = process?.cpuUsage
  const memoryBytes = process?.memoryBytes
  const disk = health.data?.components?.diskSpace?.details
  const git = [info.data?.git?.branch, info.data?.git?.commit?.id].filter(Boolean).join(' · ')

  return (
    <div className="grid grid-cols-2 gap-4 sm:grid-cols-3 xl:grid-cols-5">
      <StatCard
        icon={<GitBranch className="size-3.5" />}
        label={t('stats.version')}
        value={info.data?.build?.version ?? '—'}
        sub={git || undefined}
      />
      <StatCard
        icon={<Clock className="size-3.5" />}
        label={t('stats.uptime')}
        value={uptimeSecs !== undefined ? formatDuration(uptimeSecs) : '—'}
      />
      <StatCard
        icon={<Cpu className="size-3.5" />}
        label={t('stats.cpu')}
        value={cpuPct !== undefined ? `${cpuPct >= 10 ? Math.round(cpuPct) : cpuPct.toFixed(1)}%` : '—'}
      />
      <StatCard
        icon={<Memory className="size-3.5" />}
        label={t('stats.memory')}
        value={memoryBytes !== undefined ? formatBytes(memoryBytes) : '—'}
      />
      <StatCard
        icon={<HardDrives className="size-3.5" />}
        label={t('stats.diskFree')}
        value={disk?.free !== undefined ? formatBytes(disk.free) : '—'}
        sub={disk?.total !== undefined ? t('stats.diskTotal', { total: formatBytes(disk.total) }) : undefined}
      />
    </div>
  )
}
