use std::collections::{HashMap, HashSet};

use anyhow::{Context, Result};
use domain::{Asset, Instrument, Money, StrategyId};
use market_data::{BinanceMarketData, MarketDataProvider};
use persistence::strategy_configs::StrategyConfigRecord;
use portfolio::PortfolioManager;
use sqlx::PgPool;
use strategies::StrategyRegistry;
use tracing::info;

use crate::config::{AppConfig, StrategyInstanceSpec, SymbolPair};
use crate::robot_runtime::RobotContext;
use crate::strategy_switch::ActiveStrategyTracker;

/// Símbolos e instâncias efetivas desta execução — podem vir do Postgres
/// (robôs em `running`) ou das variáveis de ambiente.
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    pub symbols: Vec<SymbolPair>,
    pub strategy_instances: Vec<StrategyInstanceSpec>,
    /// Quando true, só instâncias com `strategy_configs.enabled` entram no
    /// registry (controlado pelo dashboard via start/stop do robô).
    pub robots_from_db: bool,
    /// Metadados dos robôs `running` (vazio no modo legado só-env).
    pub running_robots: Vec<persistence::operational_robots::OperationalRobot>,
}

pub async fn resolve_runtime_config(pool: &PgPool, config: &AppConfig) -> Result<RuntimeConfig> {
    let running = persistence::operational_robots::list_running(pool).await?;
    if running.is_empty() {
        return Ok(RuntimeConfig {
            symbols: config.enabled_symbols.clone(),
            strategy_instances: if config.strategy_instances.is_empty() {
                Vec::new()
            } else {
                config.strategy_instances.clone()
            },
            robots_from_db: false,
            running_robots: Vec::new(),
        });
    }

    let mut symbols: HashMap<String, SymbolPair> = HashMap::new();
    let mut instances = Vec::new();

    for robot in &running {
        let (base, quote) = robot.symbol.split_once('/').with_context(|| {
            format!(
                "robot {} has invalid symbol {:?}, expected BASE/QUOTE",
                robot.id, robot.symbol
            )
        })?;
        symbols.insert(
            robot.symbol.clone(),
            SymbolPair {
                base: base.to_string(),
                quote: quote.to_string(),
            },
        );
        for kind in &robot.candidate_kinds {
            instances.push(StrategyInstanceSpec {
                id: persistence::operational_robots::strategy_instance_id(&robot.id, kind),
                kind: kind.clone(),
                symbols: vec![robot.symbol.clone()],
            });
        }
    }

    Ok(RuntimeConfig {
        symbols: symbols.into_values().collect(),
        strategy_instances: instances,
        robots_from_db: true,
        running_robots: running,
    })
}

