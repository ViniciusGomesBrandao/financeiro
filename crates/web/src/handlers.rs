//! Handlers HTTP. Cada um só lê do Postgres (via `persistence`) e, quando
//! precisa de um cálculo, reaproveita uma função já existente em
//! `domain`/`analytics` — nenhuma regra financeira nova é escrita aqui.

use std::collections::HashMap;

use axum::extract::State;
use axum::Json;
use domain::InstrumentId;
use sqlx::PgPool;

use crate::dto::{
    OpenPositionDto, OverviewDto, PriceDto, StrategyPerformanceDto, TimelineEntryDto, TradeDto,
};
use crate::error::AppError;
use crate::state::AppState;

/// Identifica `strategy_id`s de testes de integração/smoke test (prefixo
/// `smoke-test-` ou `restart-test-`, ex.: `smoke-test-ema-crossover`) que
/// rodam contra o mesmo Postgres de desenvolvimento usado pelo
/// `quant-engine` real (ver `crates/app/tests/`). Não é uma regra de
/// negócio: só evita que dados de teste contaminem os KPIs e a timeline
/// que um usuário real vê no dashboard. Mesmo padrão usado em
/// `app::setup::is_test_strategy` (duplicado deliberadamente — `web` não
/// depende de `app` — mas esta cópia é só para leitura; a de `app` é a que
/// importa de verdade, pois exclui essas posições do ledger da própria
/// instância ao vivo do `PortfolioManager`, não só da exibição).
fn is_test_strategy(strategy_id: &str) -> bool {
    strategy_id.starts_with("smoke-test-") || strategy_id.starts_with("restart-test-")
}

/// Mapa instrument_id -> symbol, usado para enriquecer registros que só
/// guardam o id. Os `instruments` cadastrados são poucos (um por par
/// monitorado), então buscar todos a cada requisição é desprezível.
pub(crate) async fn symbol_lookup(
    pool: &PgPool,
) -> Result<HashMap<InstrumentId, String>, AppError> {
    let instruments = persistence::instruments::list_all(pool).await?;
    Ok(instruments
        .into_iter()
        .map(|i| (i.id, i.symbol.to_string()))
        .collect())
}

pub async fn overview(State(state): State<AppState>) -> Result<Json<OverviewDto>, AppError> {
    // Fase multi-robô: o motor grava em `robot_portfolio_snapshots`.
    // Agregamos o último snapshot de cada robô. Fallback na tabela legada
    // `portfolio_snapshots` só se ainda não houver nenhum robô persistido.
    let robot_snaps = persistence::robot_portfolio_snapshots::latest_all(&state.pool).await?;
    let aggregated = aggregate_robot_snapshots(&robot_snaps);
    let legacy = if aggregated.is_none() {
        persistence::portfolio_snapshots::latest(&state.pool).await?
    } else {
        None
    };

    let closed_positions: Vec<domain::Position> = persistence::positions::list_closed(&state.pool)
        .await?
        .into_iter()
        .filter(|p| !is_test_strategy(p.strategy_id.as_str()))
        .collect();
    let performance = analytics::compute_performance(&closed_positions);

    let today = aggregated
        .as_ref()
        .map(|s| s.timestamp)
        .or_else(|| legacy.as_ref().map(|s| s.timestamp))
        .unwrap_or_else(chrono::Utc::now);
    let closed_today: Vec<domain::Position> = closed_positions
        .iter()
        .filter(|p| p.closed_at.is_some_and(|c| is_same_utc_day(c, today)))
        .cloned()
        .collect();
    let realized_pnl_total = performance.net_pnl;
    let realized_pnl_today = analytics::compute_performance(&closed_today).net_pnl;

    let latest_prices = persistence::latest_prices::list_all(&state.pool).await?;
    let price_by_instrument: HashMap<_, _> = latest_prices
        .into_iter()
        .map(|p| (p.instrument_id, p.price))
        .collect();
    let open_positions: Vec<domain::Position> = persistence::positions::list_open(&state.pool)
        .await?
        .into_iter()
        .filter(|p| !is_test_strategy(p.strategy_id.as_str()))
        .collect();
    let unrealized_pnl: rust_decimal::Decimal = open_positions
        .iter()
        .map(|p| {
            let mark = price_by_instrument
                .get(&p.instrument_id)
                .copied()
                .unwrap_or(p.entry_price);
            p.unrealized_pnl(mark)
        })
        .sum();

    let has_portfolio = aggregated.is_some() || legacy.is_some();
    let cash = aggregated
        .as_ref()
        .map(|s| s.cash)
        .or_else(|| legacy.as_ref().map(|s| s.cash));
    let equity = aggregated
        .as_ref()
        .map(|s| s.equity)
        .or_else(|| legacy.as_ref().map(|s| s.equity));
    let return_pct = aggregated
        .as_ref()
        .map(|s| s.return_pct)
        .or_else(|| legacy.as_ref().map(|s| s.return_pct));
    let exposure_ratio = aggregated
        .as_ref()
        .map(|s| s.exposure_ratio)
        .or_else(|| legacy.as_ref().map(|s| s.exposure_ratio));
    let as_of = aggregated
        .as_ref()
        .map(|s| s.timestamp)
        .or_else(|| legacy.as_ref().map(|s| s.timestamp));

    Ok(Json(OverviewDto {
        as_of,
        cash,
        equity,
        realized_pnl_total: has_portfolio.then_some(realized_pnl_total),
        realized_pnl_today: has_portfolio.then_some(realized_pnl_today),
        unrealized_pnl: has_portfolio.then_some(unrealized_pnl),
        return_pct,
        max_drawdown: performance.max_drawdown,
        open_positions_count: has_portfolio.then_some(open_positions.len() as u32),
        exposure_ratio,
    }))
}

