import { Navigate, useSearchParams } from 'react-router-dom'

// the komf OAuth callback lands on /?oauth=...; forward it to the komf settings page,
// which toasts the outcome and strips the params
export function RootRedirect() {
  const [searchParams] = useSearchParams()
  if (searchParams.has('oauth')) {
    return <Navigate to={{ pathname: '/admin/integrations/komf', search: searchParams.toString() }} replace />
  }
  return <Navigate to="/dashboard" replace />
}