/// Monta `RobotContext` a partir dos robôs running + instrumentos carregados.
pub fn build_robot_contexts(
    runtime: &RuntimeConfig,
    instruments: &[Instrument],
    config: &AppConfig,
) -> Result<Vec<RobotContext>> {
    let by_symbol: HashMap<String, &Instrument> = instruments
        .iter()
        .map(|i| (i.symbol.to_string(), i))
        .collect();

    if runtime.robots_from_db {
        let mut out = Vec::with_capacity(runtime.running_robots.len());
        for robot in &runtime.running_robots {
            let instrument = by_symbol.get(&robot.symbol).with_context(|| {
                format!(
                    "robot {} references unknown symbol {}",
                    robot.id, robot.symbol
                )
            })?;
            let timeframe = domain::Timeframe::parse(&robot.timeframe).with_context(|| {
                format!(
                    "robot {} has invalid timeframe {:?}; expected 1m/5m/15m/30m/1h/4h/1d/1w",
                    robot.id, robot.timeframe
                )
            })?;
            let strategy_ids = robot
                .candidate_kinds
                .iter()
                .map(|kind| {
                    StrategyId::new(persistence::operational_robots::strategy_instance_id(
                        &robot.id, kind,
                    ))
                    .with_context(|| format!("invalid instance id for robot {}", robot.id))
                })
                .collect::<Result<Vec<_>>>()?;
            out.push(RobotContext {
                id: robot.id.clone(),
                instrument_id: instrument.id,
                symbol: robot.symbol.clone(),
                timeframe,
                strategy_ids,
                paper_capital: robot.paper_capital,
            });
        }
        return Ok(out);
    }

    // Modo legado (sem operational_robots): um "robô" sintético por símbolo
    // com timeframe M1 e capital = PAPER_INITIAL_BALANCE.
    let instance_configs = if !runtime.strategy_instances.is_empty() {
        runtime.strategy_instances.clone()
    } else {
        config
            .enabled_strategies
            .iter()
            .map(|kind| StrategyInstanceSpec {
                id: kind.clone(),
                kind: kind.clone(),
                symbols: instruments.iter().map(|i| i.symbol.to_string()).collect(),
            })
            .collect()
    };

    let mut out = Vec::new();
    for instrument in instruments {
        let strategy_ids: Vec<StrategyId> = instance_configs
            .iter()
            .filter(|spec| {
                spec.symbols
                    .iter()
                    .any(|s| s == &instrument.symbol.to_string())
            })
            .filter_map(|spec| StrategyId::new(&spec.id).ok())
            .collect();
        if strategy_ids.is_empty() {
            continue;
        }
        out.push(RobotContext {
            id: format!("legacy-{}", instrument.symbol),
            instrument_id: instrument.id,
            symbol: instrument.symbol.to_string(),
            timeframe: domain::Timeframe::M1,
            strategy_ids,
            paper_capital: config.paper_initial_balance,
        });
    }
    Ok(out)
}

/// Restaura um `PortfolioManager` por robô a partir de
/// `robot_portfolio_snapshots` + posições cujo `strategy_id` pertence ao robô.
pub async fn restore_robot_portfolios(
    pool: &PgPool,
    robots: &[RobotContext],
) -> Result<HashMap<String, PortfolioManager>> {
    let all_open = persistence::positions::list_open(pool).await?;
    let all_closed = persistence::positions::list_closed(pool).await?;
    let mut map = HashMap::new();

    for robot in robots {
        let initial = Money::new(robot.paper_capital);
        let open: Vec<_> = all_open
            .iter()
            .filter(|p| robot.owns_strategy(&p.strategy_id))
            .filter(|p| !is_test_strategy(p.strategy_id.as_str()))
            .cloned()
            .collect();
        let closed: Vec<_> = all_closed
            .iter()
            .filter(|p| robot.owns_strategy(&p.strategy_id))
            .filter(|p| !is_test_strategy(p.strategy_id.as_str()))
            .cloned()
            .collect();

        let latest =
            persistence::robot_portfolio_snapshots::latest_for_robot(pool, &robot.id).await?;
        let current_cash = match latest {
            Some(snap) => {
                if snap.open_positions_count as usize != open.len() {
                    anyhow::bail!(
                        "robot {}: snapshot open_positions_count={} but positions table has {}",
                        robot.id,
                        snap.open_positions_count,
                        open.len()
                    );
                }
                Money::new(snap.cash)
            }
            None if open.is_empty() => initial,
            None => {
                anyhow::bail!(
                    "robot {}: {} open position(s) but no robot_portfolio_snapshots row",
                    robot.id,
                    open.len()
                );
            }
        };

        info!(
            robot_id = %robot.id,
            cash = %current_cash,
            open = open.len(),
            "restoring robot portfolio"
        );
        let pm = PortfolioManager::restore(initial, current_cash, open, closed)
            .with_context(|| format!("restoring portfolio for robot {}", robot.id))?;
        map.insert(robot.id.clone(), pm);
    }

    Ok(map)
}

/// Constrói o rastreador de estratégia ativa (Fase 3) com os limiares
/// default do Judge (`strategy_judge::JudgeThresholds::default`) — esta
/// fase não introduz configuração nova via env var, de propósito.
pub fn build_active_strategy_tracker() -> ActiveStrategyTracker {
    let judge = strategy_judge::StrategyJudge::new(strategy_judge::JudgeThresholds::default());
    ActiveStrategyTracker::new(judge)
}

