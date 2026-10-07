import { useState, type FormEvent } from 'react'
import { useTranslation } from 'react-i18next'
import { usersApi } from '@/lib/api/users'
import { Button } from '@/components/ui/Button'
import { TextField } from '@/components/ui/TextField'
import { Section } from './Section'

export function PasswordSection() {
  const { t } = useTranslation('account')
  const [password, setPassword] = useState('')
  const [confirm, setConfirm] = useState('')
  const [busy, setBusy] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [updated, setUpdated] = useState(false)

  const mismatch = confirm.length > 0 && password !== confirm

  const submit = async (e: FormEvent) => {
    e.preventDefault()
    if (!password || password !== confirm) return
    setBusy(true)
    setError(null)
    setUpdated(false)
    try {
      await usersApi.updatePassword(password)
      setPassword('')
      setConfirm('')
      setUpdated(true)
    } catch (err) {
      setError(err instanceof Error ? err.message : t('security.password.failed'))
    } finally {
      setBusy(false)
    }
  }

  return (
    <Section title={t('security.password.title')}>
      <form onSubmit={submit} className="flex max-w-sm flex-col gap-4">
        <TextField
          label={t('security.password.new')}
          type="password"
          autoComplete="new-password"
          required
          value={password}
          onChange={(e) => setPassword(e.target.value)}
          helper={t('security.password.staySignedIn')}
        />
        <TextField
          label={t('security.password.confirm')}
          type="password"
          autoComplete="new-password"
          required
          value={confirm}
          onChange={(e) => setConfirm(e.target.value)}
          error={mismatch ? t('security.password.mismatch') : undefined}
        />
        {error && <p className="text-sm text-danger">{error}</p>}
        {updated && <p className="text-sm text-accent-strong">{t('security.password.updated')}</p>}
        <div>
          <Button type="submit" variant="primary" loading={busy} disabled={!password || password !== confirm}>
            {t('security.password.submit')}
          </Button>
        </div>
      </form>
    </Section>
  )
}
