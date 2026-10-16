import { AnimatePresence, motion } from 'motion/react'
import { useToastStore } from '@/lib/store/toast'

export function Toaster() {
  const toast = useToastStore((s) => s.toast)
  return (
    <div className="pointer-events-none fixed inset-x-0 bottom-20 z-50 flex justify-center px-4">
      <AnimatePresence>
        {toast && (
          <motion.div
            key={toast.id}
            initial={{ opacity: 0, y: 8 }}
            animate={{ opacity: 1, y: 0 }}
            exit={{ opacity: 0 }}
            transition={{ duration: 0.18 }}
            className="max-w-full truncate rounded-full bg-black/80 px-4 py-2 text-[13px] text-white shadow-pop"
          >
            {toast.message}
          </motion.div>
        )}
      </AnimatePresence>
    </div>
  )
}
