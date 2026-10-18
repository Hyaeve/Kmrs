import { FadeKey } from '@/components/ui/FadeKey'
import { ClampedText } from '@/components/ui/ClampedText'

export function Summary({ text, className }: { text: string; className?: string }) {
  return (
    <FadeKey id={text} className={className}>
      <ClampedText text={text} className="max-w-3xl leading-relaxed text-ink-2" />
    </FadeKey>
  )
}
