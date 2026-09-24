import { useCallback, useEffect, useState } from 'react'
import { fetchDashboard } from '@/api/client'
import type { DashboardData } from '@/api/types'

export const REFRESH_MS = 5000

export function useDashboard() {
  const [data, setData] = useState<DashboardData | null>(null)
  const [error, setError] = useState<string | null>(null)
  const [loading, setLoading] = useState(true)

  const refresh = useCallback(async () => {
    try {
      const next = await fetchDashboard()
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

  return { data, error, loading, refresh }
}
