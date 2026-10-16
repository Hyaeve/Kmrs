import type { ReactNode } from 'react'
import { motion, useReducedMotion } from 'motion/react'

interface DetailHeroProps {
  /** cover URL reused as the blurred backdrop */
  backdrop: string
  /** rendered above the hero content, over the backdrop (e.g. back button) */
  leading?: ReactNode
  children: ReactNode
}

export function DetailHero({ backdrop, leading, children }: DetailHeroProps) {
  const reduce = useReducedMotion()
  return (
    <section className="relative">
      <div aria-hidden className="absolute -inset-x-4 -top-6 bottom-0 overflow-hidden md:-inset-x-8 md:-top-8">
        {/* div+backgroundImage: a 404 thumbnail must not show a broken-image icon */}
        <div
          className="size-full scale-125 bg-cover bg-center opacity-25 blur-3xl"
          style={{ backgroundImage: `url(${backdrop})` }}
        />
        <div className="absolute inset-0 bg-gradient-to-b from-transparent to-bg" />
      </div>
      {leading}
      <motion.div
        className="relative flex flex-col gap-6 pt-2 sm:flex-row"
        initial={reduce ? false : { opacity: 0, y: 12 }}
        animate={{ opacity: 1, y: 0 }}
        transition={{ duration: 0.4, ease: [0.16, 1, 0.3, 1] }}
      >
        {children}
      </motion.div>
    </section>
  )
}
