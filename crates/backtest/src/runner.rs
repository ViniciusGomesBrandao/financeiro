use std::collections::HashMap;

use chrono::{DateTime, Utc};
use domain::{Candle, Instrument, InstrumentId, MarketEvent, Price};
use execution::Broker;
use features::{FeatureConfig, FeatureEngine, FeatureSnapshot};
use portfolio::PortfolioManager;
use risk::{RiskDecision, RiskEngine};
use strategies::StrategyRegistry;
use tracing::info;

use crate::error::BacktestError;
use crate::report::BacktestReport;

/// Reproduz candles históricos exatamente pelo mesmo pipeline que o loop
/// live da aplicação conduz — `StrategyRegistry -> RiskEngine -> Broker ->
/// PortfolioManager` — de modo que o comportamento de uma estratégia em
/// backtest e em live/paper trading nunca possa divergir silenciosamente
/// porque o backtest reimplementou a integração.
///
/// O que *não* é tentado aqui: intercalar eventos de múltiplos instrumentos
/// além da simples ordenação cronológica, fills parciais, efeitos de
/// latência/fila ou controles point-in-time de viés de sobrevivência. Esta
/// é uma fundação para um backtesting mais rigoroso no futuro — não uma
/// afirmação de que seus resultados sejam representativos da performance
/// live.
pub struct BacktestRunner {
    instruments: HashMap<InstrumentId, Instrument>,
    feature_config: FeatureConfig,
}

impl BacktestRunner {
    pub fn new(instruments: Vec<Instrument>) -> Self {
        Self::with_feature_config(instruments, FeatureConfig::default())
    }

    /// Como `new`, mas com parâmetros de feature customizados (janelas,
    /// lambda da EWMA, ...) em vez dos defaults de
    /// `features::FeatureConfig`. Ver `crates/features` para o que cada
    /// parâmetro controla.
    pub fn with_feature_config(
        instruments: Vec<Instrument>,
        feature_config: FeatureConfig,
    ) -> Self {
        Self {
            instruments: instruments.into_iter().map(|i| (i.id, i)).collect(),
            feature_config,
        }
    }

