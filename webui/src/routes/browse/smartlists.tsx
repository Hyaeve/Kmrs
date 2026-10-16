import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { Plus } from '@phosphor-icons/react'
import { useQuery } from '@tanstack/react-query'
import { usersApi } from '@/lib/api/users'
import { isAdmin, useAuthStore } from '@/lib/store/auth'
import { useDocumentTitle } from '@/lib/hooks/useDocumentTitle'
import { Button } from '@/components/ui/Button'
import { SmartListsGrid } from '@/components/browse/SmartListsGrid'
import { SmartListDialog } from '@/components/smartlists/SmartListDialog'

export function BrowseSmartListsPage() {
  const { t } = useTranslation('smartlists')
  const [createOpen, setCreateOpen] = useState(false)
  const user = useAuthStore((s) => s.user)
  const admin = isAdmin(user)
  // admins browse by owner dimension: everyone, only their own, or one specific user
  const [owner, setOwner] = useState<string>('')
  const usersQuery = useQuery({ queryKey: ['users'], queryFn: usersApi.list, enabled: admin })
  useDocumentTitle(t('title'))

  return (
    <div>
      <div className="mb-5 flex flex-wrap items-center justify-between gap-3">
        <h1 className="font-display text-2xl font-semibold text-ink">{t('title')}</h1>
        <div className="flex items-center gap-2">
          {admin && (
            <select
              value={owner}
              onChange={(e) => setOwner(e.target.value)}
              aria-label={t('ownerFilterLabel')}
              className="h-8 rounded-lg border border-line bg-surface px-2 text-sm text-ink focus:border-accent/70 focus:outline-none"
            >
              <option value="">{t('ownerFilter.all')}</option>
              <option value={user?.id}>{t('ownerFilter.mine')}</option>
              {(usersQuery.data ?? [])
                .filter((u) => u.id !== user?.id)
                .map((u) => (
                  <option key={u.id} value={u.id}>
                    {u.email}
                  </option>
                ))}
            </select>
          )}
          <Button variant="primary" size="sm" onClick={() => setCreateOpen(true)}>
            <Plus className="size-4" /> {t('create')}
          </Button>
        </div>
      </div>
      <SmartListsGrid owner={owner || undefined} onCreate={() => setCreateOpen(true)} />
      <SmartListDialog open={createOpen} onOpenChange={setCreateOpen} />
    </div>
  )
}