/// Soma caixa/equity dos robôs e pondera retorno/exposição pelo equity.
fn aggregate_robot_snapshots(
    snaps: &[persistence::robot_portfolio_snapshots::RobotPortfolioSnapshot],
) -> Option<domain::PortfolioSnapshot> {
    if snaps.is_empty() {
        return None;
    }
    let mut cash = rust_decimal::Decimal::ZERO;
    let mut equity = rust_decimal::Decimal::ZERO;
    let mut return_weighted = rust_decimal::Decimal::ZERO;
    let mut exposure_weighted = rust_decimal::Decimal::ZERO;
    let mut timestamp = snaps[0].snapshot.timestamp;
    for s in snaps {
        cash += s.snapshot.cash;
        equity += s.snapshot.equity;
        return_weighted += s.snapshot.return_pct * s.snapshot.equity;
        exposure_weighted += s.snapshot.exposure_ratio * s.snapshot.equity;
        if s.snapshot.timestamp > timestamp {
            timestamp = s.snapshot.timestamp;
        }
    }
    let (return_pct, exposure_ratio) = if equity > rust_decimal::Decimal::ZERO {
        (return_weighted / equity, exposure_weighted / equity)
    } else {
        (rust_decimal::Decimal::ZERO, rust_decimal::Decimal::ZERO)
    };
    Some(domain::PortfolioSnapshot {
        timestamp,
        cash,
        equity,
        realized_pnl: rust_decimal::Decimal::ZERO,
        unrealized_pnl: rust_decimal::Decimal::ZERO,
        open_positions_count: 0,
        exposure_ratio,
        return_pct,
        realized_pnl_today: rust_decimal::Decimal::ZERO,
    })
}

fn is_same_utc_day(a: chrono::DateTime<chrono::Utc>, b: chrono::DateTime<chrono::Utc>) -> bool {
    use chrono::Datelike;
    a.year() == b.year() && a.ordinal() == b.ordinal()
}

pub async fn prices(State(state): State<AppState>) -> Result<Json<Vec<PriceDto>>, AppError> {
    let symbols = symbol_lookup(&state.pool).await?;
    let prices = persistence::latest_prices::list_all(&state.pool).await?;

    let mut dtos: Vec<PriceDto> = prices
        .into_iter()
        .filter_map(|p| {
            symbols.get(&p.instrument_id).map(|symbol| PriceDto {
                symbol: symbol.clone(),
                price: p.price,
                updated_at: p.updated_at,
            })
        })
        .collect();
    dtos.sort_by(|a, b| a.symbol.cmp(&b.symbol));
    Ok(Json(dtos))
}