    /// Reproduz `candles` (em qualquer ordem, com qualquer combinação de
    /// instrumentos — ordenados internamente por `close_time`) através de
    /// `registry`/`risk_engine`/`broker`, mutando `portfolio` exatamente
    /// como o trading live faria.
    pub async fn run(
        &self,
        mut candles: Vec<Candle>,
        registry: &mut StrategyRegistry,
        risk_engine: &RiskEngine,
        broker: &mut dyn Broker,
        portfolio: &mut PortfolioManager,
    ) -> Result<BacktestReport, BacktestError> {
        candles.sort_by_key(|c| c.close_time);

        let mut mark_prices: HashMap<InstrumentId, rust_decimal::Decimal> = HashMap::new();
        let mut last_timestamp: DateTime<Utc> = Utc::now();
        let mut processed = 0usize;
        // Um `FeatureEngine` por instrumento, criado sob demanda — mesma
        // convenção de estado por-instrumento que `strategies::generic` já
        // usa para seus próprios indicadores internos.
        let mut feature_engines: HashMap<InstrumentId, FeatureEngine> = HashMap::new();
        let mut feature_snapshots: HashMap<InstrumentId, Vec<FeatureSnapshot>> = HashMap::new();
        // Um `PortfolioSnapshot` por candle processado, na mesma cadência
        // que `app::pipeline::run` já grava no Postgres ao vivo — aqui só
        // acumulado em memória. Puramente observacional: não influencia
        // nenhuma decisão de estratégia/risco/execução, só dá ao chamador
        // (ex. `experiments`) a série temporal de equity necessária para
        // retorno diário/mensal/anual e drawdown de curva de equity.
        let mut equity_curve: Vec<domain::PortfolioSnapshot> = Vec::new();

        // Ordens produzidas a partir do candle T (entradas aprovadas pelo
        // Risk Engine e saídas disparadas por stop/take-profit) nunca são
        // executáveis no close(T) que as originou — esse preço já é
        // conhecido/passado no instante em que a decisão existiria de
        // verdade. Ficam pendentes aqui até o próximo candle *do mesmo
        // instrumento* aparecer na reprodução (candles de instrumentos
        // diferentes se intercalam cronologicamente), quando então são
        // preenchidas usando o open() daquele candle como preço de
        // referência. Se o instrumento nunca mais aparecer, a ordem
        // permanece pendente para sempre e é descartada ao final — nunca
        // executada.
        let mut pending_orders: HashMap<InstrumentId, Vec<domain::OrderRequest>> = HashMap::new();

        for candle in candles {
            let Some(instrument) = self.instruments.get(&candle.instrument_id) else {
                continue;
            };
            if !candle.is_closed {
                continue;
            }

            // Drena antes de qualquer outra coisa: para quem gerou essas
            // ordens, este é o "próximo candle disponível" prometido, e seu
            // open() é o primeiro preço que já seria realisticamente
            // executável.
            if let Some(orders) = pending_orders.remove(&candle.instrument_id) {
                let reference_price = Price::new(candle.open)?;
                for order_request in orders {
                    broker
                        .submit_order(
                            order_request,
                            reference_price,
                            candle.open_time,
                            instrument,
                            portfolio,
                        )
                        .await?;
                }
            }

            mark_prices.insert(candle.instrument_id, candle.close);
            last_timestamp = candle.close_time;
            processed += 1;

            // Alimentado antes de qualquer coisa que dependa do candle,
            // para casar com "informação disponível até o instante
            // analisado": o snapshot resultante nunca pode refletir um
            // candle que ainda não foi processado nesta mesma iteração.
            let engine = feature_engines
                .entry(candle.instrument_id)
                .or_insert_with(|| {
                    FeatureEngine::new(candle.instrument_id, self.feature_config.clone())
                });
            if let Some(snapshot) = engine.update(&candle) {
                feature_snapshots
                    .entry(candle.instrument_id)
                    .or_default()
                    .push(snapshot);
            }

            self.queue_risk_driven_exit(&candle, risk_engine, portfolio, &mut pending_orders);

            let event = MarketEvent::Candle(candle.clone());
            // `portfolio` (não estado interno da estratégia) é a fonte de
            // verdade sobre posição real; `StrategyRegistry::dispatch`
            // escopa a visão de cada estratégia ao seu próprio robô antes
            // de repassar — ver `strategies::{PositionQuery, RobotPositions}`,
            // ADR-20.
            let signals = registry.dispatch(instrument, &event, &*portfolio);

            for signal in signals {
                // O dimensionamento/aprovação continuam avaliados no
                // close(T) — "tamanho decidido no instante da decisão" — só
                // o preenchimento em si é adiado para open(T+1); ver o doc
                // de `pending_orders` acima.
                let price = Price::new(candle.close)?;
                let risk_snapshot = portfolio.risk_snapshot(candle.close_time);
                let decision = risk_engine.evaluate(&signal, instrument, price, &risk_snapshot);

                if let RiskDecision::Approved(order_request) = decision {
                    // Se `queue_risk_driven_exit` já enfileirou uma ordem
                    // para *este robô* neste instrumento nesta mesma
                    // iteração, este sinal foi avaliado contra uma posição
                    // que já está prestes a ser fechada — enfileirá-lo
                    // também duplicaria a saída (as duas ordens são
                    // drenadas em sequência no próximo candle; a segunda
                    // encontraria a posição já fechada pela primeira, um
                    // naked short espúrio). Escopado por `strategy_id`, não
                    // só pelo instrumento (ADR-20): a saída pendente de um
                    // robô não pode descartar a entrada de outro robô no
                    // mesmo instrumento neste candle. Ao vivo isso não pode
                    // acontecer porque a saída por risco executa *antes* do
                    // dispatch da estratégia, na mesma passagem — o
                    // dispatch já vê o portfólio atualizado. Em backtest,
                    // com a execução adiada, as duas decisões são tomadas
                    // sobre o mesmo instantâneo (ainda não atualizado) do
                    // portfólio; descartar aqui recria a mesma precedência
                    // do live (saída por risco sempre primeiro) sem exigir
                    // que o `RiskEngine` saiba de ordens ainda não
                    // executadas.
                    let risk_exit_already_pending_for_this_robot = pending_orders
                        .get(&candle.instrument_id)
                        .is_some_and(|orders| {
                            orders.iter().any(|o| o.strategy_id == signal.strategy_id)
                        });
                    if risk_exit_already_pending_for_this_robot {
                        tracing::debug!(
                            instrument_id = ?candle.instrument_id,
                            strategy_id = %signal.strategy_id,
                            "descartando sinal: saída por risco já enfileirada para este \
                             robô neste instrumento neste candle"
                        );
                        continue;
                    }
                    pending_orders
                        .entry(candle.instrument_id)
                        .or_default()
                        .push(order_request);
                }
            }

            equity_curve.push(portfolio.snapshot(&mark_prices, candle.close_time));
        }

        if !pending_orders.is_empty() {
            let dropped: usize = pending_orders.values().map(Vec::len).sum();
            info!(
                dropped,
                "reprodução terminou com ordens pendentes sem um próximo candle para \
                 preenchê-las; descartadas, nunca executadas"
            );
        }

        info!(candles_processed = processed, "backtest replay complete");

        Ok(BacktestReport {
            final_snapshot: portfolio.snapshot(&mark_prices, last_timestamp),
            closed_positions: portfolio.closed_positions().to_vec(),
            candles_processed: processed,
            feature_snapshots,
            equity_curve,
        })
    }

