import { motion } from 'motion/react'
import type { ReactNode } from 'react'

/** Refades its block whenever `id` changes, softening in-place content swaps after refetches (SSE, metadata edits). */
export function FadeKey({ id, children, className }: { id: string; children: ReactNode; className?: string }) {
  return (
    <motion.div
      key={id}
      className={className}
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      transition={{ duration: 0.25, ease: [0.16, 1, 0.3, 1] }}
    >
      {children}
    </motion.div>
  )
}