/// Resolve cada symbol configurado contra o provider de market data
/// (buscando os metadados ao vivo na exchange em vez de deixar tick/lot
/// sizes hardcoded) e o persiste, para que `instruments` seja sempre um
/// espelho fiel do que o provider reporta no momento.
pub async fn load_instruments(
    symbols: &[SymbolPair],
    provider: &BinanceMarketData,
    pool: &PgPool,
) -> Result<Vec<Instrument>> {
    let mut instruments = Vec::with_capacity(symbols.len());

    for pair in symbols {
        let base = Asset::new(&pair.base)
            .with_context(|| format!("invalid base asset {:?}", pair.base))?;
        let quote = Asset::new(&pair.quote)
            .with_context(|| format!("invalid quote asset {:?}", pair.quote))?;

        let mut instrument = provider
            .fetch_instrument(&base, &quote)
            .await
            .with_context(|| format!("fetching instrument metadata for {base}/{quote}"))?;

        // O Postgres é a autoridade sobre o `InstrumentId`, não o id
        // aleatório que `Instrument::new` acabou de atribuir — reaproveita
        // o id já registrado para esta chave natural, para que ele
        // permaneça estável entre reinícios (ver docs de
        // `persistence::instruments::upsert`, ADR-9).
        instrument.id = persistence::instruments::upsert(pool, &instrument).await?;
        info!(symbol = %instrument.symbol, instrument_id = ?instrument.id, "instrument registered");
        instruments.push(instrument);
    }

    Ok(instruments)
}

/// Resolve as instâncias de estratégia desta execução a partir de
/// `AppConfig`, sem nenhuma combinação hardcoded aqui: se
/// `STRATEGY_INSTANCES` foi configurado (`config.strategy_instances`),
/// usa-o tal como está — cada entrada já é uma instância explícita
/// (id + kind + símbolos). Se não, cai no comportamento pré-Fase-1.5
/// (preservado para não quebrar nenhum deploy existente): uma instância por
/// kind de `ENABLED_STRATEGIES`, id igual ao kind, aplicada a todo
/// instrumento carregado compatível com a classe de ativo que o
/// `StrategyDescriptor` do catálogo declara.
fn resolve_instance_configs(
    runtime: &RuntimeConfig,
    config: &AppConfig,
    instruments: &[Instrument],
) -> Result<Vec<strategies::StrategyInstanceConfig>> {
    if runtime.robots_from_db {
        return runtime
            .strategy_instances
            .iter()
            .map(|spec| {
                Ok(strategies::StrategyInstanceConfig {
                    id: StrategyId::new(&spec.id)
                        .with_context(|| format!("invalid strategy instance id {:?}", spec.id))?,
                    kind: spec.kind.clone(),
                    symbols: spec.symbols.clone(),
                })
            })
            .collect();
    }

    if !config.strategy_instances.is_empty() {
        return config
            .strategy_instances
            .iter()
            .map(|spec| {
                Ok(strategies::StrategyInstanceConfig {
                    id: StrategyId::new(&spec.id)
                        .with_context(|| format!("invalid strategy instance id {:?}", spec.id))?,
                    kind: spec.kind.clone(),
                    symbols: spec.symbols.clone(),
                })
            })
            .collect();
    }

    config
        .enabled_strategies
        .iter()
        .map(|kind| {
            let descriptor = strategies::catalog::get(kind)
                .with_context(|| format!("unknown strategy kind {kind:?}"))?
                .descriptor;
            let symbols = instruments
                .iter()
                .filter(|i| {
                    descriptor
                        .requirements
                        .supported_asset_classes
                        .contains(&i.asset_class)
                })
                .map(|i| i.symbol.to_string())
                .collect();
            Ok(strategies::StrategyInstanceConfig {
                id: StrategyId::new(kind)?,
                kind: kind.clone(),
                symbols,
            })
        })
        .collect()
}