    /// Constrói (se aplicável) a ordem de saída disparada por stop-loss/take
    /// -profit para o candle atual e a enfileira em `pending_orders` — nunca
    /// a executa diretamente, pelo mesmo motivo pelo qual sinais de entrada
    /// também são adiados (ver o doc de `pending_orders` em `run`).
    ///
    /// Checa **cada** posição aberta neste instrumento, não só uma — com
    /// múltiplos robôs (ADR-20) pode haver mais de uma simultânea, cada
    /// uma com seu próprio `entry_price` e portanto seu próprio limiar de
    /// stop/take-profit; uma ordem de saída é enfileirada por posição
    /// rompida, nunca uma só para "o" instrumento.
    fn queue_risk_driven_exit(
        &self,
        candle: &Candle,
        risk_engine: &RiskEngine,
        portfolio: &PortfolioManager,
        pending_orders: &mut HashMap<InstrumentId, Vec<domain::OrderRequest>>,
    ) {
        for position in portfolio.open_positions_for_instrument(candle.instrument_id) {
            if risk_engine.check_exit(position, candle.close).is_none() {
                continue;
            }

            let exit_order = risk_engine.build_exit_order(position, candle.close_time);
            pending_orders
                .entry(candle.instrument_id)
                .or_default()
                .push(exit_order);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::TimeZone;
    use domain::{
        Asset, AssetClass, Exchange, MarketDataKind, MarketType, Money, Symbol, Timeframe,
    };
    use execution::{PaperBroker, PaperBrokerConfig};
    use risk::RiskConfig;
    use rust_decimal_macros::dec;
    use std::collections::HashSet;
    use strategies::generic::{EmaCrossoverParams, EmaCrossoverStrategy};

    fn instrument() -> Instrument {
        let base = Asset::new("BTC").unwrap();
        let quote = Asset::new("USDT").unwrap();
        Instrument::new(
            Symbol::from_pair(&base, &quote),
            base,
            quote,
            AssetClass::Crypto,
            Exchange::Binance,
            MarketType::Spot,
            dec!(0.01),
            dec!(0.0001),
            dec!(0.0001),
            dec!(10),
        )
    }

    fn candle(instrument_id: InstrumentId, close: rust_decimal::Decimal, minute: i64) -> Candle {
        let open_time =
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap() + chrono::Duration::minutes(minute);
        Candle {
            instrument_id,
            timeframe: Timeframe::M1,
            open_time,
            close_time: open_time + chrono::Duration::minutes(1),
            open: close,
            high: close,
            low: close,
            close,
            volume: dec!(1),
            is_closed: true,
        }
    }

    #[tokio::test]
    async fn replays_candles_and_opens_a_position_on_crossover() {
        let instrument = instrument();
        let runner = BacktestRunner::new(vec![instrument.clone()]);

        let mut registry = StrategyRegistry::new();
        let strategy = EmaCrossoverStrategy::new(
            domain::StrategyId::new("ema-backtest").unwrap(),
            EmaCrossoverParams {
                fast_period: 2,
                slow_period: 4,
            },
        );
        let available = HashSet::from([MarketDataKind::Ohlcv]);
        registry
            .register(
                Box::new(strategy),
                std::slice::from_ref(&instrument),
                &available,
            )
            .unwrap();

        let risk_engine = RiskEngine::new(RiskConfig {
            order_notional: Money::new(dec!(1000)),
            max_position_notional: Money::new(dec!(1000)),
            max_total_exposure: Money::new(dec!(5000)),
            max_open_positions: 5,
            stop_loss_pct: None,
            take_profit_pct: None,
            max_daily_loss: Money::new(dec!(1000)),
        });
        let mut broker = PaperBroker::new(PaperBrokerConfig {
            maker_fee: dec!(0.0005),
            taker_fee: dec!(0.001),
            spread_bps: dec!(2),
            slippage_bps: dec!(3),
        });
        let mut portfolio = PortfolioManager::new(Money::new(dec!(100000)));

        // Mesmo formato de queda seguida de alta usado nos testes de
        // strategies::generic::ema_crossover, que produz de forma
        // confiável um crossover de alta.
        let prices = [
            dec!(100),
            dec!(90),
            dec!(80),
            dec!(70),
            dec!(90),
            dec!(120),
            dec!(150),
        ];
        let candles: Vec<Candle> = prices
            .iter()
            .enumerate()
            .map(|(i, p)| candle(instrument.id, *p, i as i64))
            .collect();

        let report = runner
            .run(
                candles,
                &mut registry,
                &risk_engine,
                &mut broker,
                &mut portfolio,
            )
            .await
            .unwrap();

        assert_eq!(report.candles_processed, 7);
        assert!(
            !portfolio.open_positions().is_empty(),
            "expected the bullish crossover to open a position"
        );

        // Um snapshot de equity por candle processado, em ordem
        // cronológica — a matéria-prima que `analytics::compute_equity_curve_report`
        // consome.
        assert_eq!(report.equity_curve.len(), 7);
        assert!(report
            .equity_curve
            .windows(2)
            .all(|w| w[0].timestamp < w[1].timestamp));
    }

    /// Prova a integração ponta a ponta do `FeatureEngine` com
    /// `BacktestRunner`: `feature_snapshots` vem preenchido por
    /// instrumento, e o snapshot de um índice já processado nunca muda
    /// quando o mesmo prefixo de candles é reproduzido com mais candles
    /// depois dele — a mesma garantia de não-look-ahead de
    /// `features::compute_series`, agora verificada através do runner
    /// real (registry/risk/broker/portfolio incluídos), não só do motor de
    /// features isolado.
    #[tokio::test]
    async fn feature_snapshots_are_populated_and_free_of_look_ahead_bias() {
        let instrument = instrument();
        let feature_config = features::FeatureConfig {
            sma_period: 3,
            ema_period: 3,
            return_periods: vec![1],
            ..features::FeatureConfig::default()
        };

        let prices = [
            dec!(100),
            dec!(101),
            dec!(99),
            dec!(105),
            dec!(95),
            dec!(110),
            dec!(90),
            dec!(120),
        ];
        let candles: Vec<Candle> = prices
            .iter()
            .enumerate()
            .map(|(i, p)| candle(instrument.id, *p, i as i64))
            .collect();

        async fn run_prefix(
            instrument: &Instrument,
            feature_config: features::FeatureConfig,
            candles: Vec<Candle>,
        ) -> BacktestReport {
            let runner =
                BacktestRunner::with_feature_config(vec![instrument.clone()], feature_config);
            let mut registry = StrategyRegistry::new();
            let risk_engine = RiskEngine::new(RiskConfig {
                order_notional: Money::new(dec!(1000)),
                max_position_notional: Money::new(dec!(1000)),
                max_total_exposure: Money::new(dec!(5000)),
                max_open_positions: 5,
                stop_loss_pct: None,
                take_profit_pct: None,
                max_daily_loss: Money::new(dec!(1000)),
            });
            let mut broker = PaperBroker::new(PaperBrokerConfig {
                maker_fee: dec!(0.0005),
                taker_fee: dec!(0.001),
                spread_bps: dec!(2),
                slippage_bps: dec!(3),
            });
            let mut portfolio = PortfolioManager::new(Money::new(dec!(100000)));
            runner
                .run(
                    candles,
                    &mut registry,
                    &risk_engine,
                    &mut broker,
                    &mut portfolio,
                )
                .await
                .unwrap()
        }

        let short_report =
            run_prefix(&instrument, feature_config.clone(), candles[..5].to_vec()).await;
        let full_report = run_prefix(&instrument, feature_config, candles).await;

        let short_snapshots = short_report
            .feature_snapshots
            .get(&instrument.id)
            .expect("expected feature snapshots for the instrument");
        let full_snapshots = full_report
            .feature_snapshots
            .get(&instrument.id)
            .expect("expected feature snapshots for the instrument");

        assert_eq!(short_snapshots.len(), 5);
        assert_eq!(full_snapshots.len(), 8);
        assert_eq!(
            short_snapshots[4], full_snapshots[4],
            "a snapshot already produced must never be altered by candles that arrive after it"
        );
        assert!(
            short_snapshots[4].sma.is_some(),
            "sma_period=3 over 5 candles must already have a value"
        );
    }

    /// Auditoria semântica, requisito 1 (timing): um sinal calculado a
    /// partir do fechamento do candle T não pode executar nem retroativamente
    /// *em* T, nem antes do próximo candle disponível existir. Prova isso de
    /// três formas complementares através do `BacktestRunner` real (não só
    /// do motor de features isolado):
    ///
    /// 1. Reproduzindo só `candles[..k]` (sem o candle que dispara a
    ///    entrada), nenhuma posição é aberta — o sinal genuinamente não
    ///    era "conhecível" antes de T.
    /// 2. Reproduzindo `candles[..=k]` (com o candle gatilho T, mas sem
    ///    nenhum candle depois dele), a ordem fica pendente e *nenhuma*
    ///    posição é aberta ainda — T ainda não tinha um "próximo candle
    ///    disponível" para executar contra.
    /// 3. Reproduzindo `candles[..=k+1]` (T e seu próximo candle), a
    ///    posição resultante existe e seu `opened_at` é exatamente
    ///    `candles[k+1].open_time` — a execução é adiada para o open() do
    ///    candle seguinte, nunca para o close() do candle que gerou o
    ///    sinal.
    #[tokio::test]
    async fn signal_execution_never_precedes_the_candle_that_produced_it() {
        use strategies::generic::{QuantMomentumParams, QuantMomentumStrategy};
        use strategies::FeatureStrategyAdapter;

        let instrument = instrument();
        let params = QuantMomentumParams {
            regression_period: 5,
            min_r_squared: 0.9,
            relative_volume_period: 5,
            min_relative_volume: 0.0,
            ..QuantMomentumParams::default()
        };
        // Reta perfeita: a janela de regressão enche e a entrada dispara
        // exatamente no 5º candle (índice 4), nunca antes.
        let prices: Vec<rust_decimal::Decimal> = (0..6)
            .map(|i| dec!(100) + rust_decimal::Decimal::from(i * 2))
            .collect();
        let candles: Vec<Candle> = prices
            .iter()
            .enumerate()
            .map(|(i, p)| candle(instrument.id, *p, i as i64))
            .collect();

        async fn run_prefix(
            instrument: &Instrument,
            params: QuantMomentumParams,
            candles: Vec<Candle>,
        ) -> PortfolioManager {
            let runner = BacktestRunner::new(vec![instrument.clone()]);
            let mut registry = StrategyRegistry::new();
            let strategy = QuantMomentumStrategy::new(
                domain::StrategyId::new("qm-timing-test").unwrap(),
                params,
            );
            let available = HashSet::from([MarketDataKind::Ohlcv]);
            registry
                .register(
                    Box::new(FeatureStrategyAdapter::new(strategy)),
                    std::slice::from_ref(instrument),
                    &available,
                )
                .unwrap();
            let risk_engine = RiskEngine::new(RiskConfig {
                order_notional: Money::new(dec!(1000)),
                max_position_notional: Money::new(dec!(1000)),
                max_total_exposure: Money::new(dec!(5000)),
                max_open_positions: 5,
                stop_loss_pct: None,
                take_profit_pct: None,
                max_daily_loss: Money::new(dec!(1000)),
            });
            let mut broker = PaperBroker::new(PaperBrokerConfig {
                maker_fee: dec!(0.0005),
                taker_fee: dec!(0.001),
                spread_bps: dec!(2),
                slippage_bps: dec!(3),
            });
            let mut portfolio = PortfolioManager::new(Money::new(dec!(100000)));
            runner
                .run(
                    candles,
                    &mut registry,
                    &risk_engine,
                    &mut broker,
                    &mut portfolio,
                )
                .await
                .unwrap();
            portfolio
        }

        // (1) Sem o candle que dispara a entrada (índice 4): nenhuma
        // posição pode existir.
        let before_trigger = run_prefix(&instrument, params, candles[..4].to_vec()).await;
        assert!(
            before_trigger.open_positions().is_empty(),
            "no position may exist before the candle whose data triggers the signal"
        );

        // (2) Com o candle gatilho incluído, mas sem nenhum candle depois
        // dele: a ordem existe (o sinal foi aprovado), mas ainda está
        // pendente — sem um "próximo candle disponível", ela não pode ter
        // sido preenchida.
        let trigger_no_next = run_prefix(&instrument, params, candles[..5].to_vec()).await;
        assert!(
            trigger_no_next.open_positions().is_empty(),
            "an order approved on the trigger candle must stay pending until the next candle \
             for that instrument is available — it must not fill on the same candle that \
             produced the signal"
        );

        // (3) Com o candle gatilho E o próximo candle: a posição existe, e
        // seu opened_at é exatamente o open_time do candle *seguinte* ao
        // que gerou o sinal — nunca o close_time do candle gatilho.
        let trigger_with_next = run_prefix(&instrument, params, candles[..6].to_vec()).await;
        let position = trigger_with_next.open_positions().first().expect(
            "expected a position to be open once the next candle after the trigger is processed",
        );
        assert_eq!(
            position.opened_at, candles[5].open_time,
            "execution must be timestamped at the open of the candle *after* the one that \
             produced the signal, never at that candle's own close"
        );
    }

    /// Estratégia de teste que emite um único sinal Long na primeira vez em
    /// que é chamada, e nunca mais — usada para isolar exatamente qual
    /// preço/timestamp o runner usa para preencher a ordem resultante, sem
    /// depender da lógica de nenhuma estratégia quantitativa real.
    struct FireOnceStrategy {
        id: domain::StrategyId,
        requirements: strategies::StrategyRequirements,
        fired: bool,
    }

    impl FireOnceStrategy {
        fn new() -> Self {
            Self {
                id: domain::StrategyId::new("fire-once-test").unwrap(),
                requirements: strategies::StrategyRequirements::new(
                    vec![MarketDataKind::Ohlcv],
                    vec![AssetClass::Crypto],
                ),
                fired: false,
            }
        }
    }

    impl strategies::Strategy for FireOnceStrategy {
        fn id(&self) -> &domain::StrategyId {
            &self.id
        }

        fn requirements(&self) -> &strategies::StrategyRequirements {
            &self.requirements
        }

        fn on_event(
            &mut self,
            instrument: &Instrument,
            event: &MarketEvent,
            _positions: &dyn strategies::PositionQuery,
        ) -> Option<domain::Signal> {
            if self.fired {
                return None;
            }
            let MarketEvent::Candle(candle) = event else {
                return None;
            };
            self.fired = true;
            Some(
                domain::Signal::new(
                    self.id.clone(),
                    instrument.id,
                    domain::SignalDirection::Long,
                    1.0,
                    candle.close_time,
                    None,
                    None,
                    serde_json::Value::Null,
                )
                .unwrap(),
            )
        }
    }

    fn timing_test_risk_engine(take_profit_pct: Option<rust_decimal::Decimal>) -> RiskEngine {
        RiskEngine::new(RiskConfig {
            order_notional: Money::new(dec!(1000)),
            max_position_notional: Money::new(dec!(10000)),
            max_total_exposure: Money::new(dec!(50000)),
            max_open_positions: 5,
            stop_loss_pct: None,
            take_profit_pct,
            max_daily_loss: Money::new(dec!(10000)),
        })
    }

    fn timing_test_broker() -> PaperBroker {
        PaperBroker::new(PaperBrokerConfig {
            maker_fee: dec!(0.0005),
            taker_fee: dec!(0.001),
            spread_bps: dec!(2),
            slippage_bps: dec!(3),
        })
    }

    /// Prova, com preços de controle (não os de uma estratégia real), que o
    /// preço de referência do fill é o open() do próximo candle — nunca o
    /// close() (nem o open()) do candle que gerou o sinal.
    #[tokio::test]
    async fn entry_order_fills_using_open_of_next_candle_as_reference_price() {
        let instrument = instrument();
        let runner = BacktestRunner::new(vec![instrument.clone()]);

        let mut registry = StrategyRegistry::new();
        let available = HashSet::from([MarketDataKind::Ohlcv]);
        registry
            .register(
                Box::new(FireOnceStrategy::new()),
                std::slice::from_ref(&instrument),
                &available,
            )
            .unwrap();

        let risk_engine = timing_test_risk_engine(None);
        let mut broker = timing_test_broker();
        let mut portfolio = PortfolioManager::new(Money::new(dec!(100000)));

        let base = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        // Candle T: gera o sinal. open == close == 100, para que qualquer
        // contaminação com o preço de T (em vez de T+1) seja óbvia.
        let signal_candle = Candle {
            instrument_id: instrument.id,
            timeframe: Timeframe::M1,
            open_time: base,
            close_time: base + chrono::Duration::minutes(1),
            open: dec!(100),
            high: dec!(100),
            low: dec!(100),
            close: dec!(100),
            volume: dec!(1),
            is_closed: true,
        };
        // Candle T+1: único candle cujo open() poderia ter sido usado como
        // referência do fill.
        let next_candle = Candle {
            instrument_id: instrument.id,
            timeframe: Timeframe::M1,
            open_time: base + chrono::Duration::minutes(1),
            close_time: base + chrono::Duration::minutes(2),
            open: dec!(200),
            high: dec!(200),
            low: dec!(200),
            close: dec!(205),
            volume: dec!(1),
            is_closed: true,
        };

        let report = runner
            .run(
                vec![signal_candle.clone(), next_candle.clone()],
                &mut registry,
                &risk_engine,
                &mut broker,
                &mut portfolio,
            )
            .await
            .unwrap();

        assert_eq!(report.candles_processed, 2);
        let position = portfolio
            .open_positions()
            .first()
            .expect("expected the deferred entry order to fill once the next candle arrives");

        assert_eq!(
            position.opened_at, next_candle.open_time,
            "a posição deve ser timestampada com o instante da execução (open do próximo \
             candle), não com o instante do sinal"
        );
        // spread(2bps) + slippage(3bps) somam poucos centavos sobre 200 —
        // o preço de entrada tem que estar próximo de 200 (open de T+1),
        // nunca de 100 (preço de T).
        assert!(
            position.entry_price > dec!(150),
            "entry_price ({}) deve refletir o open() do próximo candle (~200), não o preço do \
             candle que gerou o sinal (100)",
            position.entry_price
        );
    }

    /// Se o candle gatilho for o último da reprodução, a ordem aprovada
    /// nunca deve executar — não existe um "próximo candle disponível"
    /// para servir de referência de preço.
    #[tokio::test]
    async fn entry_order_never_executes_when_no_next_candle_exists() {
        let instrument = instrument();
        let runner = BacktestRunner::new(vec![instrument.clone()]);

        let mut registry = StrategyRegistry::new();
        let available = HashSet::from([MarketDataKind::Ohlcv]);
        registry
            .register(
                Box::new(FireOnceStrategy::new()),
                std::slice::from_ref(&instrument),
                &available,
            )
            .unwrap();

        let risk_engine = timing_test_risk_engine(None);
        let mut broker = timing_test_broker();
        let mut portfolio = PortfolioManager::new(Money::new(dec!(100000)));

        let base = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let signal_candle = Candle {
            instrument_id: instrument.id,
            timeframe: Timeframe::M1,
            open_time: base,
            close_time: base + chrono::Duration::minutes(1),
            open: dec!(100),
            high: dec!(100),
            low: dec!(100),
            close: dec!(100),
            volume: dec!(1),
            is_closed: true,
        };

        let report = runner
            .run(
                vec![signal_candle],
                &mut registry,
                &risk_engine,
                &mut broker,
                &mut portfolio,
            )
            .await
            .unwrap();

        assert_eq!(report.candles_processed, 1);
        assert!(
            portfolio.open_positions().is_empty(),
            "an order with no next candle to fill against must never execute"
        );
        assert!(
            portfolio.closed_positions().is_empty(),
            "an order with no next candle to fill against must never execute"
        );
    }

    /// A mesma deferência para o próximo candle disponível se aplica a
    /// saídas disparadas por risco (stop loss / take profit): o candle que
    /// rompe o limiar só *decide* a saída; o preenchimento acontece no
    /// open() do candle seguinte, exatamente como uma saída de estratégia.
    #[tokio::test]
    async fn risk_driven_exit_also_fills_using_open_of_next_candle() {
        let instrument = instrument();
        let runner = BacktestRunner::new(vec![instrument.clone()]);

        let mut registry = StrategyRegistry::new();
        let available = HashSet::from([MarketDataKind::Ohlcv]);
        registry
            .register(
                Box::new(FireOnceStrategy::new()),
                std::slice::from_ref(&instrument),
                &available,
            )
            .unwrap();

        // 50% de take profit: dispara com folga assim que o preço dobra,
        // sem depender do valor exato ajustado por spread/slippage do
        // entry_price real.
        let risk_engine = timing_test_risk_engine(Some(dec!(0.5)));
        let mut broker = timing_test_broker();
        let mut portfolio = PortfolioManager::new(Money::new(dec!(100000)));

        let base = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        // T: gera o sinal de entrada.
        let signal_candle = Candle {
            instrument_id: instrument.id,
            timeframe: Timeframe::M1,
            open_time: base,
            close_time: base + chrono::Duration::minutes(1),
            open: dec!(100),
            high: dec!(100),
            low: dec!(100),
            close: dec!(100),
            volume: dec!(1),
            is_closed: true,
        };
        // T+1: preenche a entrada (open=200); close (205) fica perto do
        // entry_price, então o take profit ainda não pode disparar aqui.
        let fill_candle = Candle {
            instrument_id: instrument.id,
            timeframe: Timeframe::M1,
            open_time: base + chrono::Duration::minutes(1),
            close_time: base + chrono::Duration::minutes(2),
            open: dec!(200),
            high: dec!(205),
            low: dec!(200),
            close: dec!(205),
            volume: dec!(1),
            is_closed: true,
        };
        // T+2: close (400) rompe o take profit de 50% sobre um entry_price
        // ~200 — a saída é decidida aqui, mas só pode encher no próximo
        // candle.
        let exit_trigger_candle = Candle {
            instrument_id: instrument.id,
            timeframe: Timeframe::M1,
            open_time: base + chrono::Duration::minutes(2),
            close_time: base + chrono::Duration::minutes(3),
            open: dec!(210),
            high: dec!(400),
            low: dec!(210),
            close: dec!(400),
            volume: dec!(1),
            is_closed: true,
        };
        // T+3: único candle cujo open() poderia ter sido usado como
        // referência do fill de saída.
        let exit_fill_candle = Candle {
            instrument_id: instrument.id,
            timeframe: Timeframe::M1,
            open_time: base + chrono::Duration::minutes(3),
            close_time: base + chrono::Duration::minutes(4),
            open: dec!(1000),
            high: dec!(1000),
            low: dec!(1000),
            close: dec!(1000),
            volume: dec!(1),
            is_closed: true,
        };

        let report = runner
            .run(
                vec![
                    signal_candle,
                    fill_candle,
                    exit_trigger_candle,
                    exit_fill_candle.clone(),
                ],
                &mut registry,
                &risk_engine,
                &mut broker,
                &mut portfolio,
            )
            .await
            .unwrap();

        assert_eq!(report.candles_processed, 4);
        assert!(
            portfolio.open_positions().is_empty(),
            "expected the position to have been closed by the take-profit exit"
        );
        let closed = portfolio
            .closed_positions()
            .first()
            .expect("expected exactly one closed position from the take-profit exit");

        // Nesta série sintética de candles contíguos de 1 minuto,
        // close_time(exit_trigger_candle) == open_time(exit_fill_candle) —
        // então o timestamp sozinho não distingue "preenchido no candle do
        // rompimento" de "preenchido no próximo candle". A prova real está
        // em `exit_price` abaixo: se a execução tivesse usado o close() do
        // candle do rompimento (400) como referência, em vez do open() do
        // candle seguinte (1000), o preço resultante não passaria da
        // asserção de magnitude a seguir.
        assert_eq!(
            closed.closed_at,
            Some(exit_fill_candle.open_time),
            "a saída disparada por risco deve ser preenchida no open() do candle seguinte ao \
             que rompeu o limiar, nunca no close() que a disparou"
        );
        // spread + slippage não deslocam o preço o suficiente para
        // confundir 1000 (open de T+3) com 400 (close de T+2).
        let exit_price = closed
            .exit_price
            .expect("expected exit_price to be set on a closed position");
        assert!(
            exit_price > dec!(700),
            "exit_price ({exit_price}) deve refletir o open() do candle seguinte ao \
             rompimento (~1000), não o close() que o disparou (400)"
        );
    }

    /// Estratégia de teste que emite uma entrada Long na primeira chamada
    /// e, a partir daí, emite seu próprio `Flat` sempre que
    /// `PositionQuery` mostra uma posição real aberta — reproduzindo o
    /// padrão das três estratégias quantitativas (saída própria baseada em
    /// posição real, não em estado interno).
    struct FireOnceThenFlatWhilePositioned {
        id: domain::StrategyId,
        requirements: strategies::StrategyRequirements,
        /// Nº de candles já vistos. 1 = dispara a entrada; a partir do 3º
        /// (candle seguinte ao que preenche a entrada) passa a emitir seu
        /// próprio `Flat` sempre que `PositionQuery` mostra posição real
        /// aberta — nunca no 2º candle (o próprio candle de preenchimento),
        /// que reproduziria uma saída prematura irrelevante ao cenário
        /// testado.
        calls: usize,
    }

    impl FireOnceThenFlatWhilePositioned {
        fn new() -> Self {
            Self {
                id: domain::StrategyId::new("flat-while-positioned-test").unwrap(),
                requirements: strategies::StrategyRequirements::new(
                    vec![MarketDataKind::Ohlcv],
                    vec![AssetClass::Crypto],
                ),
                calls: 0,
            }
        }
    }

    impl strategies::Strategy for FireOnceThenFlatWhilePositioned {
        fn id(&self) -> &domain::StrategyId {
            &self.id
        }

        fn requirements(&self) -> &strategies::StrategyRequirements {
            &self.requirements
        }

        fn on_event(
            &mut self,
            instrument: &Instrument,
            event: &MarketEvent,
            positions: &dyn strategies::PositionQuery,
        ) -> Option<domain::Signal> {
            let MarketEvent::Candle(candle) = event else {
                return None;
            };
            self.calls += 1;
            if self.calls == 1 {
                return Some(
                    domain::Signal::new(
                        self.id.clone(),
                        instrument.id,
                        domain::SignalDirection::Long,
                        1.0,
                        candle.close_time,
                        None,
                        None,
                        serde_json::Value::Null,
                    )
                    .unwrap(),
                );
            }
            if self.calls < 3 {
                return None;
            }
            positions.open_position_side(instrument.id)?;
            Some(
                domain::Signal::new(
                    self.id.clone(),
                    instrument.id,
                    domain::SignalDirection::Flat,
                    1.0,
                    candle.close_time,
                    None,
                    None,
                    serde_json::Value::Null,
                )
                .unwrap(),
            )
        }
    }

    /// Regressão: um candle onde a saída por stop/take-profit dispara *e*
    /// a própria estratégia (vendo, via `PositionQuery`, que ainda está
    /// posicionada — porque a saída por risco só foi enfileirada, não
    /// executada) também emite seu próprio `Flat` não pode gerar duas
    /// ordens de fechamento para a mesma posição. Antes da correção, as
    /// duas eram enfileiradas e a segunda falhava ao drenar (a posição já
    /// tinha sido fechada pela primeira) — exatamente o erro
    /// "naked short" encontrado ao rodar o experiment runner contra dados
    /// reais da Binance.
    #[tokio::test]
    async fn risk_exit_and_strategys_own_flat_on_the_same_candle_never_double_close() {
        let instrument = instrument();
        let runner = BacktestRunner::new(vec![instrument.clone()]);

        let mut registry = StrategyRegistry::new();
        let available = HashSet::from([MarketDataKind::Ohlcv]);
        registry
            .register(
                Box::new(FireOnceThenFlatWhilePositioned::new()),
                std::slice::from_ref(&instrument),
                &available,
            )
            .unwrap();

        let risk_engine = timing_test_risk_engine(Some(dec!(0.5)));
        let mut broker = timing_test_broker();
        let mut portfolio = PortfolioManager::new(Money::new(dec!(100000)));

        let base = Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap();
        let signal_candle = Candle {
            instrument_id: instrument.id,
            timeframe: Timeframe::M1,
            open_time: base,
            close_time: base + chrono::Duration::minutes(1),
            open: dec!(100),
            high: dec!(100),
            low: dec!(100),
            close: dec!(100),
            volume: dec!(1),
            is_closed: true,
        };
        let fill_candle = Candle {
            instrument_id: instrument.id,
            timeframe: Timeframe::M1,
            open_time: base + chrono::Duration::minutes(1),
            close_time: base + chrono::Duration::minutes(2),
            open: dec!(200),
            high: dec!(205),
            low: dec!(200),
            close: dec!(205),
            volume: dec!(1),
            is_closed: true,
        };
        // T+2: close (400) rompe o take profit de 50% — a saída por risco
        // é enfileirada aqui. A ESTRATÉGIA também é despachada neste mesmo
        // candle e, vendo a posição ainda aberta (a saída por risco só foi
        // enfileirada, não executada), também tentaria enfileirar seu
        // próprio Flat — é exatamente essa duplicata que a correção evita.
        let exit_trigger_candle = Candle {
            instrument_id: instrument.id,
            timeframe: Timeframe::M1,
            open_time: base + chrono::Duration::minutes(2),
            close_time: base + chrono::Duration::minutes(3),
            open: dec!(210),
            high: dec!(400),
            low: dec!(210),
            close: dec!(400),
            volume: dec!(1),
            is_closed: true,
        };
        let exit_fill_candle = Candle {
            instrument_id: instrument.id,
            timeframe: Timeframe::M1,
            open_time: base + chrono::Duration::minutes(3),
            close_time: base + chrono::Duration::minutes(4),
            open: dec!(1000),
            high: dec!(1000),
            low: dec!(1000),
            close: dec!(1000),
            volume: dec!(1),
            is_closed: true,
        };

        // Antes da correção, isto retornava `Err` (naked short) em vez de
        // `Ok` — a asserção principal desta regressão é `.unwrap()` não
        // entrar em panic.
        let report = runner
            .run(
                vec![
                    signal_candle,
                    fill_candle,
                    exit_trigger_candle,
                    exit_fill_candle,
                ],
                &mut registry,
                &risk_engine,
                &mut broker,
                &mut portfolio,
            )
            .await
            .unwrap();

        assert_eq!(report.candles_processed, 4);
        assert!(portfolio.open_positions().is_empty());
        assert_eq!(
            portfolio.closed_positions().len(),
            1,
            "exactly one close must happen, not two"
        );
    }

    /// Requisito 9: `BacktestRunner` com múltiplos robôs no mesmo
    /// instrumento (registry construído via `strategies::build_registry`,
    /// Fase 1.5) funciona corretamente — as duas instâncias de
    /// `ema_crossover`, com ids diferentes, abrem sua própria posição no
    /// mesmo crossover, sem que uma rejeite ou substitua a outra (ADR-20).
    #[tokio::test]
    async fn backtest_runner_supports_multiple_robots_on_the_same_instrument() {
        let instrument = instrument();
        let runner = BacktestRunner::new(vec![instrument.clone()]);

        let configs = vec![
            strategies::StrategyInstanceConfig {
                id: domain::StrategyId::new("ema-robot-a").unwrap(),
                kind: "ema_crossover".to_string(),
                symbols: vec![instrument.symbol.to_string()],
            },
            strategies::StrategyInstanceConfig {
                id: domain::StrategyId::new("ema-robot-b").unwrap(),
                kind: "ema_crossover".to_string(),
                symbols: vec![instrument.symbol.to_string()],
            },
        ];
        let available = HashSet::from([MarketDataKind::Ohlcv]);
        let mut registry =
            strategies::build_registry(&configs, std::slice::from_ref(&instrument), &available)
                .unwrap();
        assert_eq!(registry.len(), 2);

        let risk_engine = RiskEngine::new(RiskConfig {
            order_notional: Money::new(dec!(1000)),
            max_position_notional: Money::new(dec!(1000)),
            max_total_exposure: Money::new(dec!(5000)),
            max_open_positions: 5,
            stop_loss_pct: None,
            take_profit_pct: None,
            max_daily_loss: Money::new(dec!(1000)),
        });
        let mut broker = PaperBroker::new(PaperBrokerConfig {
            maker_fee: dec!(0.0005),
            taker_fee: dec!(0.001),
            spread_bps: dec!(2),
            slippage_bps: dec!(3),
        });
        let mut portfolio = PortfolioManager::new(Money::new(dec!(100000)));

        // Série de queda-e-alta análoga à de
        // `replays_candles_and_opens_a_position_on_crossover`, mas com um
        // candle extra: `strategies::build_registry` sempre constrói com
        // os params default do catálogo (`fast_period: 12, slow_period: 26`,
        // bem mais lentos que o `fast: 2, slow: 4` usado à mão naquele
        // outro teste), então o crossover de alta só ocorre no 7º candle —
        // o 8º existe só para dar ao runner o "próximo candle disponível"
        // exigido para preencher a entrada (ADR-18: sinal em close(T), fill
        // em open(T+1); sem T+1 aqui, a ordem ficaria pendente para sempre).
        let prices = [
            dec!(100),
            dec!(90),
            dec!(80),
            dec!(70),
            dec!(90),
            dec!(120),
            dec!(150),
            dec!(160),
        ];
        let candles: Vec<Candle> = prices
            .iter()
            .enumerate()
            .map(|(i, p)| candle(instrument.id, *p, i as i64))
            .collect();

        let report = runner
            .run(
                candles,
                &mut registry,
                &risk_engine,
                &mut broker,
                &mut portfolio,
            )
            .await
            .unwrap();

        assert_eq!(report.candles_processed, 8);
        assert_eq!(
            portfolio.open_positions().len(),
            2,
            "both robots must have opened their own position on the same crossover"
        );
        let robot_a = portfolio
            .open_position_for(
                instrument.id,
                &domain::StrategyId::new("ema-robot-a").unwrap(),
            )
            .expect("robot-a must have an open position");
        let robot_b = portfolio
            .open_position_for(
                instrument.id,
                &domain::StrategyId::new("ema-robot-b").unwrap(),
            )
            .expect("robot-b must have an open position");
        assert_eq!(robot_a.side, robot_b.side);
        assert_eq!(robot_a.entry_price, robot_b.entry_price);
        assert_ne!(
            robot_a.id, robot_b.id,
            "must be two distinct positions, not one shared"
        );
    }
}
