import i18n from '@/lib/i18n'
import type { BookDto, MediaStatus } from '@/lib/api/types'

// same ERR_xxxx set the server (kmrs, like komga) puts in media.comment and error messages;
// server-originated text stays in English, only UI-owned labels are localized
const ERROR_CODE_MESSAGES: Record<string, string> = {
  ERR_1000: 'File could not be accessed during analysis',
  ERR_1001: 'Media type is not supported',
  ERR_1002: 'Encrypted RAR archives are not supported',
  ERR_1003: 'Solid RAR archives are not supported',
  ERR_1004: 'Multi-Volume RAR archives are not supported',
  ERR_1005: 'Unknown error while analyzing book',
  ERR_1006: 'Book does not contain any page',
  ERR_1007: 'Some entries could not be analyzed',
  ERR_1008: "Unknown error while getting book's entries",
  ERR_1009: 'A read list with that name already exists',
  ERR_1015: 'Error while deserializing ComicRack CBL',
  ERR_1016: 'Directory not accessible or not a directory',
  ERR_1017: 'Cannot scan folder that is part of an existing library',
  ERR_1018: 'File not found',
  ERR_1019: 'Cannot import file that is part of an existing library',
  ERR_1020: 'Book to upgrade does not belong to provided series',
  ERR_1021: 'Destination file already exists',
  ERR_1022: 'Newly imported book could not be scanned',
  ERR_1023: 'Book already present in read list',
  ERR_1024: 'OAuth2 login error: no email attribute',
  ERR_1025: 'OAuth2 login error: no local user exist with that email',
  ERR_1026: 'OpenID Connect login error: email not verified',
  ERR_1027: 'OpenID Connect login error: no email_verified attribute',
  ERR_1028: 'OpenID Connect login error: no email attribute',
  ERR_1029: 'ComicRack CBL does not contain any Book element',
  ERR_1030: 'ComicRack CBL has no Name element',
  ERR_1031: 'ComicRack CBL Book is missing series or number',
  ERR_1032: 'EPUB file has wrong media type',
  ERR_1033: 'Some entries are missing',
  ERR_1034: 'An API key with that comment already exists',
  ERR_1035: 'Error while getting EPUB TOC',
  ERR_1036: 'Error while getting EPUB Landmarks',
  ERR_1037: 'Error while getting EPUB page list',
  ERR_1038: 'Error while getting EPUB divina pages',
  ERR_1039: 'Error while getting EPUB positions',
}

/** Replaces ERR_xxxx codes in a server message with readable text, keeping the rest of the sentence. */
export function convertErrorCodes(message: string): string {
  return message.replace(/ERR_\d{4}/g, (code) => ERROR_CODE_MESSAGES[code] ?? code)
}

export interface MediaIssue {
  severity: 'danger' | 'accent' | 'muted'
  title: string
  detail?: string
}

/** Why a book cannot be opened in the web reader, or null when it can. */
export function mediaIssue(book: Pick<BookDto, 'media' | 'deleted'>): MediaIssue | null {
  const { media } = book
  // deleted wins over any media status: the file is gone, nothing else applies (komga parity)
  if (book.deleted) return { severity: 'danger', title: i18n.t('mediaIssue.unavailable') }
  const detail = media.comment ? convertErrorCodes(media.comment) : undefined
  switch (media.status) {
    case 'ERROR':
      return { severity: 'danger', title: i18n.t('mediaIssue.error'), detail }
    case 'UNSUPPORTED':
      return { severity: 'accent', title: i18n.t('mediaIssue.unsupported'), detail }
    case 'OUTDATED':
      return { severity: 'accent', title: i18n.t('mediaIssue.outdated') }
    case 'UNKNOWN':
      return { severity: 'muted', title: i18n.t('mediaIssue.unknown') }
    default:
      break
  }
  return null
}

/**
 * Short status text for the card secondary line, replacing the normal meta; undefined keeps it.
 * deleted beats media status and OUTDATED stays hidden on cards, like komga's card body line.
 */
export function cardStatusLabel(item: { deleted: boolean; mediaStatus?: MediaStatus }): { text: string; className: string } | undefined {
  if (item.deleted) return { text: i18n.t('cardStatus.unavailable'), className: 'font-medium text-danger' }
  switch (item.mediaStatus) {
    case 'ERROR':
      return { text: i18n.t('cardStatus.error'), className: 'font-medium text-danger' }
    case 'UNSUPPORTED':
      return { text: i18n.t('cardStatus.unsupported'), className: 'font-medium text-accent-strong' }
    case 'UNKNOWN':
      return { text: i18n.t('cardStatus.toBeAnalyzed'), className: '' }
    default:
      return undefined
  }
}