/// Monta o registry de estratégias: resolve as instâncias configuradas
/// (`resolve_instance_configs`), persiste a configuração de cada uma e
/// delega a construção/registro a `strategies::build_registry` — o mesmo
/// builder data-driven usado por qualquer outro consumidor futuro (ex.
/// backtest), sem duplicar aqui a lógica de "kind -> instância" que já
/// vive em `strategies::catalog`.
///
/// Adicionar um novo kind ao projeto significa adicionar uma entrada em
/// `strategies::catalog::entries()`, não mexer aqui — este loop já cobre
/// as 6 estratégias existentes (antes da Fase 1 usar o catálogo, cobria só
/// as 3 baselines; as 3 quantitativas nunca eram alcançáveis via
/// `ENABLED_STRATEGIES`, mesmo já implementadas e testadas — ver o
/// relatório da Fase 1).
pub async fn build_strategy_registry(
    runtime: &RuntimeConfig,
    config: &AppConfig,
    instruments: &[Instrument],
    provider_capabilities: &HashSet<domain::MarketDataKind>,
    pool: &PgPool,
) -> Result<StrategyRegistry> {
    let instance_configs = resolve_instance_configs(runtime, config, instruments)?;
    let mut enabled_instances = Vec::with_capacity(instance_configs.len());

    for instance in &instance_configs {
        let descriptor = strategies::catalog::get(&instance.kind)
            .with_context(|| format!("unknown strategy kind {:?}", instance.kind))?
            .descriptor;
        let enabled = if runtime.robots_from_db {
            persistence::strategy_configs::is_enabled(pool, &instance.id).await?
        } else {
            true
        };
        let record = StrategyConfigRecord {
            id: instance.id.clone(),
            strategy_kind: instance.kind.clone(),
            params: descriptor.default_params,
            supported_asset_classes: descriptor.requirements.supported_asset_classes.clone(),
            required_market_data: descriptor.requirements.required_market_data.clone(),
            enabled,
        };
        persistence::strategy_configs::upsert(pool, &record).await?;
        if enabled {
            enabled_instances.push(instance.clone());
        }
    }

    strategies::build_registry(&enabled_instances, instruments, provider_capabilities)
        .context("building strategy registry from instance configs")
}

/// `strategy_id`s de testes de integração (`crates/app/tests/`), que rodam
/// contra o mesmo Postgres de desenvolvimento usado pelo `quant-engine`
/// real — ver `web::handlers::is_test_strategy`, que filtra o mesmo padrão
/// só para leitura do dashboard. Aqui a filtragem é mais fundamental:
/// exclui essas posições ANTES de reconstruir o `PortfolioManager`, para
/// que o ledger da própria instância ao vivo (caixa, P&L realizado) nunca
/// as contabilize, e não apenas para que o dashboard deixe de mostrá-las.
///
/// Causa raiz confirmada: `restore_portfolio` (antes desta filtragem)
/// carregava `positions::list_closed` sem filtro, então o
/// `PortfolioManager::realized_pnl_total`/`realized_pnl_today` da própria
/// instância ao vivo somava também os `realized_pnl_net` de posições de
/// teste — em particular `crates/app/tests/portfolio_restart.rs`, cujo
/// cenário sintético (compra a 50000, venda a 51000) produz um P&L de
/// milhares de unidades monetárias, muito maior que qualquer trade real, o
/// que inflava "P&L total realizado"/"P&L diário" a um valor sem relação
/// com a soma real por estratégia mostrada em Desempenho. `equity`/`cash`
/// nunca foram afetados por esse bug: eles vêm do caixa rastreado
/// incrementalmente (não recalculado a partir de `closed_positions`), por
/// isso patrimônio/retorno já pareciam corretos mesmo com o P&L realizado
/// inflado.
fn is_test_strategy(strategy_id: &str) -> bool {
    strategy_id.starts_with("smoke-test-") || strategy_id.starts_with("restart-test-")
}

