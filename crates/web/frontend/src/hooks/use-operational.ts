import { useCallback, useEffect, useState } from 'react'
import { createRobot, fetchOperational, setRobotStatus } from '@/api/client'
import type { CreateRobotInput, OperationalData } from '@/api/types'
import { REFRESH_MS } from '@/hooks/use-dashboard'

export function useOperational() {
  const [data, setData] = useState<OperationalData | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)
  const [busy, setBusy] = useState(false)

  const refresh = useCallback(async () => {
    try {
      const next = await fetchOperational()
      setData(next)
      setError(null)
    } catch (err) {
      setError(err instanceof Error ? err.message : String(err))
    } finally {
      setLoading(false)
    }
  }, [])

  useEffect(() => {
    void refresh()
    const id = window.setInterval(() => void refresh(), REFRESH_MS)
    return () => window.clearInterval(id)
  }, [refresh])

  const create = useCallback(
    async (input: CreateRobotInput) => {
      setBusy(true)
      try {
        await createRobot(input)
        await refresh()
      } finally {
        setBusy(false)
      }
    },
    [refresh],
  )

  const toggleStatus = useCallback(
    async (id: string, status: 'running' | 'stopped') => {
      setBusy(true)
      try {
        await setRobotStatus(id, status)
        await refresh()
      } finally {
        setBusy(false)
      }
    },
    [refresh],
  )

  return { data, error, loading, busy, refresh, create, toggleStatus }
}
