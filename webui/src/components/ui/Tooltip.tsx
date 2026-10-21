import * as RadixTooltip from '@radix-ui/react-tooltip'
import type { ReactNode } from 'react'

// a single shared provider at the app root: Radix's instant re-open on adjacent
// tooltips only works within one Provider, per-tooltip Providers always wait the full delay
export function TooltipProvider({ children }: { children: ReactNode }) {
  return <RadixTooltip.Provider delayDuration={350}>{children}</RadixTooltip.Provider>
}

export function Tooltip({ content, children, side = 'top' }: { content: ReactNode; children: ReactNode; side?: 'top' | 'bottom' | 'left' | 'right' }) {
  return (
    <RadixTooltip.Root>
      <RadixTooltip.Trigger asChild>{children}</RadixTooltip.Trigger>
      <RadixTooltip.Portal>
        <RadixTooltip.Content
          side={side}
          sideOffset={6}
          className="z-50 max-w-sm rounded-lg border border-line bg-overlay px-2.5 py-1.5 text-xs break-words text-ink shadow-pop"
        >
          {content}
        </RadixTooltip.Content>
      </RadixTooltip.Portal>
    </RadixTooltip.Root>
  )
}
