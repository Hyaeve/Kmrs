import { useQuery } from '@tanstack/react-query'
import { librariesApi } from '@/lib/api/libraries'

/** An unavailable library makes its books and series act deleted (komga parity); reads the shared libraries cache. */
export function useLibraryUnavailable(libraryId: string): boolean {
  const { data } = useQuery({ queryKey: ['libraries'], queryFn: librariesApi.list })
  return data?.find((l) => l.id === libraryId)?.unavailable ?? false
}
