import { Navigate, useParams } from 'react-router-dom'

export function LibraryRedirect() {
  const { libraryId = '' } = useParams()
  return <Navigate to={`/libraries/${libraryId}/series`} replace />
}
