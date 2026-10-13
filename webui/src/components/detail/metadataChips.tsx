import { Buildings, MaskHappy, Sparkle, Tag, UsersThree } from '@phosphor-icons/react'
import type { AuthorDto, NotableEntityDto } from '@/lib/api/types'
import { sortAuthorsByRole } from '@/lib/utils/authors'
import { KNOWN_ENTITY_KINDS } from '@/lib/utils/notableEntities'
import { serializeAuthor, serializeNotableEntity } from '@/components/filters/filterUrl'
import type { DetailChipItem } from './DetailChipFlow'

const iconCls = 'size-3.5'

/** publisher + author chips of a detail hero; authors link to the browse page under `base` */
export function creatorChipItems(publisher: string, authors: AuthorDto[], base: '/series' | '/books'): DetailChipItem[] {
  const items: DetailChipItem[] = []
  if (publisher)
    items.push({
      key: `publisher-${publisher}`,
      label: publisher,
      icon: <Buildings className={iconCls} />,
      to: `/series?publishers=${encodeURIComponent(publisher)}`,
    })
  for (const a of sortAuthorsByRole(authors))
    items.push({
      key: `author-${a.name}-${a.role}`,
      label: (
        <>
          {a.name} {a.role && <span className="text-ink-3">({a.role})</span>}
        </>
      ),
      to: `${base}?authors=${encodeURIComponent(serializeAuthor(a))}`,
    })
  return items
}

export function genreChipItems(genres: string[]): DetailChipItem[] {
  return genres.map((g) => ({
    key: `genre-${g}`,
    label: g,
    icon: <MaskHappy className={iconCls} />,
    to: `/series?genres=${encodeURIComponent(g)}`,
  }))
}

export function tagChipItems(tags: string[], base: '/series' | '/books', locale: string, className?: string): DetailChipItem[] {
  return [...tags]
    .sort((a, b) => a.localeCompare(b, locale))
    .map((t) => ({
      key: `tag-${t}`,
      label: t,
      icon: <Tag className={iconCls} />,
      to: `${base}?tags=${encodeURIComponent(t)}`,
      className,
    }))
}

export interface NotableEntityChipGroup {
  kind: string
  items: DetailChipItem[]
}

/** one group per kind: known kinds first, customs in data order; names locale-sorted */
export function notableEntityChipGroups(entities: NotableEntityDto[], base: '/series' | '/books', locale: string): NotableEntityChipGroup[] {
  const byKind = new Map<string, NotableEntityDto[]>()
  for (const e of entities) {
    const list = byKind.get(e.kind) ?? []
    list.push(e)
    byKind.set(e.kind, list)
  }
  const rank = (kind: string) => {
    const i = (KNOWN_ENTITY_KINDS as readonly string[]).indexOf(kind)
    return i === -1 ? KNOWN_ENTITY_KINDS.length : i
  }
  return [...byKind.entries()]
    .sort((a, b) => rank(a[0]) - rank(b[0]))
    .map(([kind, list]) => ({
      kind,
      items: [...list]
        .sort((a, b) => a.name.localeCompare(b.name, locale))
        .map((e) => ({
          key: `entity-${e.kind}-${e.name}`,
          label: e.name,
          icon: <Sparkle className={iconCls} />,
          to: `${base}?notableEntities=${encodeURIComponent(serializeNotableEntity(e))}`,
        })),
    }))
}

/** book-only tags are dimmed to mark they come from the books, not the series; both link to the series browse because the backend's series tag search unions series-level and aggregated book tags */
export function seriesTagChipItems(seriesTags: string[], bookTags: string[], locale: string): DetailChipItem[] {
  const own = seriesTags.filter((t) => t !== '')
  const ownSet = new Set(own)
  const bookOnly = [...new Set(bookTags.filter((t) => t !== '' && !ownSet.has(t)))]
  return [...tagChipItems(own, '/series', locale), ...tagChipItems(bookOnly, '/series', locale, 'opacity-70')]
}

export function sharingLabelChipItems(labels: string[]): DetailChipItem[] {
  return labels.map((s) => ({ key: `sharing-${s}`, label: s, icon: <UsersThree className={iconCls} /> }))
}
