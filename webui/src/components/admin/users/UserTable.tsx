import { useTranslation } from 'react-i18next'
import { DotsThreeVertical, Key, PencilSimple, Trash, Users } from '@phosphor-icons/react'
import type { UserDto } from '@/lib/api/types'
import { cn } from '@/lib/utils/cn'
import { Button } from '@/components/ui/Button'
import { Chip } from '@/components/ui/Chip'
import { EmptyState } from '@/components/ui/EmptyState'
import { IconButton } from '@/components/ui/IconButton'
import { Menu, MenuItem, MenuSeparator } from '@/components/ui/Menu'
import { Skeleton } from '@/components/ui/Skeleton'
import { Tooltip } from '@/components/ui/Tooltip'
import { ASSIGNABLE_ROLES } from './roles'

interface UserTableProps {
  users: UserDto[] | undefined
  isLoading: boolean
  isLoadingError: boolean
  error: unknown
  onRetry: () => void
  currentUserId: string | undefined
  onEdit: (user: UserDto) => void
  onChangePassword: (user: UserDto) => void
  onDelete: (user: UserDto) => void
}

function Avatar({ email }: { email: string }) {
  return (
    <span className="flex size-8 shrink-0 items-center justify-center rounded-full bg-accent-soft text-sm font-semibold text-accent-strong">
      {email[0]?.toUpperCase() ?? '?'}
    </span>
  )
}

function RoleChips({ user }: { user: UserDto }) {
  const { t } = useTranslation('admin-users')
  const present = ASSIGNABLE_ROLES.filter((r) => user.roles.includes(r.value))
  if (present.length === 0) return <span className="text-ink-3">—</span>
  return (
    <div className="flex flex-wrap gap-1">
      {present.map((r) => (
        <Chip
          key={r.value}
          className={cn('px-2 py-0.5', r.value === 'ADMIN' && 'border-accent/40 bg-accent-soft text-accent-strong')}
        >
          {t(r.labelKey)}
        </Chip>
      ))}
    </div>
  )
}

function Restrictions({ user }: { user: UserDto }) {
  const { t } = useTranslation('admin-users')
  const lines: string[] = []
  if (user.ageRestriction)
    lines.push(
      user.ageRestriction.restriction === 'ALLOW_ONLY'
        ? t('restrictions.ageAllow', { age: user.ageRestriction.age })
        : t('restrictions.ageExclude', { age: user.ageRestriction.age }),
    )
  const labelParts: string[] = []
  if (user.labelsAllow.length > 0) labelParts.push(t('restrictions.labelsAllowed', { count: user.labelsAllow.length }))
  if (user.labelsExclude.length > 0) labelParts.push(t('restrictions.labelsExcluded', { count: user.labelsExclude.length }))
  if (lines.length === 0 && labelParts.length === 0) return <span className="text-ink-3">—</span>
  return (
    <div className="flex flex-col gap-0.5">
      {lines.map((l) => (
        <span key={l} className="text-ink-2">
          {l}
        </span>
      ))}
      {labelParts.length > 0 && <span className="text-xs text-ink-3">{labelParts.join(' · ')}</span>}
    </div>
  )
}

// Radix swallows hover on disabled menu items, so self-actions get a plain
// span to keep the explanatory tooltip alive.
function DisabledAction({ icon, label, hint, danger }: { icon: React.ReactNode; label: string; hint: string; danger?: boolean }) {
  return (
    <Tooltip content={hint}>
      <span
        className={cn(
          'flex cursor-not-allowed items-center gap-2.5 rounded-lg px-2.5 py-2 text-[13px] opacity-40 select-none',
          danger ? 'text-danger' : 'text-ink',
        )}
      >
        {icon}
        {label}
      </span>
    </Tooltip>
  )
}

function RowActions({
  user,
  isSelf,
  onEdit,
  onChangePassword,
  onDelete,
}: {
  user: UserDto
  isSelf: boolean
  onEdit: (user: UserDto) => void
  onChangePassword: (user: UserDto) => void
  onDelete: (user: UserDto) => void
}) {
  const { t } = useTranslation('admin-users')
  return (
    <Menu
      trigger={
        <IconButton label={t('rowActions', { email: user.email })}>
          <DotsThreeVertical className="size-4" />
        </IconButton>
      }
    >
      {isSelf ? (
        <DisabledAction
          icon={<PencilSimple className="size-4" />}
          label={t('common:action.edit')}
          hint={t('selfEditHint')}
        />
      ) : (
        <MenuItem onSelect={() => onEdit(user)}>
          <PencilSimple className="size-4" /> {t('common:action.edit')}
        </MenuItem>
      )}
      <MenuItem onSelect={() => onChangePassword(user)}>
        <Key className="size-4" /> {t('account:security.password.title')}
      </MenuItem>
      <MenuSeparator />
      {isSelf ? (
        <DisabledAction
          icon={<Trash className="size-4" />}
          label={t('common:action.delete')}
          hint={t('selfDeleteHint')}
          danger
        />
      ) : (
        <MenuItem danger onSelect={() => onDelete(user)}>
          <Trash className="size-4" /> {t('common:action.delete')}
        </MenuItem>
      )}
    </Menu>
  )
}

