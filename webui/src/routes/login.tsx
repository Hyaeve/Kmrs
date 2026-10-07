import { useEffect, useState, type FormEvent } from 'react'
import { useNavigate, useSearchParams } from 'react-router-dom'
import { useQuery } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { Eye, EyeSlash } from '@phosphor-icons/react'
import i18n from '@/lib/i18n'
import { usersApi } from '@/lib/api/users'
import { clientSettingsApi } from '@/lib/api/clientSettings'
import { useAuthStore } from '@/lib/store/auth'
import { cn } from '@/lib/utils/cn'
import { useChanged } from '@/lib/hooks/useChanged'
import { LogoMark } from '@/components/LogoMark'
import { LanguageSwitcher } from '@/components/LanguageSwitcher'
import { Button } from '@/components/ui/Button'
import { TextField } from '@/components/ui/TextField'
import { HIDE_PASSWORD_KEY } from '@/routes/admin/ui'
import type { UserDto } from '@/lib/api/types'

const REMEMBER_KEY = 'kmweb.rememberMe'

export function LoginPage() {
  const { t } = useTranslation('auth')
  const navigate = useNavigate()
  const [params] = useSearchParams()
  const status = useAuthStore((s) => s.status)
  const setUser = useAuthStore((s) => s.setUser)

  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [showPassword, setShowPassword] = useState(false)
  const [rememberMe, setRememberMe] = useState(() => localStorage.getItem(REMEMBER_KEY) !== 'false')
  const errorParam = params.get('error')
  const [error, setError] = useState<string | null>(() =>
    errorParam ? t('signInFailedWithReason', { reason: errorParam }) : null,
  )
  const [busy, setBusy] = useState(false)

  const { data: claim } = useQuery({ queryKey: ['claim'], queryFn: usersApi.claimStatus, retry: false })
  const { data: providers } = useQuery({ queryKey: ['oauth2-providers'], queryFn: usersApi.oauth2Providers, retry: false })
  // anonymous callers only see settings flagged allowUnauthorized; on any failure
  // data stays undefined and the password form shows, so admins can't lock themselves out
  const { data: globalSettings } = useQuery({
    queryKey: ['client-settings', 'global', 'public'],
    queryFn: clientSettingsApi.listGlobal,
    retry: false,
  })
  const claimMode = claim != null && !claim.isClaimed
  const hidePasswordForm =
    !claimMode && (providers?.length ?? 0) > 0 && globalSettings?.[HIDE_PASSWORD_KEY]?.value === 'true'

  useEffect(() => {
    if (status === 'authenticated') navigate(params.get('redirect') || '/dashboard', { replace: true })
  }, [status, navigate, params])

  if (useChanged([errorParam, t]) && errorParam) setError(t('signInFailedWithReason', { reason: errorParam }))

  const submit = async (e: FormEvent) => {
    e.preventDefault()
    setBusy(true)
    setError(null)
    localStorage.setItem(REMEMBER_KEY, String(rememberMe))
    try {
      const user: UserDto = claimMode
        ? await claimThenLogin(email, password, rememberMe)
        : await usersApi.login(email, password, rememberMe)
      setUser(user)
      navigate(params.get('redirect') || '/dashboard', { replace: true })
    } catch (err) {
      setError(err instanceof Error ? err.message : t('signInFailed'))
    } finally {
      setBusy(false)
    }
  }

  return (
    <div className="grid min-h-dvh bg-bg lg:grid-cols-2">
      {/* brand panel */}
      <div className="brand-gradient relative hidden flex-col justify-between overflow-hidden p-12 lg:flex">
        <div className="absolute inset-0 bg-[radial-gradient(ellipse_at_bottom_left,rgb(0_0_0/0.35),transparent_60%)]" />
        <LogoMark className="relative size-10 text-white" mono />
        <div className="relative">
          <h1 className="font-display max-w-md text-5xl leading-[1.08] font-medium tracking-tight text-white">
            {t('brand.tagline')}
          </h1>
          <p className="mt-5 max-w-sm text-[15px] leading-relaxed text-white/75">
            {t('brand.subtitle')}
          </p>
        </div>
        <div className="relative text-xs text-white/50">{t('brand.selfHosted')}</div>
      </div>

      {/* form panel */}
      <div className="relative flex flex-col items-center justify-center px-6 py-12">
        <LanguageSwitcher className="absolute top-6 right-6" size="sm" />
        <div className="w-full max-w-sm">
          <LogoMark className="mb-10 size-18 lg:hidden" />
          <h2 className="font-display text-3xl font-semibold tracking-tight">
            {claimMode ? t('claimTitle') : t('welcomeBack')}
          </h2>
          <p className="mt-2 text-sm text-ink-3">
            {claimMode
              ? t('claimSubtitle')
              : hidePasswordForm
                ? t('oauthSubtitle')
                : t('signInSubtitle')}
          </p>

          {!hidePasswordForm && (
            <form onSubmit={submit} className="mt-8 flex flex-col gap-5">
            <TextField
              label={t('email')}
              type="email"
              autoComplete="email"
              required
              value={email}
              onChange={(e) => setEmail(e.target.value)}
              placeholder={t('emailPlaceholder')}
            />
            <TextField
              label={t('password')}
              type={showPassword ? 'text' : 'password'}
              autoComplete={claimMode ? 'new-password' : 'current-password'}
              required
              value={password}
              onChange={(e) => setPassword(e.target.value)}
              placeholder="••••••••"
              trailing={
                <button
                  type="button"
                  onClick={() => setShowPassword((v) => !v)}
                  className="cursor-pointer text-ink-3 hover:text-ink-2"
                  aria-label={showPassword ? t('hidePassword') : t('showPassword')}
                >
                  {showPassword ? <EyeSlash className="size-4.5" /> : <Eye className="size-4.5" />}
                </button>
              }
            />

            <label className="flex cursor-pointer items-center gap-2.5 text-sm text-ink-2 select-none">
              <input
                type="checkbox"
                checked={rememberMe}
                onChange={(e) => setRememberMe(e.target.checked)}
                className="size-4 cursor-pointer accent-[#ff7a55]"
              />
              {t('keepSignedIn')}
            </label>

            {error && <p className="text-sm text-danger">{error}</p>}

            <Button type="submit" variant="primary" loading={busy} className="mt-1 h-11">
              {claimMode ? t('createAccount') : t('signIn')}
            </Button>
          </form>
          )}

          {providers && providers.length > 0 && (
            <>
              {!hidePasswordForm && (
                <div className="my-6 flex items-center gap-3 text-xs text-ink-3">
                  <div className="h-px flex-1 bg-line" />
                  {t('orContinueWith')}
                  <div className="h-px flex-1 bg-line" />
                </div>
              )}
              <div className={cn('flex flex-col gap-2', hidePasswordForm && 'mt-8')}>
                {providers.map((p) => (
                  <Button key={p.registrationId} onClick={() => (window.location.href = `/oauth2/authorization/${p.registrationId}`)}>
                    {p.name}
                  </Button>
                ))}
              </div>
            </>
          )}
        </div>
      </div>
    </div>
  )
}

async function claimThenLogin(email: string, password: string, rememberMe: boolean): Promise<UserDto> {
  const res = await fetch('/api/v1/claim', {
    method: 'POST',
    credentials: 'same-origin',
    headers: {
      'X-Requested-With': 'XMLHttpRequest',
      'X-Komga-Email': email,
      'X-Komga-Password': password,
    },
  })
  if (!res.ok) throw new Error(i18n.t('auth:claimFailed', { status: res.status }))
  return usersApi.login(email, password, rememberMe)
}
