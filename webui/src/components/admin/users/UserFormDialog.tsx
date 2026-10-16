import { useMemo, useState, type FormEvent } from 'react'
import { useMutation, useQuery, useQueryClient } from '@tanstack/react-query'
import { useTranslation } from 'react-i18next'
import { ApiError } from '@/lib/api/client'
import { librariesApi } from '@/lib/api/libraries'
import { usersApi } from '@/lib/api/users'
import type { AgeRestrictionDto, Role, UserCreationDto, UserDto, UserUpdateDto } from '@/lib/api/types'
import { useChanged } from '@/lib/hooks/useChanged'
import { cn } from '@/lib/utils/cn'
import { Button } from '@/components/ui/Button'
import { Dialog } from '@/components/ui/Dialog'
import { SegmentedControl } from '@/components/ui/SegmentedControl'
import { Skeleton } from '@/components/ui/Skeleton'
import { Switch } from '@/components/ui/Switch'
import { TextField } from '@/components/ui/TextField'
import { LabelListEditor } from './LabelListEditor'
import { ASSIGNABLE_ROLES } from './roles'

type AgeMode = 'none' | 'allow' | 'exclude'

interface FieldErrors {
  email?: string
  password?: string
  age?: string
  form?: string[]
}

type SavePayload = { kind: 'create'; body: UserCreationDto } | { kind: 'update'; id: string; body: UserUpdateDto }

function sameSet(a: ReadonlySet<string>, b: ReadonlySet<string>): boolean {
  if (a.size !== b.size) return false
  for (const v of a) if (!b.has(v)) return false
  return true
}

function violationMessages(err: unknown): { field: string; message: string }[] {
  if (!(err instanceof ApiError)) return []
  const body = err.body as { violations?: { fieldName?: unknown; message?: unknown }[] } | undefined
  if (!body || !Array.isArray(body.violations)) return []
  return body.violations
    .filter((v) => typeof v?.message === 'string')
    .map((v) => ({ field: typeof v.fieldName === 'string' ? v.fieldName : '', message: v.message as string }))
}

function FormSectionTitle({ children }: { children: string }) {
  return <h3 className="mb-3 text-[11px] font-medium tracking-[0.08em] text-ink-3 uppercase">{children}</h3>
}

