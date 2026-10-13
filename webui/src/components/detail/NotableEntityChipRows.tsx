import { Fragment } from 'react'
import { cn } from '@/lib/utils/cn'
import { entityKindLabel } from '@/lib/utils/notableEntities'
import { DetailChipFlow } from './DetailChipFlow'
import type { NotableEntityChipGroup } from './metadataChips'

/** one metadata row per kind: the kind label next to its chips linking to the browse page */
export function NotableEntityChipRows({ groups, className }: { groups: NotableEntityChipGroup[]; className?: string }) {
  if (groups.length === 0) return null
  return (
    <div className={cn('grid grid-cols-[max-content_minmax(0,1fr)] gap-x-4 gap-y-1.5', className)}>
      {groups.map((g) => (
        <Fragment key={g.kind}>
          <p className="max-w-36 py-1 text-xs tracking-wide break-words text-ink-3 uppercase sm:max-w-52">{entityKindLabel(g.kind)}</p>
          <DetailChipFlow items={g.items} />
        </Fragment>
      ))}
    </div>
  )
}
