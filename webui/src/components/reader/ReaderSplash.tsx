import { CircleNotch } from '@phosphor-icons/react'

export function ReaderSplash() {
  return (
    <div className="fixed inset-0 flex items-center justify-center bg-black">
      <CircleNotch className="size-8 animate-spin text-white/60" />
    </div>
  )
}
