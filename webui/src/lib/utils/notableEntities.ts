import i18n from '@/lib/i18n'

/** known kinds in display order; anything else keeps its raw server name */
export const KNOWN_ENTITY_KINDS = ['character', 'team', 'location'] as const

/** kind labels are shared vocabulary (common namespace); custom kinds render raw */
export function entityKindLabel(kind: string): string {
  return i18n.t(`common:notableEntityKind.${kind}`, { defaultValue: kind })
}
