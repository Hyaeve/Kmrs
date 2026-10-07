export const urls = {
  seriesThumbnail: (seriesId: string, bust?: string | number) =>
    `/api/v1/series/${seriesId}/thumbnail${bust ? `?${bust}` : ''}`,
  bookThumbnail: (bookId: string, bust?: string | number) =>
    `/api/v1/books/${bookId}/thumbnail${bust ? `?${bust}` : ''}`,
  collectionThumbnail: (collectionId: string, bust?: string | number) =>
    `/api/v1/collections/${collectionId}/thumbnail${bust ? `?${bust}` : ''}`,
  readlistThumbnail: (readListId: string, bust?: string | number) =>
    `/api/v1/readlists/${readListId}/thumbnail${bust ? `?${bust}` : ''}`,
  smartlistThumbnail: (smartListId: string, bust?: string | number) =>
    `/api/v1/smart-lists/${smartListId}/thumbnail${bust ? `?${bust}` : ''}`,
  bookPage: (bookId: string, page: number, opts?: { convert?: 'jpeg' | 'png' }) =>
    `/api/v1/books/${bookId}/pages/${page}${opts?.convert ? `?convert=${opts.convert}` : ''}`,
  bookPageThumbnail: (bookId: string, page: number) => `/api/v1/books/${bookId}/pages/${page}/thumbnail`,
  bookFile: (bookId: string) => `/api/v1/books/${bookId}/file`,
  seriesFile: (seriesId: string) => `/api/v1/series/${seriesId}/file`,
}
