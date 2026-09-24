export type Overview = {
  as_of: string | null
  cash: string | null
  equity: string | null
  realized_pnl_total: string | null
  realized_pnl_today: string | null
  unrealized_pnl: string | null
  return_pct: string | null
  max_drawdown: string
  open_positions_count: number | null
  exposure_ratio: string | null
}

export type Price = {
  symbol: string
  price: string
  updated_at: string
}

export type OpenPosition = {
  symbol: string
  strategy_id: string
  side: string
  quantity: string
  entry_price: string
  current_price: string | null
  unrealized_pnl: string | null
  opened_at: string
}

export type Trade = {
  symbol: string
  strategy_id: string
  side: string
  quantity: string
  entry_price: string
  exit_price: string
  opened_at: string
  closed_at: string
  pnl_gross: string
  fees_paid: string
  spread_paid: string
  slippage_paid: string
  pnl_net: string
}

export type StrategyPerformance = {
  strategy_id: string
  total_trades: number
  winners: number
  losers: number
  win_rate: string
  gross_pnl: string
  net_pnl: string
  average_win: string
  average_loss: string
  profit_factor: string | null
  max_drawdown: string
}

export type TimelineEntry = {
  created_at: string
  symbol: string
  strategy_id: string
  trigger: string
  approved: boolean
  reason: string | null
  signal_direction: string | null
  signal_confidence: number | null
  order_side: string | null
  order_quantity: string | null
  fill_price: string | null
  fee: string | null
  spread_cost: string | null
  slippage_cost: string | null
  pnl_gross: string | null
  pnl_net: string | null
}

export type DashboardData = {
  overview: Overview
  prices: Price[]
  positions: OpenPosition[]
  performance: StrategyPerformance[]
  trades: Trade[]
  timeline: TimelineEntry[]
}

export type StrategyCatalogEntry = {
  kind: string
  display_name: string
  description: string
  category: string
}

export type OperationalRobot = {
  id: string
  name: string
  symbol: string
  timeframe: string
  candidate_kinds: string[]
  strategy_instance_ids: string[]
  paper_capital: string
  status: 'running' | 'stopped'
  active_strategy_id: string | null
  judge_evaluated_at: string | null
  judge_mood: 'satisfied' | 'looking' | 'idle' | string
  active_why: string
  market_regime: string | null
  regime_strength: number | null
  regime_summary: string | null
  selection_fit_score: number | null
  candidate_fits: Array<{
    strategy_id: string
    strategy_kind: string
    fit_score: number
    economic_state: string
  }> | null
  cash: string | null
  equity: string | null
  unrealized_pnl: string | null
  return_pct: string | null
  open_positions_count: number
  closed_trades_count: number
  net_pnl: string
  win_rate: string
  profit_factor: string | null
  expectancy: string
  max_drawdown: string
  created_at: string
  updated_at: string
  engine_restart_required: boolean
}

export type CreateRobotInput = {
  id: string
  name: string
  symbol: string
  timeframe: string
  candidate_kinds: string[]
  paper_capital: string
}

export type JudgeDecisionJson = {
  timestamp: string
  strategy_id: string
  instrument_id: string
  state: string
  metrics: {
    trades: number
    net_pnl: string
    expectancy: string
    profit_factor: string | null
    max_drawdown: string
    win_rate: string
  }
  reason: {
    kind: string
    trades?: number
    required?: number
    win_rate?: string
    min?: string
    profit_factor?: string
    expectancy?: string
    drawdown?: string
    max?: string
    proposed?: string
    confirmations?: number
  }
}

export type JudgeEvaluation = {
  id: string
  symbol: string
  robot_id: string | null
  evaluated_at: string
  selected_strategy_id: string | null
  decisions: JudgeDecisionJson[]
}

export type StrategySwitch = {
  id: string
  symbol: string
  robot_id: string | null
  previous_strategy_id: string | null
  new_strategy_id: string | null
  reason: JudgeDecisionJson['reason']
  switched_at: string
}

export type PnlCurvePoint = {
  at: string
  cumulative_pnl: string
}

export type RobotDetail = {
  robot: OperationalRobot
  trades: Trade[]
  evaluations: JudgeEvaluation[]
  switches: StrategySwitch[]
  realized_pnl_curve: PnlCurvePoint[]
  candidate_performance: StrategyPerformance[]
}

export type OperationalData = {
  robots: OperationalRobot[]
  catalog: StrategyCatalogEntry[]
  evaluations: JudgeEvaluation[]
  switches: StrategySwitch[]
}