/// Reconstrói o `PortfolioManager` a partir do estado já persistido no
/// Postgres — posições abertas, posições fechadas e o caixa do último
/// `portfolio_snapshots` — para que um restart do `quant-engine` nunca
/// comece com um portfólio vazio enquanto o banco já reflete posições
/// abertas. Deve ser chamada antes de abrir o stream de market data e
/// antes de processar qualquer evento, para que o Risk Engine já veja o
/// estado real desde o primeiro candle processado após o restart.
///
/// Não recria, duplica nem altera nenhuma posição existente: as posições
/// carregadas de `persistence::positions` são passadas como estão para
/// `PortfolioManager::restore`, que só as organiza em memória — a única
/// transformação aplicada aqui é excluir posições de `strategy_id` de
/// teste (`is_test_strategy`) antes de repassá-las.
///
/// Trata duas inconsistências explicitamente, em vez de seguir em frente
/// com um estado que pode estar errado:
/// - existem posições abertas persistidas mas nenhum `portfolio_snapshots`
///   foi encontrado — não há de onde derivar o caixa atual com segurança;
/// - os dados carregados violam uma invariante que `PortfolioManager`
///   exige (ver `PortfolioManager::restore`), por exemplo mais de uma
///   posição aberta para o mesmo instrumento.
///
/// Na primeira execução (banco sem nenhum snapshot e sem posição aberta),
/// isso não é uma inconsistência: o portfólio simplesmente começa do
/// `initial_cash` configurado, exatamente como antes desta mudança.
pub async fn restore_portfolio(pool: &PgPool, initial_cash: Money) -> Result<PortfolioManager> {
    let open_positions: Vec<_> = persistence::positions::list_open(pool)
        .await
        .context("loading open positions from Postgres")?
        .into_iter()
        .filter(|p| !is_test_strategy(p.strategy_id.as_str()))
        .collect();
    let closed_positions: Vec<_> = persistence::positions::list_closed(pool)
        .await
        .context("loading closed positions from Postgres")?
        .into_iter()
        .filter(|p| !is_test_strategy(p.strategy_id.as_str()))
        .collect();
    let latest_snapshot = persistence::portfolio_snapshots::latest(pool)
        .await
        .context("loading latest portfolio snapshot from Postgres")?;

    let has_snapshot = latest_snapshot.is_some();
    let current_cash = match latest_snapshot {
        Some(snapshot) => {
            // O snapshot é a fonte do caixa; a tabela `positions` é a fonte
            // das posições abertas. Se os dois discordarem sobre *quantas*
            // posições estão abertas, o estado persistido não é confiável
            // o bastante para retomar (ex.: um teste de integração fechou
            // a posição mas deixou um snapshot futuro com contagem antiga).
            if snapshot.open_positions_count as usize != open_positions.len() {
                anyhow::bail!(
                    "inconsistent state while restoring portfolio: latest portfolio_snapshots \
                     row (timestamp={}) reports open_positions_count={} but positions table \
                     has {} open row(s); refusing to resume from a mismatched snapshot",
                    snapshot.timestamp,
                    snapshot.open_positions_count,
                    open_positions.len()
                );
            }
            Money::new(snapshot.cash)
        }
        None if open_positions.is_empty() => {
            // Primeira execução: nada foi persistido ainda, não é uma
            // inconsistência.
            initial_cash
        }
        None => {
            anyhow::bail!(
                "found {} open position(s) in Postgres but no portfolio_snapshots row exists; \
                 refusing to guess the current cash balance to resume from",
                open_positions.len()
            );
        }
    };

    if !open_positions.is_empty() || has_snapshot {
        info!(
            open_positions = open_positions.len(),
            closed_positions = closed_positions.len(),
            cash = %current_cash,
            "restoring portfolio state from Postgres before starting market data"
        );
    }

    PortfolioManager::restore(initial_cash, current_cash, open_positions, closed_positions)
        .context("restoring portfolio state from persisted data")
}
