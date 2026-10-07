import { useState } from 'react'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { UserPlus } from '@phosphor-icons/react'
import { usersApi } from '@/lib/api/users'
import type { UserDto } from '@/lib/api/types'
import { useAuthStore } from '@/lib/store/auth'
import { useDocumentTitle } from '@/lib/hooks/useDocumentTitle'
import { Button } from '@/components/ui/Button'
import { PageHeader } from '@/components/ui/PageHeader'
import { ActivitySection } from '@/components/admin/users/ActivitySection'
import { ChangePasswordDialog } from '@/components/admin/users/ChangePasswordDialog'
import { DeleteUserDialog } from '@/components/admin/users/DeleteUserDialog'
import { UserFormDialog } from '@/components/admin/users/UserFormDialog'
import { UserTable } from '@/components/admin/users/UserTable'

type DialogState =
  | { kind: 'add' }
  | { kind: 'edit'; user: UserDto }
  | { kind: 'password'; user: UserDto }
  | { kind: 'delete'; user: UserDto }
  | null

export function AdminUsersPage() {
  const { t } = useTranslation('admin-users')
  const me = useAuthStore((s) => s.user)
  const usersQuery = useQuery({ queryKey: ['admin', 'users'], queryFn: usersApi.list })
  const [dialog, setDialog] = useState<DialogState>(null)

  useDocumentTitle(t('title'))

  const users = usersQuery.data

  return (
    <div className="max-w-5xl">
      <PageHeader
        title={t('title')}
        subtitle={users ? t('subtitle', { count: users.length }) : t('subtitleEmpty')}
        actions={
          <Button variant="primary" onClick={() => setDialog({ kind: 'add' })}>
            <UserPlus className="size-4" />
            {t('addUser')}
          </Button>
        }
      />

      <UserTable
        users={users}
        isLoading={usersQuery.isLoading}
        isLoadingError={usersQuery.isLoadingError}
        error={usersQuery.error}
        onRetry={() => void usersQuery.refetch()}
        currentUserId={me?.id}
        onEdit={(user) => setDialog({ kind: 'edit', user })}
        onChangePassword={(user) => setDialog({ kind: 'password', user })}
        onDelete={(user) => setDialog({ kind: 'delete', user })}
      />

      <ActivitySection />

      <UserFormDialog
        open={dialog?.kind === 'add' || dialog?.kind === 'edit'}
        user={dialog?.kind === 'edit' ? dialog.user : undefined}
        onOpenChange={(open) => {
          if (!open) setDialog(null)
        }}
      />
      <ChangePasswordDialog
        user={dialog?.kind === 'password' ? dialog.user : null}
        onOpenChange={(open) => {
          if (!open) setDialog(null)
        }}
      />
      <DeleteUserDialog
        user={dialog?.kind === 'delete' ? dialog.user : null}
        onOpenChange={(open) => {
          if (!open) setDialog(null)
        }}
      />
    </div>
  )
}