pub async fn positions(
    State(state): State<AppState>,
) -> Result<Json<Vec<OpenPositionDto>>, AppError> {
    let symbols = symbol_lookup(&state.pool).await?;
    let latest_prices = persistence::latest_prices::list_all(&state.pool).await?;
    let price_by_instrument: HashMap<_, _> = latest_prices
        .into_iter()
        .map(|p| (p.instrument_id, p.price))
        .collect();

    let open = persistence::positions::list_open(&state.pool).await?;
    let dtos = open
        .into_iter()
        .filter(|p| !is_test_strategy(p.strategy_id.as_str()))
        .map(|position| {
            let current_price = price_by_instrument.get(&position.instrument_id).copied();
            // Reaproveita domain::Position::unrealized_pnl — a mesma
            // função que risk/portfolio usam internamente, não uma
            // reimplementação para a UI.
            let unrealized_pnl = current_price.map(|price| position.unrealized_pnl(price));
            OpenPositionDto {
                symbol: symbols
                    .get(&position.instrument_id)
                    .cloned()
                    .unwrap_or_else(|| "?".to_string()),
                strategy_id: position.strategy_id.as_str().to_string(),
                side: format!("{:?}", position.side),
                quantity: position.quantity,
                entry_price: position.entry_price,
                current_price,
                unrealized_pnl,
                opened_at: position.opened_at,
            }
        })
        .collect();
    Ok(Json(dtos))
}

pub async fn trades(State(state): State<AppState>) -> Result<Json<Vec<TradeDto>>, AppError> {
    let symbols = symbol_lookup(&state.pool).await?;
    let trades = persistence::trades::list_all(&state.pool).await?;

    let dtos = trades
        .into_iter()
        .filter(|t| !is_test_strategy(t.strategy_id.as_str()))
        .take(200)
        .map(|trade| TradeDto {
            id: Some(trade.id),
            symbol: symbols
                .get(&trade.instrument_id)
                .cloned()
                .unwrap_or_else(|| "?".to_string()),
            strategy_id: trade.strategy_id.as_str().to_string(),
            side: format!("{:?}", trade.side),
            quantity: trade.quantity,
            entry_price: trade.entry_price,
            exit_price: trade.exit_price,
            opened_at: trade.opened_at,
            closed_at: trade.closed_at,
            pnl_gross: trade.pnl_gross,
            fees_paid: trade.fees_paid,
            spread_paid: trade.spread_paid,
            slippage_paid: trade.slippage_paid,
            pnl_net: trade.pnl_net,
            exit_trigger: None,
            exit_reason: None,
            entry_direction: None,
            entry_confidence: None,
        })
        .collect();
    Ok(Json(dtos))
}

pub async fn performance(
    State(state): State<AppState>,
) -> Result<Json<Vec<StrategyPerformanceDto>>, AppError> {
    let closed = persistence::positions::list_closed(&state.pool).await?;

    let mut by_strategy: HashMap<String, Vec<domain::Position>> = HashMap::new();
    for position in closed
        .into_iter()
        .filter(|p| !is_test_strategy(p.strategy_id.as_str()))
    {
        by_strategy
            .entry(position.strategy_id.as_str().to_string())
            .or_default()
            .push(position);
    }

    let mut dtos: Vec<StrategyPerformanceDto> = by_strategy
        .into_iter()
        .map(|(strategy_id, positions)| {
            // Mesma função pura usada para a visão geral — só muda o
            // subconjunto de posições fechadas que ela recebe.
            let report = analytics::compute_performance(&positions);
            StrategyPerformanceDto {
                strategy_id,
                total_trades: report.total_trades,
                winners: report.winners,
                losers: report.losers,
                win_rate: report.win_rate,
                gross_pnl: report.gross_pnl,
                net_pnl: report.net_pnl,
                average_win: report.average_win,
                average_loss: report.average_loss,
                profit_factor: report.profit_factor,
                max_drawdown: report.max_drawdown,
            }
        })
        .collect();
    dtos.sort_by(|a, b| a.strategy_id.cmp(&b.strategy_id));
    Ok(Json(dtos))
}

pub async fn timeline(
    State(state): State<AppState>,
) -> Result<Json<Vec<TimelineEntryDto>>, AppError> {
    let entries = persistence::risk_decisions::list_timeline(&state.pool, 100).await?;
    Ok(Json(
        entries
            .into_iter()
            .filter(|e| !is_test_strategy(&e.strategy_id))
            .map(TimelineEntryDto::from)
            .map(|mut dto| {
                // Rede de segurança para linhas gravadas antes de
                // `app::pipeline::translate_rejection_reason` existir —
                // toda decisão nova já é persistida em português; isto só
                // traduz o que ficou preso em inglês no Postgres.
                dto.reason = dto.reason.map(|r| crate::i18n::translate_legacy_reason(&r));
                dto
            })
            .collect(),
    ))
}