export function UserFormDialog({
  open,
  onOpenChange,
  user,
}: {
  open: boolean
  onOpenChange: (open: boolean) => void
  user?: UserDto
}) {
  const { t } = useTranslation('admin-users')
  const queryClient = useQueryClient()
  const [email, setEmail] = useState('')
  const [password, setPassword] = useState('')
  const [roles, setRoles] = useState<Set<Role>>(new Set())
  const [shareAll, setShareAll] = useState(true)
  const [libraryIds, setLibraryIds] = useState<Set<string>>(new Set())
  const [ageMode, setAgeMode] = useState<AgeMode>('none')
  const [age, setAge] = useState('')
  const [labelsAllow, setLabelsAllow] = useState<string[]>([])
  const [labelsExclude, setLabelsExclude] = useState<string[]>([])
  const [errors, setErrors] = useState<FieldErrors>({})

  const librariesQuery = useQuery({ queryKey: ['libraries'], queryFn: librariesApi.list, enabled: open })

  if (useChanged([open, user]) && open) {
    setErrors({})
    setEmail('')
    setPassword('')
    setRoles(new Set((user?.roles ?? []).filter((r) => r !== 'USER')))
    setShareAll(user?.sharedAllLibraries ?? true)
    setLibraryIds(new Set(user?.sharedLibrariesIds ?? []))
    setAgeMode(user?.ageRestriction ? (user.ageRestriction.restriction === 'ALLOW_ONLY' ? 'allow' : 'exclude') : 'none')
    setAge(user?.ageRestriction ? String(user.ageRestriction.age) : '')
    setLabelsAllow(user?.labelsAllow ?? [])
    setLabelsExclude(user?.labelsExclude ?? [])
  }

  const saveMutation = useMutation({
    mutationFn: async (payload: SavePayload) => {
      if (payload.kind === 'create') await usersApi.create(payload.body)
      else await usersApi.update(payload.id, payload.body)
    },
    onSuccess: () => {
      void queryClient.invalidateQueries({ queryKey: ['admin', 'users'] })
      onOpenChange(false)
    },
    onError: (err) => {
      const violations = violationMessages(err)
      if (violations.length > 0) {
        const next: FieldErrors = {}
        const rest: string[] = []
        for (const v of violations) {
          if (v.field === 'email') next.email = v.message
          else if (v.field === 'password') next.password = v.message
          else if (v.field.startsWith('ageRestriction')) next.age = v.message
          else rest.push(v.field ? `${v.field}: ${v.message}` : v.message)
        }
        if (rest.length > 0) next.form = rest
        setErrors(next)
        return
      }
      const message = err instanceof Error ? err.message : t('form.saveFailed')
      // A duplicate-email 400 reads much better under the email field than as a banner.
      if (!user && /email/i.test(message)) setErrors({ email: message })
      else setErrors({ form: [message] })
    },
  })

  const close = () => {
    saveMutation.reset()
    onOpenChange(false)
  }

  const toggleRole = (role: Role, on: boolean) =>
    setRoles((prev) => {
      const next = new Set(prev)
      if (on) next.add(role)
      else next.delete(role)
      return next
    })

  const toggleLibrary = (id: string) =>
    setLibraryIds((prev) => {
      const next = new Set(prev)
      if (next.has(id)) next.delete(id)
      else next.add(id)
      return next
    })

  const ageNumber = Number(age)
  const ageMissing = ageMode !== 'none' && age.trim() === ''
  const ageInvalid = ageMode !== 'none' && age.trim() !== '' && (!Number.isInteger(ageNumber) || ageNumber < 0)

  const changes = useMemo((): UserUpdateDto => {
    if (!user) return {}
    const out: UserUpdateDto = {}
    if (!sameSet(new Set(user.roles.filter((r) => r !== 'USER')), roles)) out.roles = [...roles]
    if (shareAll !== user.sharedAllLibraries || (!shareAll && !sameSet(new Set(user.sharedLibrariesIds), libraryIds)))
      out.sharedLibraries = shareAll ? { all: true } : { all: false, libraryIds: [...libraryIds] }
    const nextAge: AgeRestrictionDto | null =
      ageMode === 'none' ? null : { age: ageNumber, restriction: ageMode === 'allow' ? 'ALLOW_ONLY' : 'EXCLUDE' }
    const prevAge = user.ageRestriction ?? null
    if ((prevAge?.age ?? null) !== (nextAge?.age ?? null) || (prevAge?.restriction ?? null) !== (nextAge?.restriction ?? null))
      // 'NONE' is how a patch clears an existing restriction; omitting would keep it.
      out.ageRestriction = nextAge ?? { age: 0, restriction: 'NONE' }
    if (!sameSet(new Set(user.labelsAllow), new Set(labelsAllow))) out.labelsAllow = labelsAllow
    if (!sameSet(new Set(user.labelsExclude), new Set(labelsExclude))) out.labelsExclude = labelsExclude
    return out
  }, [user, roles, shareAll, libraryIds, ageMode, ageNumber, labelsAllow, labelsExclude])

  const isDirty = Object.keys(changes).length > 0
  const canSubmit =
    !saveMutation.isPending && !ageMissing && !ageInvalid && (user ? isDirty : email.trim() !== '' && password !== '')

  const submit = (e: FormEvent) => {
    e.preventDefault()
    if (!canSubmit) return
    setErrors({})
    if (user) {
      saveMutation.mutate({ kind: 'update', id: user.id, body: changes })
      return
    }
    const body: UserCreationDto = {
      email: email.trim(),
      password,
      roles: [...roles],
      sharedLibraries: shareAll ? { all: true } : { all: false, libraryIds: [...libraryIds] },
    }
    if (ageMode !== 'none') body.ageRestriction = { age: ageNumber, restriction: ageMode === 'allow' ? 'ALLOW_ONLY' : 'EXCLUDE' }
    if (labelsAllow.length > 0) body.labelsAllow = labelsAllow
    if (labelsExclude.length > 0) body.labelsExclude = labelsExclude
    saveMutation.mutate({ kind: 'create', body })
  }

  return (
    <Dialog
      open={open}
      onOpenChange={(o) => {
        if (!o) close()
      }}
      title={user ? t('form.editTitle') : t('addUser')}
      size="lg"
    >
      <form onSubmit={submit}>
        <div className="flex flex-col gap-6 px-5 py-4">
          {errors.form && (
            <div className="flex flex-col gap-1 rounded-lg border border-danger/40 bg-danger/10 px-3 py-2.5 text-[13px] text-danger">
              {errors.form.map((m, i) => (
                <p key={i}>{m}</p>
              ))}
            </div>
          )}

          {user ? (
            <p className="truncate text-sm text-ink-3">{user.email}</p>
          ) : (
            <section>
              <FormSectionTitle>{t('form.sectionAccount')}</FormSectionTitle>
              <div className="grid gap-4 sm:grid-cols-2">
                <TextField
                  label={t('account:profile.email')}
                  type="email"
                  autoComplete="off"
                  required
                  value={email}
                  onChange={(e) => {
                    setEmail(e.target.value)
                    setErrors((p) => ({ ...p, email: undefined }))
                  }}
                  error={errors.email}
                  placeholder="reader@example.com"
                />
                <TextField
                  label={t('form.password')}
                  type="password"
                  autoComplete="new-password"
                  required
                  value={password}
                  onChange={(e) => {
                    setPassword(e.target.value)
                    setErrors((p) => ({ ...p, password: undefined }))
                  }}
                  error={errors.password}
                />
              </div>
            </section>
          )}

          <section>
            <FormSectionTitle>{t('account:profile.roles')}</FormSectionTitle>
            <div className="flex flex-col">
              {ASSIGNABLE_ROLES.map((r) => (
                <div
                  key={r.value}
                  className="flex items-center justify-between gap-4 border-t border-line py-3 first:border-t-0 first:pt-0 last:pb-0"
                >
                  <div>
                    <p className="text-sm text-ink">{t(r.labelKey)}</p>
                    <p className="mt-0.5 text-xs text-ink-3">{t(r.descriptionKey)}</p>
                  </div>
                  <Switch checked={roles.has(r.value)} onCheckedChange={(v) => toggleRole(r.value, v)} label={t(r.labelKey)} />
                </div>
              ))}
            </div>
          </section>

          <section>
            <FormSectionTitle>{t('form.sectionLibraryAccess')}</FormSectionTitle>
            <div className="flex items-center justify-between gap-4">
              <div>
                <p className="text-sm text-ink">{t('account:profile.allLibraries')}</p>
                <p className="mt-0.5 text-xs text-ink-3">{t('form.allLibrariesDesc')}</p>
              </div>
              <Switch checked={shareAll} onCheckedChange={setShareAll} label={t('account:profile.allLibraries')} />
            </div>
            {!shareAll && (
              <div className="mt-3 max-h-52 overflow-y-auto rounded-lg border border-line">
                {librariesQuery.isLoading && (
                  <div className="flex flex-col gap-2 p-3">
                    <Skeleton className="h-5 w-2/3" />
                    <Skeleton className="h-5 w-1/2" />
                  </div>
                )}
                {librariesQuery.data?.map((lib) => (
                  <label
                    key={lib.id}
                    className="flex cursor-pointer items-center gap-2.5 border-t border-line px-3 py-2.5 transition-colors first:border-t-0 hover:bg-raised"
                  >
                    <input
                      type="checkbox"
                      checked={libraryIds.has(lib.id)}
                      onChange={() => toggleLibrary(lib.id)}
                      className="size-4 shrink-0 accent-accent"
                    />
                    <span className="truncate text-sm text-ink-2">{lib.name}</span>
                  </label>
                ))}
                {librariesQuery.data?.length === 0 && <p className="px-3 py-2.5 text-sm text-ink-3">{t('layout:nav.noLibraries')}</p>}
              </div>
            )}
          </section>

          <section>
            <FormSectionTitle>{t('form.sectionContentRestrictions')}</FormSectionTitle>
            <div>
              <p className="mb-2 text-[13px] font-medium text-ink-2">{t('form.ageRestriction')}</p>
              <div className="flex flex-wrap items-center gap-3">
                <SegmentedControl<AgeMode>
                  options={[
                    { value: 'none', label: t('form.ageMode.none') },
                    { value: 'allow', label: t('form.ageMode.allow') },
                    { value: 'exclude', label: t('form.ageMode.exclude') },
                  ]}
                  value={ageMode}
                  onChange={setAgeMode}
                />
                {ageMode !== 'none' && (
                  <input
                    type="number"
                    min={0}
                    step={1}
                    value={age}
                    onChange={(e) => {
                      setAge(e.target.value)
                      setErrors((p) => ({ ...p, age: undefined }))
                    }}
                    placeholder={t('form.age')}
                    aria-label={t('form.age')}
                    className={cn(
                      'h-10 w-24 rounded-lg border bg-surface px-3 text-base text-ink transition-colors placeholder:text-ink-3 focus:outline-none',
                      errors.age || ageInvalid ? 'border-danger/60 focus:border-danger' : 'border-line focus:border-accent/70',
                    )}
                  />
                )}
              </div>
              {errors.age || ageInvalid ? (
                <p className="mt-2 text-[13px] text-danger">{errors.age ?? t('form.ageInvalid')}</p>
              ) : ageMode === 'allow' ? (
                <p className="mt-2 text-[13px] text-ink-3">{t('form.ageAllowHint')}</p>
              ) : ageMode === 'exclude' ? (
                <p className="mt-2 text-[13px] text-ink-3">{t('form.ageExcludeHint')}</p>
              ) : null}
            </div>
            <div className="mt-4 grid gap-4 sm:grid-cols-2">
              <LabelListEditor label={t('form.allowedLabels')} values={labelsAllow} onChange={setLabelsAllow} placeholder={t('metadata:placeholder.addLabel')} />
              <LabelListEditor label={t('form.excludedLabels')} values={labelsExclude} onChange={setLabelsExclude} placeholder={t('metadata:placeholder.addLabel')} />
            </div>
          </section>
        </div>

        <div className="flex justify-end gap-2 border-t border-line px-5 py-3.5">
          <Button type="button" variant="ghost" onClick={close}>
            {t('common:action.cancel')}
          </Button>
          <Button type="submit" variant="primary" loading={saveMutation.isPending} disabled={!canSubmit}>
            {user ? t('metadata:action.saveChanges') : t('addUser')}
          </Button>
        </div>
      </form>
    </Dialog>
  )
}
