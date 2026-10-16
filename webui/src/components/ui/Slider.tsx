import * as RadixSlider from '@radix-ui/react-slider'
import { cn } from '@/lib/utils/cn'

interface SliderProps {
  value: number
  onValueChange: (v: number) => void
  min: number
  max: number
  step?: number
  label?: string
  className?: string
  dir?: 'ltr' | 'rtl'
}

export function Slider({ value, onValueChange, min, max, step = 1, label, className, dir }: SliderProps) {
  return (
    <RadixSlider.Root
      className={cn('relative flex h-5 w-full touch-none items-center select-none', className)}
      value={[value]}
      onValueChange={([v]) => onValueChange(v)}
      min={min}
      max={max}
      step={step}
      aria-label={label}
      dir={dir}
    >
      <RadixSlider.Track className="relative h-1 flex-1 rounded-full bg-overlay">
        <RadixSlider.Range className="absolute h-full rounded-full bg-accent" />
      </RadixSlider.Track>
      <RadixSlider.Thumb
        className={cn(
          'block size-4 cursor-grab rounded-full bg-white shadow-[0_1px_4px_rgb(0_0_0/0.4)] transition-transform',
          'hover:scale-110 focus:outline-none active:cursor-grabbing',
        )}
      />
    </RadixSlider.Root>
  )
}