export function UserTable({
  users,
  isLoading,
  isLoadingError,
  error,
  onRetry,
  currentUserId,
  onEdit,
  onChangePassword,
  onDelete,
}: UserTableProps) {
  const { t } = useTranslation('admin-users')

  if (isLoading)
    return (
      <div className="flex flex-col gap-4 rounded-xl border border-line bg-surface p-4">
        {Array.from({ length: 3 }, (_, i) => (
          <div key={i} className="flex items-center gap-3">
            <Skeleton className="size-8 rounded-full" />
            <Skeleton className="h-4 flex-1" />
            <Skeleton className="h-4 w-28" />
          </div>
        ))}
      </div>
    )

  if (isLoadingError)
    return (
      <div className="flex items-center gap-3 rounded-xl border border-line bg-surface px-4 py-6">
        <p className="text-sm text-danger">{error instanceof Error ? error.message : t('loadFailed')}</p>
        <Button size="sm" onClick={onRetry}>
          {t('common:action.retry')}
        </Button>
      </div>
    )

  if (!users || users.length === 0)
    return (
      <div className="rounded-xl border border-line bg-surface">
        <EmptyState icon={<Users />} title={t('emptyTitle')} body={t('emptyBody')} />
      </div>
    )

  const sorted = [...users].sort((a, b) => a.email.localeCompare(b.email))
  const actionProps = { onEdit, onChangePassword, onDelete }
  const librariesText = (u: UserDto) =>
    u.sharedAllLibraries
      ? t('account:profile.allLibraries')
      : t('account:profile.sharedLibraries', { count: u.sharedLibrariesIds.length })

  return (
    <>
      <div className="hidden overflow-x-auto rounded-xl border border-line bg-surface md:block">
        <table className="w-full min-w-[720px] text-sm">
          <thead>
            <tr className="border-b border-line text-left text-xs text-ink-3">
              <th className="px-4 py-3 font-medium">{t('user')}</th>
              <th className="px-4 py-3 font-medium">{t('account:profile.roles')}</th>
              <th className="px-4 py-3 font-medium">{t('account:profile.libraries')}</th>
              <th className="px-4 py-3 font-medium">{t('table.restrictions')}</th>
              <th className="px-4 py-3">
                <span className="sr-only">{t('table.actions')}</span>
              </th>
            </tr>
          </thead>
          <tbody>
            {sorted.map((u) => {
              const isSelf = u.id === currentUserId
              return (
                <tr key={u.id} className="border-t border-line first:border-t-0">
                  <td className="px-4 py-3">
                    <div className="flex items-center gap-3">
                      <Avatar email={u.email} />
                      <span className="truncate text-ink">{u.email}</span>
                      {isSelf && <Chip className="px-2 py-0.5 text-[11px]">{t('you')}</Chip>}
                    </div>
                  </td>
                  <td className="px-4 py-3">
                    <RoleChips user={u} />
                  </td>
                  <td className="px-4 py-3 whitespace-nowrap text-ink-2">{librariesText(u)}</td>
                  <td className="px-4 py-3">
                    <Restrictions user={u} />
                  </td>
                  <td className="px-4 py-3 text-right">
                    <RowActions user={u} isSelf={isSelf} {...actionProps} />
                  </td>
                </tr>
              )
            })}
          </tbody>
        </table>
      </div>

      <div className="flex flex-col gap-3 md:hidden">
        {sorted.map((u) => {
          const isSelf = u.id === currentUserId
          return (
            <div key={u.id} className="rounded-xl border border-line bg-surface p-4">
              <div className="flex items-center justify-between gap-2">
                <div className="flex min-w-0 items-center gap-3">
                  <Avatar email={u.email} />
                  <div className="flex min-w-0 items-center gap-2">
                    <span className="truncate text-sm font-medium text-ink">{u.email}</span>
                    {isSelf && <Chip className="shrink-0 px-2 py-0.5 text-[11px]">{t('you')}</Chip>}
                  </div>
                </div>
                <RowActions user={u} isSelf={isSelf} {...actionProps} />
              </div>
              <div className="mt-3">
                <RoleChips user={u} />
              </div>
              <dl className="mt-3 grid grid-cols-[92px_1fr] gap-x-3 gap-y-1.5 text-[13px]">
                <dt className="text-ink-3">{t('account:profile.libraries')}</dt>
                <dd className="text-ink-2">{librariesText(u)}</dd>
                <dt className="text-ink-3">{t('table.restrictions')}</dt>
                <dd>
                  <Restrictions user={u} />
                </dd>
              </dl>
            </div>
          )
        })}
      </div>
    </>
  )
}
