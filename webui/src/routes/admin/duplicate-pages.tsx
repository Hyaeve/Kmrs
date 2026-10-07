import { useState } from 'react'
import { useTranslation } from 'react-i18next'
import { useDocumentTitle } from '@/lib/hooks/useDocumentTitle'
import { PageHeader } from '@/components/ui/PageHeader'
import { SegmentedControl } from '@/components/ui/SegmentedControl'
import { KnownTab } from '@/components/admin/page-hashes/KnownTab'
import { UnknownTab } from '@/components/admin/page-hashes/UnknownTab'

type Tab = 'known' | 'unknown'

export function AdminDuplicatePagesPage() {
  const { t } = useTranslation('admin-maintenance')
  const [tab, setTab] = useState<Tab>('known')

  useDocumentTitle(t('layout:nav.duplicatePages'))

  return (
    <div className="max-w-6xl">
      <PageHeader
        title={t('layout:nav.duplicatePages')}
        subtitle={t('duplicatePages.subtitle')}
        actions={
          <SegmentedControl
            options={[
              { value: 'known', label: t('duplicatePages.known') },
              { value: 'unknown', label: t('common:state.unknown') },
            ]}
            value={tab}
            onChange={setTab}
          />
        }
      />
      {tab === 'known' ? <KnownTab /> : <UnknownTab />}
    </div>
  )
}
