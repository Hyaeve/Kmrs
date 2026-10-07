import { useState, type FormEvent } from 'react'
import { useMutation } from '@tanstack/react-query'
import { Trans, useTranslation } from 'react-i18next'
import { usersApi } from '@/lib/api/users'
import type { UserDto } from '@/lib/api/types'
import { useChanged } from '@/lib/hooks/useChanged'
import { Button } from '@/components/ui/Button'
import { Dialog } from '@/components/ui/Dialog'
import { TextField } from '@/components/ui/TextField'

export function ChangePasswordDialog({ user, onOpenChange }: { user: UserDto | null; onOpenChange: (open: boolean) => void }) {
  const { t } = useTranslation('admin-users')
  const [password, setPassword] = useState('')
  const [confirm, setConfirm] = useState('')

  if (useChanged([user]) && user) {
    setPassword('')
    setConfirm('')
  }

  const mutation = useMutation({
    mutationFn: ({ id, password: pw }: { id: string; password: string }) => usersApi.updatePasswordFor(id, pw),
    onSuccess: () => onOpenChange(false),
  })

  const close = () => {
    mutation.reset()
    onOpenChange(false)
  }

  const mismatch = confirm.length > 0 && password !== confirm

  const submit = (e: FormEvent) => {
    e.preventDefault()
    if (!user || !password || password !== confirm) return
    mutation.mutate({ id: user.id, password })
  }

  return (
    <Dialog
      open={!!user}
      onOpenChange={(o) => {
        if (!o) close()
      }}
      title={t('account:security.password.title')}
      size="sm"
    >
      <form onSubmit={submit} className="flex flex-col gap-4 px-5 py-4">
        <p className="text-sm text-ink-3">
          <Trans
            i18nKey="changePassword.body"
            ns="admin-users"
            values={{ email: user?.email ?? '' }}
            components={{ email: <span className="text-ink" /> }}
          />
        </p>
        <TextField
          label={t('account:security.password.new')}
          type="password"
          autoComplete="new-password"
          required
          value={password}
          onChange={(e) => setPassword(e.target.value)}
        />
        <TextField
          label={t('account:security.password.confirm')}
          type="password"
          autoComplete="new-password"
          required
          value={confirm}
          onChange={(e) => setConfirm(e.target.value)}
          error={mismatch ? t('account:security.password.mismatch') : undefined}
        />
        {mutation.isError && (
          <p className="text-sm text-danger">
            {mutation.error instanceof Error ? mutation.error.message : t('account:security.password.failed')}
          </p>
        )}
        <div className="flex justify-end gap-2">
          <Button type="button" variant="ghost" onClick={close}>
            {t('common:action.cancel')}
          </Button>
          <Button type="submit" variant="primary" loading={mutation.isPending} disabled={!password || password !== confirm}>
            {t('account:security.password.submit')}
          </Button>
        </div>
      </form>
    </Dialog>
  )
}
