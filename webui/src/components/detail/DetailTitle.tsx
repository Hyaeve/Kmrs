import { motion } from 'motion/react'

/** Detail hero title; refades when the title changes (metadata edits, SSE refreshes). */
export function DetailTitle({ title }: { title: string }) {
  return (
    <motion.h1
      key={title}
      initial={{ opacity: 0 }}
      animate={{ opacity: 1 }}
      transition={{ duration: 0.25, ease: [0.16, 1, 0.3, 1] }}
      className="mt-1 font-display text-3xl font-semibold tracking-tight text-ink md:text-4xl"
    >
      {title}
    </motion.h1>
  )
}
