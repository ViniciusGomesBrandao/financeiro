import type {
  CreateRobotInput,
  DashboardData,
  OperationalData,
  OperationalRobot,
  RobotDetail,
  StrategyCatalogEntry,
} from './types'

async function getJson<T>(path: string): Promise<T> {
  const res = await fetch(path)
  if (!res.ok) throw new Error(`${path}: HTTP ${res.status}`)
  return res.json() as Promise<T>
}

async function sendJson<T>(
  path: string,
  method: string,
  body?: unknown,
): Promise<T> {
  const res = await fetch(path, {
    method,
    headers: body ? { 'Content-Type': 'application/json' } : undefined,
    body: body ? JSON.stringify(body) : undefined,
  })
  if (!res.ok) {
    const text = await res.text()
    throw new Error(text || `${path}: HTTP ${res.status}`)
  }
  return res.json() as Promise<T>
}

export async function fetchDashboard(): Promise<DashboardData> {
  const [overview, prices, positions, performance, trades, timeline] = await Promise.all([
    getJson<DashboardData['overview']>('/api/overview'),
    getJson<DashboardData['prices']>('/api/prices'),
    getJson<DashboardData['positions']>('/api/positions'),
    getJson<DashboardData['performance']>('/api/performance'),
    getJson<DashboardData['trades']>('/api/trades'),
    getJson<DashboardData['timeline']>('/api/timeline'),
  ])
  return { overview, prices, positions, performance, trades, timeline }
}

export async function fetchOperational(): Promise<OperationalData> {
  const [robots, catalog, evaluations, switches] = await Promise.all([
    getJson<OperationalRobot[]>('/api/robots'),
    getJson<StrategyCatalogEntry[]>('/api/strategy-catalog'),
    getJson<OperationalData['evaluations']>('/api/judge/evaluations'),
    getJson<OperationalData['switches']>('/api/strategy-switches'),
  ])
  return { robots, catalog, evaluations, switches }
}

export async function fetchRobotDetail(id: string): Promise<RobotDetail> {
  return getJson<RobotDetail>(`/api/robots/${encodeURIComponent(id)}`)
}

export async function createRobot(input: CreateRobotInput): Promise<OperationalRobot> {
  return sendJson<OperationalRobot>('/api/robots', 'POST', input)
}

export async function setRobotStatus(
  id: string,
  status: 'running' | 'stopped',
): Promise<OperationalRobot> {
  return sendJson<OperationalRobot>(`/api/robots/${encodeURIComponent(id)}/status`, 'PATCH', {
    status,
  })
}