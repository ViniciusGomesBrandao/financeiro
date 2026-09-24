'use client'

import * as React from 'react'
import {
  motion,
  useSpring,
  useTransform,
  type HTMLMotionProps,
  type MotionValue,
  type SpringOptions,
} from 'motion/react'
import useMeasure from 'react-use-measure'
import { cn } from '@/lib/utils'

type SlidingNumberRollerProps = {
  prevValue: number
  value: number
  place: number
  transition: SpringOptions
}

function SlidingNumberRoller({ prevValue, value, place, transition }: SlidingNumberRollerProps) {
  const startNumber = Math.floor(prevValue / place) % 10
  const targetNumber = Math.floor(value / place) % 10
  const animatedValue = useSpring(startNumber, transition)

  React.useEffect(() => {
    animatedValue.set(targetNumber)
  }, [targetNumber, animatedValue])

  const [measureRef, { height }] = useMeasure()

  return (
    <span
      ref={measureRef}
      className="relative inline-block w-[1ch] overflow-x-visible overflow-y-clip font-mono tabular-nums leading-none"
    >
      <span className="invisible">0</span>
      {Array.from({ length: 10 }, (_, i) => (
        <SlidingNumberDisplay
          key={i}
          motionValue={animatedValue}
          number={i}
          height={height}
          transition={transition}
        />
      ))}
    </span>
  )
}

function SlidingNumberDisplay({
  motionValue,
  number,
  height,
  transition,
}: {
  motionValue: MotionValue
  number: number
  height: number
  transition: SpringOptions
}) {
  const y = useTransform(motionValue, (latest) => {
    if (!height) return 0
    const currentNumber = latest % 10
    const offset = (10 + number - currentNumber) % 10
    let translateY = offset * height
    if (offset > 5) translateY -= 10 * height
    return translateY
  })

  if (!height) {
    return <span className="absolute inset-0 flex items-center justify-center">{number}</span>
  }

  return (
    <motion.span
      style={{ y }}
      transition={{ ...transition, type: 'spring' }}
      className="absolute inset-0 flex items-center justify-center"
    >
      {number}
    </motion.span>
  )
}

export type SlidingNumberProps = Omit<HTMLMotionProps<'span'>, 'children'> & {
  number: number
  decimalPlaces?: number
  thousandSeparator?: string
  decimalSeparator?: string
  transition?: SpringOptions
  className?: string
}

/** Animate UI Sliding Number — anima só quando o valor muda. */
export function SlidingNumber({
  number,
  decimalPlaces = 0,
  thousandSeparator = '.',
  decimalSeparator = ',',
  transition = { stiffness: 200, damping: 20, mass: 0.4 },
  className,
  ...props
}: SlidingNumberProps) {
  const abs = Math.abs(number)
  const factor = Math.pow(10, decimalPlaces)
  const scaled = Math.round(abs * factor) / factor
  const formatted = decimalPlaces > 0 ? scaled.toFixed(decimalPlaces) : String(Math.round(scaled))
  const [intRaw, decRaw = ''] = formatted.split('.')

  const prevRef = React.useRef(scaled)
  const prevFormatted = decimalPlaces > 0 ? prevRef.current.toFixed(decimalPlaces) : String(Math.round(prevRef.current))
  const [prevIntRaw = '', prevDecRaw = ''] = prevFormatted.split('.')

  React.useEffect(() => {
    prevRef.current = scaled
  }, [scaled])

  const intPlaces = React.useMemo(
    () => Array.from({ length: intRaw.length }, (_, i) => Math.pow(10, intRaw.length - i - 1)),
    [intRaw.length],
  )
  const decPlaces = React.useMemo(
    () =>
      decRaw
        ? Array.from({ length: decRaw.length }, (_, i) => Math.pow(10, decRaw.length - i - 1))
        : [],
    [decRaw],
  )

  const prevInt = prevIntRaw.padStart(intRaw.length, '0')
  const prevDec = prevDecRaw.padEnd(decRaw.length, '0')

  return (
    <motion.span
      data-slot="sliding-number"
      className={cn('inline-flex items-center font-mono tabular-nums', className)}
      {...props}
    >
      {number < 0 ? <span>-</span> : null}
      {intPlaces.map((place, idx) => {
        const digitsToRight = intPlaces.length - idx - 1
        const sep = digitsToRight > 0 && digitsToRight % 3 === 0
        return (
          <React.Fragment key={`int-${place}`}>
            <SlidingNumberRoller
              prevValue={parseInt(prevInt || '0', 10)}
              value={parseInt(intRaw || '0', 10)}
              place={place}
              transition={transition}
            />
            {sep ? <span>{thousandSeparator}</span> : null}
          </React.Fragment>
        )
      })}
      {decRaw ? (
        <>
          <span>{decimalSeparator}</span>
          {decPlaces.map((place) => (
            <SlidingNumberRoller
              key={`dec-${place}`}
              prevValue={parseInt(prevDec || '0', 10)}
              value={parseInt(decRaw || '0', 10)}
              place={place}
              transition={transition}
            />
          ))}
        </>
      ) : null}
    </motion.span>
  )
}

/** Atalho para valores monetários pt-BR com animação ao mudar. */
export function MoneyNumber({
  value,
  className,
}: {
  value: number | null
  className?: string
}) {
  if (value === null) return <span className={className}>—</span>
  return (
    <SlidingNumber
      number={value}
      decimalPlaces={2}
      thousandSeparator="."
      decimalSeparator=","
      className={className}
    />
  )
}
