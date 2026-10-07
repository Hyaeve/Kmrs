import { api } from './client'
import type {
  ThumbnailBookDto,
  ThumbnailReadListDto,
  ThumbnailSeriesCollectionDto,
  ThumbnailSeriesDto,
  ThumbnailSmartListDto,
} from './types'

/** Multipart poster upload: `file` part is required; `selected` is a text part, default true. */
function uploadForm(file: Blob, selected: boolean): FormData {
  const form = new FormData()
  form.append('file', file)
  form.append('selected', String(selected))
  return form
}

export const seriesPostersApi = {
  list: (seriesId: string) => api.get<ThumbnailSeriesDto[]>(`/api/v1/series/${seriesId}/thumbnails`),
  upload: (seriesId: string, file: Blob, selected = true) =>
    api.upload<ThumbnailSeriesDto>(`/api/v1/series/${seriesId}/thumbnails`, uploadForm(file, selected)),
  markSelected: (seriesId: string, thumbnailId: string) =>
    api.put<void>(`/api/v1/series/${seriesId}/thumbnails/${thumbnailId}/selected`),
  delete: (seriesId: string, thumbnailId: string) =>
    api.delete<void>(`/api/v1/series/${seriesId}/thumbnails/${thumbnailId}`),
  thumbnailUrl: (seriesId: string, thumbnailId: string) => `/api/v1/series/${seriesId}/thumbnails/${thumbnailId}`,
}

export const bookPostersApi = {
  list: (bookId: string) => api.get<ThumbnailBookDto[]>(`/api/v1/books/${bookId}/thumbnails`),
  upload: (bookId: string, file: Blob, selected = true) =>
    api.upload<ThumbnailBookDto>(`/api/v1/books/${bookId}/thumbnails`, uploadForm(file, selected)),
  markSelected: (bookId: string, thumbnailId: string) =>
    api.put<void>(`/api/v1/books/${bookId}/thumbnails/${thumbnailId}/selected`),
  delete: (bookId: string, thumbnailId: string) =>
    api.delete<void>(`/api/v1/books/${bookId}/thumbnails/${thumbnailId}`),
  thumbnailUrl: (bookId: string, thumbnailId: string) => `/api/v1/books/${bookId}/thumbnails/${thumbnailId}`,
}

export const collectionPostersApi = {
  list: (collectionId: string) =>
    api.get<ThumbnailSeriesCollectionDto[]>(`/api/v1/collections/${collectionId}/thumbnails`),
  upload: (collectionId: string, file: Blob, selected = true) =>
    api.upload<ThumbnailSeriesCollectionDto>(`/api/v1/collections/${collectionId}/thumbnails`, uploadForm(file, selected)),
  markSelected: (collectionId: string, thumbnailId: string) =>
    api.put<void>(`/api/v1/collections/${collectionId}/thumbnails/${thumbnailId}/selected`),
  delete: (collectionId: string, thumbnailId: string) =>
    api.delete<void>(`/api/v1/collections/${collectionId}/thumbnails/${thumbnailId}`),
  thumbnailUrl: (collectionId: string, thumbnailId: string) =>
    `/api/v1/collections/${collectionId}/thumbnails/${thumbnailId}`,
}

export const readListPostersApi = {
  list: (readListId: string) => api.get<ThumbnailReadListDto[]>(`/api/v1/readlists/${readListId}/thumbnails`),
  upload: (readListId: string, file: Blob, selected = true) =>
    api.upload<ThumbnailReadListDto>(`/api/v1/readlists/${readListId}/thumbnails`, uploadForm(file, selected)),
  markSelected: (readListId: string, thumbnailId: string) =>
    api.put<void>(`/api/v1/readlists/${readListId}/thumbnails/${thumbnailId}/selected`),
  delete: (readListId: string, thumbnailId: string) =>
    api.delete<void>(`/api/v1/readlists/${readListId}/thumbnails/${thumbnailId}`),
  thumbnailUrl: (readListId: string, thumbnailId: string) => `/api/v1/readlists/${readListId}/thumbnails/${thumbnailId}`,
}

export const smartListPostersApi = {
  list: (smartListId: string) => api.get<ThumbnailSmartListDto[]>(`/api/v1/smart-lists/${smartListId}/thumbnails`),
  upload: (smartListId: string, file: Blob, selected = true) =>
    api.upload<ThumbnailSmartListDto>(`/api/v1/smart-lists/${smartListId}/thumbnails`, uploadForm(file, selected)),
  markSelected: (smartListId: string, thumbnailId: string) =>
    api.put<void>(`/api/v1/smart-lists/${smartListId}/thumbnails/${thumbnailId}/selected`),
  delete: (smartListId: string, thumbnailId: string) =>
    api.delete<void>(`/api/v1/smart-lists/${smartListId}/thumbnails/${thumbnailId}`),
  thumbnailUrl: (smartListId: string, thumbnailId: string) =>
    `/api/v1/smart-lists/${smartListId}/thumbnails/${thumbnailId}`,
}
