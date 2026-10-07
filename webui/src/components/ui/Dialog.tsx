import * as RadixDialog from '@radix-ui/react-dialog'
import { motion, AnimatePresence } from 'motion/react'
import { X } from '@phosphor-icons/react'
import { useTranslation } from 'react-i18next'
import { cn } from '@/lib/utils/cn'
import { IconButton } from './IconButton'
import type { ReactNode } from 'react'

interface DialogProps {
  open: boolean
  onOpenChange: (open: boolean) => void
  title?: string
  children: ReactNode
  className?: string
  /** width preset */
  size?: 'sm' | 'md' | 'lg'
  /**
   * children manage their own scrolling (e.g. a sticky-footer layout): the content area
   * becomes a non-scrolling flex column instead of the default scroll container, because
   * percentage heights resolve against a scroll container's content height, not its box
   */
  fill?: boolean
}

const sizes = { sm: 'max-w-sm', md: 'max-w-lg', lg: 'max-w-3xl' }

export function Dialog({ open, onOpenChange, title, children, className, size = 'md', fill = false }: DialogProps) {
  const { t } = useTranslation()
  return (
    <RadixDialog.Root open={open} onOpenChange={onOpenChange}>
      <AnimatePresence>
        {open && (
          <RadixDialog.Portal forceMount>
            <RadixDialog.Overlay asChild>
              <motion.div
                className="fixed inset-0 z-40 bg-black/60 backdrop-blur-[2px]"
                initial={{ opacity: 0 }}
                animate={{ opacity: 1 }}
                exit={{ opacity: 0 }}
                transition={{ duration: 0.18 }}
              />
            </RadixDialog.Overlay>
            <div className="fixed inset-0 z-40 flex items-center justify-center p-4">
              <RadixDialog.Content asChild>
                <motion.div
                  className={cn(
                    'flex max-h-[85dvh] w-full flex-col overflow-hidden rounded-xl border border-line bg-surface shadow-pop',
                    sizes[size],
                    className,
                  )}
                  initial={{ opacity: 0, scale: 0.96, y: 8 }}
                  animate={{ opacity: 1, scale: 1, y: 0 }}
                  exit={{ opacity: 0, scale: 0.97, y: 4 }}
                  transition={{ duration: 0.2, ease: [0.16, 1, 0.3, 1] }}
                >
                  {title && (
                    <div className="flex items-center justify-between border-b border-line px-5 py-3.5">
                      <RadixDialog.Title className="text-[15px] font-semibold">{title}</RadixDialog.Title>
                      <RadixDialog.Close asChild>
                        <IconButton label={t('action.close')}>
                          <X className="size-4" />
                        </IconButton>
                      </RadixDialog.Close>
                    </div>
                  )}
                  <div
                    className={cn(
                      'min-h-0 flex-1',
                      fill ? 'flex flex-col overflow-hidden' : 'overflow-y-auto',
                    )}
                  >
                    {children}
                  </div>
                </motion.div>
              </RadixDialog.Content>
            </div>
          </RadixDialog.Portal>
        )}
      </AnimatePresence>
    </RadixDialog.Root>
  )
}
