import { useNavigate, useParams } from 'react-router-dom'
import { useTranslation } from 'react-i18next'
import { SegmentedControl } from '@/components/ui/SegmentedControl'

const TABS = ['series', 'books'] as const
type Tab = (typeof TABS)[number]

/** komga-style navigation between a library's browse views. */
export function BrowseTabs({ tab }: { tab: Tab }) {
  const { t } = useTranslation('browse')
  const { libraryId } = useParams()
  const navigate = useNavigate()

  return (
    <div className="mb-5 overflow-x-auto pb-1">
      <SegmentedControl<Tab>
        options={TABS.map((value) => ({ value, label: t(`tabs.${value}`) }))}
        value={tab}
        onChange={(next) => navigate(`/libraries/${libraryId}/${next}`)}
      />
    </div>
  )
}
