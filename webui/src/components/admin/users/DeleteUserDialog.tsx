import { useMutation, useQueryClient } from '@tanstack/react-query'
import { Trans, useTranslation } from 'react-i18next'
import { usersApi } from '@/lib/api/users'
import type { UserDto } from '@/lib/api/types'
import { Button } from '@/components/ui/Button'
import { Dialog } from '@/components/ui/Dialog'

export function DeleteUserDialog({ user, onOpenChange }: { user: UserDto | null; onOpenChange: (open: boolean) => void }) {
  const { t } = useTranslation('admin-users')
  const queryClient = useQueryClient()

  const mutation = useMutation({
    mutationFn: (id: string) => usersApi.delete(id),
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['admin', 'users'] })
      // their activity rows are gone too, so the activity table must refetch
      void queryClient.invalidateQueries({ queryKey: ['admin', 'authentication-activity'] })
      onOpenChange(false)
    },
  })

  const close = () => {
    mutation.reset()
    onOpenChange(false)
  }

  return (
    <Dialog
      open={!!user}
      onOpenChange={(o) => {
        if (!o) close()
      }}
      title={t('deleteDialog.title')}
      size="sm"
    >
      <div className="px-5 py-4">
        <p className="text-sm text-ink-2">
          <Trans
            i18nKey="deleteDialog.body"
            ns="admin-users"
            values={{ email: user?.email ?? '' }}
            components={{ email: <span className="font-medium text-ink" /> }}
          />
        </p>
        {mutation.isError && (
          <p className="mt-3 text-sm text-danger">
            {mutation.error instanceof Error ? mutation.error.message : t('deleteDialog.failed')}
          </p>
        )}
      </div>
      <div className="flex justify-end gap-2 border-t border-line px-5 py-3.5">
        <Button variant="ghost" onClick={close}>
          {t('common:action.cancel')}
        </Button>
        <Button variant="danger" loading={mutation.isPending} onClick={() => user && mutation.mutate(user.id)}>
          {t('common:action.delete')}
        </Button>
      </div>
    </Dialog>
  )
}
