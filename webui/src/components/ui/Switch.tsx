import * as RadixSwitch from '@radix-ui/react-switch'
import { cn } from '@/lib/utils/cn'

interface SwitchProps {
  checked: boolean
  onCheckedChange: (v: boolean) => void
  label?: string
  disabled?: boolean
}

export function Switch({ checked, onCheckedChange, label, disabled }: SwitchProps) {
  return (
    <RadixSwitch.Root
      checked={checked}
      onCheckedChange={onCheckedChange}
      disabled={disabled}
      aria-label={label}
      className={cn(
        'relative h-6 w-11 shrink-0 cursor-pointer rounded-full transition-colors duration-200',
        'data-[state=checked]:bg-accent data-[state=unchecked]:bg-overlay',
        'border border-line disabled:pointer-events-none disabled:opacity-50',
      )}
    >
      <RadixSwitch.Thumb
        className={cn(
          'block size-5 translate-x-[2px] rounded-full bg-white shadow-sm transition-transform duration-200',
          'data-[state=checked]:translate-x-[22px]',
        )}
      />
    </RadixSwitch.Root>
  )
}
