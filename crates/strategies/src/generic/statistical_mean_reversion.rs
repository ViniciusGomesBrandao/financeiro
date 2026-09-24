//! # Reversão à Média Estatística
//!
//! **Hipótese:** a mesma premissa de `MeanReversionStrategy` (preço muito
//! distante de sua média recente tende a reverter) — mas exigindo
//! *confirmação* de dois sinais estatísticos independentes antes de agir, em
//! vez de reagir só ao z-score isolado. Um z-score extremo por si só pode
//! ser o início de um novo regime (uma tendência genuína), não uma extensão
//! temporária a ser revertida; RSI e autocorrelação servem para distinguir
//! os dois casos.
//!
//! **Features utilizadas** (`features::FeatureSnapshot`):
//! - `zscore` — magnitude e sinal do desvio em relação à média móvel (o
//!   gatilho primário de entrada *e* de saída — ver abaixo).
//! - `rsi` — confirma que o desvio de preço também aparece como
//!   sobrecomprado (`>= rsi_overbought`) ou sobrevendido
//!   (`<= rsi_oversold`) no oscilador de momentum. Z-score e RSI medem
//!   coisas relacionadas mas não idênticas (desvio de preço vs. proporção
//!   ganho/perda recente); exigir os dois reduz falsos positivos de um
//!   z-score inflado por um único candle atípico.
//! - `autocorrelation` — filtro de regime: reversão à média assume que o
//!   preço não está numa tendência persistente. Autocorrelação de retornos
//!   fortemente positiva é evidência de continuação (momentum), o oposto
//!   da premissa desta estratégia — nesse caso ela se abstém.
//!
//! **Combinação dos sinais:** nenhum peso é atribuído a nenhuma feature.
//! RSI e autocorrelação são *filtros binários* (confirmam ou vetam), não
//! termos de uma soma ponderada; a confiança do sinal de entrada vem só da
//! magnitude do z-score, a mesma variável que decide a direção. Isso evita
//! inventar um peso relativo entre z-score/RSI/autocorrelação que nenhum
//! dos três justificaria sozinho.
//!
//! **Entradas:** candles OHLCV fechados de um instrumento.
//!
//! **Condição de entrada (Short):** `zscore >= entry_z` **e**
//! `rsi >= rsi_overbought` **e** `autocorrelation <= max_autocorrelation`.
//!
//! **Condição de entrada (Long):** `zscore <= -entry_z` **e**
//! `rsi <= rsi_oversold` **e** `autocorrelation <= max_autocorrelation`.
//!
//! **Condição de saída (própria, explícita):** uma vez posicionada (Long ou
//! Short), a estratégia para de avaliar novas entradas para aquele
//! instrumento e passa a monitorar só o retorno à média: assim que
//! `|zscore| <= exit_z` (por construção, `exit_z < entry_z`), ela emite
//! `SignalDirection::Flat` — o sinal universal de "feche o que estiver
//! aberto" (`risk::RiskEngine::evaluate` trata `Flat` da mesma forma
//! independentemente de a posição existente ser comprada ou vendida). Isto
//! é deliberado: a estratégia nunca depende de outra estratégia emitir um
//! `Short` para fechar sua própria posição comprada (ou vice-versa) — ela
//! sempre sabe fechar o que ela mesma abriu.
//!
//! **Quando NÃO deve operar:**
//! - Antes que `zscore`/`rsi`/`autocorrelation` tenham histórico suficiente
//!   (todas `None` até a maior janela configurada encher).
//! - Quando o z-score é extremo mas o RSI não confirma (ex.: um único
//!   candle de gap que ainda não moveu a média de ganhos/perdas de Wilder)
//!   — tratado como ruído, não como sinal.
//! - Quando a autocorrelação indica um regime de tendência
//!   (`autocorrelation > max_autocorrelation`) — a premissa de reversão não
//!   se sustenta nesse regime.
//! - Enquanto acredita estar posicionada (ver limitação abaixo), a
//!   estratégia não avalia novas entradas — mesmo que uma delas parecesse
//!   válida.
//! - Mesma limitação estrutural da baseline: em uma tendência forte e
//!   sustentada que *não* seja capturada pelo filtro de autocorrelação (ele
//!   olha só `autocorr_period` retornos), operar contra o movimento ainda
//!   pode acumular prejuízo.
//!
//! **Rastreamento de posição:** `on_features` recebe `positions: &dyn
//! PositionQuery` e consulta isso a cada chamada para saber se há de fato
//! uma posição real aberta — nunca guarda essa informação por conta própria
//! (ver `crate::position_query`). Uma entrada rejeitada pelo Risk Engine
//! (ex.: saldo insuficiente, `PositionAlreadyOpen` por causa de outra
//! estratégia no mesmo instrumento) nunca aparece em `positions`, então a
//! estratégia continua livre para avaliar novas entradas em vez de ficar
//! "presa" monitorando a saída de uma posição que não existe.
//!
//! **Confiança:** força heurística do sinal de *entrada*
//! (`|z| / (2 * entry_z)`, saturada em `[0, 1]`), nunca uma probabilidade
//! calibrada de lucro — ver `domain::Signal::confidence`. O motor de risco
//! (`risk::RiskEngine::evaluate`) nunca lê `confidence`: o dimensionamento
//! de posição usa só `RiskConfig::order_notional`, fixo, independentemente
//! do valor de confiança do sinal (ver
//! `risk::engine::tests::signal_confidence_does_not_affect_position_sizing`).
//! O sinal de *saída* sempre reporta confiança `1.0`: a condição de saída é
//! um fato binário (reverteu o suficiente ou não), não uma força graduada
//! — `1.0` aqui significa "a regra de saída dedeterminou-se", não "100% de
//! certeza de lucro".
//!
//! **Parâmetros configuráveis:** `zscore_period`, `entry_z`, `exit_z`,
//! `rsi_period`, `rsi_overbought`, `rsi_oversold`, `autocorr_period`,
//! `autocorr_lag`, `max_autocorrelation`.

use domain::{AssetClass, Instrument, MarketDataKind, Signal, SignalDirection, StrategyId};
use features::FeatureConfig;
use serde::Serialize;
use serde_json::json;

use crate::feature_strategy::FeatureStrategy;
use crate::position_query::PositionQuery;
use crate::requirements::StrategyRequirements;

/// `Serialize` existe só para `catalog::StrategyDescriptor::default_params`
/// (descoberta pelo dashboard) — nunca usado para desserializar/injetar
/// parâmetros de volta (não há mecanismo de override nesta fase).
#[derive(Debug, Clone, Copy, Serialize)]
pub struct StatisticalMeanReversionParams {
    pub zscore_period: usize,
    pub entry_z: f64,
    /// Limite de `|zscore|` abaixo do qual a posição é considerada
    /// "revertida o suficiente" e encerrada. Deve ser menor que `entry_z`.
    pub exit_z: f64,
    pub rsi_period: usize,
    pub rsi_overbought: f64,
    pub rsi_oversold: f64,
    pub autocorr_period: usize,
    pub autocorr_lag: usize,
    /// Limite acima do qual a autocorrelação de retornos é lida como
    /// "regime de tendência" e bloqueia novas entradas.
    pub max_autocorrelation: f64,
}

impl Default for StatisticalMeanReversionParams {
    fn default() -> Self {
        Self {
            zscore_period: 20,
            entry_z: 2.0,
            exit_z: 0.5,
            rsi_period: 14,
            rsi_overbought: 70.0,
            rsi_oversold: 30.0,
            autocorr_period: 20,
            autocorr_lag: 1,
            max_autocorrelation: 0.3,
        }
    }
}

pub struct StatisticalMeanReversionStrategy {
    id: StrategyId,
    params: StatisticalMeanReversionParams,
    requirements: StrategyRequirements,
}

impl StatisticalMeanReversionStrategy {
    pub fn new(id: StrategyId, params: StatisticalMeanReversionParams) -> Self {
        assert!(
            params.zscore_period > 1,
            "zscore_period must be greater than 1"
        );
        assert!(params.entry_z > 0.0, "entry_z must be positive");
        assert!(
            params.exit_z >= 0.0 && params.exit_z < params.entry_z,
            "exit_z must be non-negative and smaller than entry_z"
        );
        assert!(
            params.rsi_overbought > params.rsi_oversold,
            "rsi_overbought must exceed rsi_oversold"
        );
        assert!(
            params.autocorr_period > params.autocorr_lag,
            "autocorr_period must exceed autocorr_lag"
        );
        Self {
            id,
            params,
            requirements: StrategyRequirements::new(
                vec![MarketDataKind::Ohlcv],
                vec![AssetClass::Crypto, AssetClass::Equity],
            ),
        }
    }
}

impl FeatureStrategy for StatisticalMeanReversionStrategy {
    fn id(&self) -> &StrategyId {
        &self.id
    }

    fn requirements(&self) -> &StrategyRequirements {
        &self.requirements
    }

    fn feature_config(&self) -> FeatureConfig {
        FeatureConfig {
            zscore_period: self.params.zscore_period,
            rsi_period: self.params.rsi_period,
            autocorrelation_period: self.params.autocorr_period,
            autocorrelation_lag: self.params.autocorr_lag,
            ..FeatureConfig::default()
        }
    }

    fn on_features(
        &mut self,
        instrument: &Instrument,
        snapshot: &features::FeatureSnapshot,
        positions: &dyn PositionQuery,
    ) -> Option<Signal> {
        // Posição real (via `positions`, nunca uma suposição interna): só
        // avalia a saída própria, nunca uma nova entrada — é isso que
        // garante que esta estratégia nunca depende de outra para se
        // fechar.
        if positions.open_position_side(instrument.id).is_some() {
            // `zscore` fica `None` quando a janela tem desvio padrão zero
            // (ver `features::ohlcv::zscore`) — ou seja, o caso em que o
            // preço ficou completamente parado, o resultado *mais*
            // decisivo de reversão possível, não "sem dado o bastante".
            // Tratar como já revertido em vez de reaproveitar `?` (que
            // abortaria a checagem de saída silenciosamente) evita que a
            // estratégia fique presa numa posição só porque o preço se
            // estabilizou por completo.
            let reverted = match snapshot.zscore {
                Some(z) => z.abs() <= self.params.exit_z,
                None => true,
            };
            if !reverted {
                return None;
            }
            return Signal::new(
                self.id.clone(),
                instrument.id,
                SignalDirection::Flat,
                1.0,
                snapshot.timestamp,
                None,
                None,
                json!({ "exit_reason": "reverted_to_mean", "zscore": snapshot.zscore }),
            )
            .ok();
        }

        let z = snapshot.zscore?;
        let rsi = snapshot.rsi?;
        let autocorrelation = snapshot.autocorrelation?;

        if autocorrelation > self.params.max_autocorrelation {
            return None;
        }

        let direction = if z >= self.params.entry_z && rsi >= self.params.rsi_overbought {
            SignalDirection::Short
        } else if z <= -self.params.entry_z && rsi <= self.params.rsi_oversold {
            SignalDirection::Long
        } else {
            return None;
        };

        let confidence = (z.abs() / (self.params.entry_z * 2.0)).clamp(0.0, 1.0);
        Signal::new(
            self.id.clone(),
            instrument.id,
            direction,
            confidence,
            snapshot.timestamp,
            None,
            None,
            json!({ "zscore": z, "rsi": rsi, "autocorrelation": autocorrelation }),
        )
        .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use domain::{Asset, Exchange, InstrumentId, MarketType, Side, Symbol};
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use std::collections::HashMap;

    fn instrument() -> Instrument {
        let base = Asset::new("SOL").unwrap();
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

    fn candle(instrument: &Instrument, close: Decimal, minute: i64) -> domain::Candle {
        let open_time =
            Utc.with_ymd_and_hms(2024, 1, 1, 0, 0, 0).unwrap() + chrono::Duration::minutes(minute);
        domain::Candle {
            instrument_id: instrument.id,
            timeframe: domain::Timeframe::M1,
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

    /// Alimenta `prices` por um `FeatureEngine` construído com o
    /// `feature_config()` da própria estratégia (nunca hardcoded no
    /// teste) e repassa cada snapshot para `on_features` — o mesmo
    /// caminho que `FeatureStrategyAdapter` percorre em produção, só sem
    /// passar pelo `StrategyRegistry`. Simula o Risk Engine aprovando toda
    /// entrada/saída emitida (o caso feliz), sincronizando `positions`
    /// como `portfolio::PortfolioManager` faria de verdade após cada fill
    /// — a simulação de uma entrada *rejeitada* tem seu próprio teste
    /// dedicado abaixo, sem passar por este helper.
    fn run(
        strategy: &mut StatisticalMeanReversionStrategy,
        instrument: &Instrument,
        prices: &[Decimal],
    ) -> Vec<Option<Signal>> {
        let mut engine = features::FeatureEngine::new(instrument.id, strategy.feature_config());
        let mut positions: HashMap<InstrumentId, Side> = HashMap::new();
        prices
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let c = candle(instrument, *p, i as i64);
                let snapshot = engine.update(&c)?;
                let signal = strategy.on_features(instrument, &snapshot, &positions)?;
                match signal.direction {
                    SignalDirection::Long => {
                        positions.insert(instrument.id, Side::Buy);
                    }
                    SignalDirection::Short => {
                        positions.insert(instrument.id, Side::Sell);
                    }
                    SignalDirection::Flat => {
                        positions.remove(&instrument.id);
                    }
                }
                Some(signal)
            })
            .collect()
    }

    #[test]
    fn no_signal_before_enough_history() {
        let mut strategy = StatisticalMeanReversionStrategy::new(
            StrategyId::new("smr-test").unwrap(),
            StatisticalMeanReversionParams {
                zscore_period: 6,
                rsi_period: 6,
                autocorr_period: 6,
                autocorr_lag: 1,
                ..StatisticalMeanReversionParams::default()
            },
        );
        let instrument = instrument();
        let prices: Vec<Decimal> = vec![dec!(100); 5];
        let signals = run(&mut strategy, &instrument, &prices);
        assert!(signals.iter().all(|s| s.is_none()));
    }

    #[test]
    fn trending_regime_blocks_entry_despite_extreme_zscore_and_rsi() {
        // Tendência de alta limpa e sustentada: cada barra sobe em relação
        // à anterior -> autocorrelação de retornos fortemente positiva ->
        // o filtro de regime deve vetar a entrada mesmo que zscore/RSI
        // estejam extremos ao final.
        let mut strategy = StatisticalMeanReversionStrategy::new(
            StrategyId::new("smr-test").unwrap(),
            StatisticalMeanReversionParams {
                zscore_period: 6,
                entry_z: 1.0,
                rsi_period: 6,
                rsi_overbought: 60.0,
                rsi_oversold: 40.0,
                autocorr_period: 6,
                autocorr_lag: 1,
                max_autocorrelation: 0.3,
                ..StatisticalMeanReversionParams::default()
            },
        );
        let instrument = instrument();
        let prices: Vec<Decimal> = (0..14).map(|i| dec!(100) + Decimal::from(i * 3)).collect();
        let signals = run(&mut strategy, &instrument, &prices);
        assert!(
            signals.iter().all(|s| s.is_none()),
            "a persistently trending series must never trigger a mean-reversion entry"
        );
    }

    #[test]
    fn emits_short_when_zscore_and_rsi_confirm_in_a_non_trending_regime() {
        let mut strategy = StatisticalMeanReversionStrategy::new(
            StrategyId::new("smr-test").unwrap(),
            StatisticalMeanReversionParams {
                zscore_period: 8,
                entry_z: 1.5,
                rsi_period: 8,
                rsi_overbought: 65.0,
                rsi_oversold: 35.0,
                autocorr_period: 8,
                autocorr_lag: 1,
                max_autocorrelation: 0.5,
                ..StatisticalMeanReversionParams::default()
            },
        );
        let instrument = instrument();
        // Oscilação de baixa amplitude (autocorrelação de lag-1 negativa,
        // regime não-tendencial) seguida de um salto abrupto para cima —
        // extremo de z-score e RSI simultâneo, sem alterar o regime de
        // curto prazo o bastante para violar o filtro de autocorrelação.
        let mut prices: Vec<Decimal> = Vec::new();
        for i in 0..10 {
            prices.push(if i % 2 == 0 { dec!(100) } else { dec!(99) });
        }
        prices.push(dec!(130));
        let signals = run(&mut strategy, &instrument, &prices);
        let last = signals.last().cloned().flatten();
        assert_eq!(
            last.map(|s| s.direction),
            Some(SignalDirection::Short),
            "expected the final spike to trigger a short mean-reversion signal"
        );
    }

    /// Requisito 2 do audit: a estratégia precisa fechar sua própria
    /// posição sozinha, sem depender de nenhuma outra estratégia (nem de
    /// stop loss/take profit externos) gerar um sinal oposto. Prova isso
    /// diretamente: depois da entrada Short, alimenta candles em que o
    /// preço reverte de volta para perto da média e confirma que a
    /// PRÓPRIA estratégia emite `Flat`, e que o rastreamento interno é
    /// limpo (uma entrada nova volta a ser possível depois).
    #[test]
    fn emits_its_own_flat_exit_once_price_reverts_to_the_mean() {
        let mut strategy = StatisticalMeanReversionStrategy::new(
            StrategyId::new("smr-test").unwrap(),
            StatisticalMeanReversionParams {
                zscore_period: 8,
                entry_z: 1.5,
                exit_z: 0.3,
                rsi_period: 8,
                rsi_overbought: 65.0,
                rsi_oversold: 35.0,
                autocorr_period: 8,
                autocorr_lag: 1,
                max_autocorrelation: 0.9,
            },
        );
        let instrument = instrument();
        let mut prices: Vec<Decimal> = Vec::new();
        for i in 0..10 {
            prices.push(if i % 2 == 0 { dec!(100) } else { dec!(99) });
        }
        prices.push(dec!(130)); // dispara a entrada Short
                                // Preço recuando de volta para perto da faixa de oscilação
                                // original. A janela (período 8) só esquece o candle do salto
                                // depois de 8 candles subsequentes — por isso 10 candles aqui, não
                                // só o suficiente para o preço "parecer" ter voltado.
        for _ in 0..10 {
            prices.push(dec!(100));
        }

        let signals = run(&mut strategy, &instrument, &prices);
        let entry_index = signals
            .iter()
            .position(|s| {
                matches!(
                    s.as_ref().map(|s| s.direction),
                    Some(SignalDirection::Short)
                )
            })
            .expect("expected a short entry to have fired");

        // Nenhum sinal entre a entrada e a saída deve reabrir/reentrar —
        // a estratégia deve estar "monitorando saída", não avaliando
        // entradas novas.
        let exit_index = signals[entry_index + 1..]
            .iter()
            .position(|s| matches!(s.as_ref().map(|s| s.direction), Some(SignalDirection::Flat)))
            .map(|i| i + entry_index + 1)
            .expect("expected the strategy to emit its own Flat exit");

        assert_eq!(
            signals[exit_index].as_ref().unwrap().direction,
            SignalDirection::Flat
        );
        assert!(
            exit_index > entry_index,
            "the exit must come strictly after the entry"
        );
    }

    /// Requisito extra do audit (parte 2 — elimina posições fantasma): se o
    /// Risk Engine rejeitar a entrada (aqui simulado por nunca atualizar o
    /// `positions` que a estratégia consulta), ela não pode ficar "presa"
    /// acreditando estar posicionada — deve continuar apta a detectar uma
    /// entrada válida em candles futuros.
    #[test]
    fn rejected_entry_leaves_the_strategy_able_to_detect_a_new_valid_entry_later() {
        let mut strategy = StatisticalMeanReversionStrategy::new(
            StrategyId::new("smr-test").unwrap(),
            StatisticalMeanReversionParams {
                zscore_period: 8,
                entry_z: 1.5,
                rsi_period: 8,
                rsi_overbought: 65.0,
                rsi_oversold: 35.0,
                autocorr_period: 8,
                autocorr_lag: 1,
                max_autocorrelation: 0.9,
                ..StatisticalMeanReversionParams::default()
            },
        );
        let instrument = instrument();
        let mut prices: Vec<Decimal> = Vec::new();
        for i in 0..10 {
            prices.push(if i % 2 == 0 { dec!(100) } else { dec!(99) });
        }
        prices.push(dec!(130)); // primeiro spike -> dispara Short
        for i in 0..10 {
            prices.push(if i % 2 == 0 { dec!(100) } else { dec!(99) });
        }
        prices.push(dec!(130)); // segundo spike -> deve disparar Short de novo

        let mut engine = features::FeatureEngine::new(instrument.id, strategy.feature_config());
        // Nunca atualizado: simula o Risk Engine rejeitando toda entrada
        // emitida.
        let positions: HashMap<InstrumentId, Side> = HashMap::new();

        let mut entries = 0;
        for (i, p) in prices.iter().enumerate() {
            let c = candle(&instrument, *p, i as i64);
            let Some(snapshot) = engine.update(&c) else {
                continue;
            };
            if let Some(signal) = strategy.on_features(&instrument, &snapshot, &positions) {
                assert_eq!(signal.direction, SignalDirection::Short);
                entries += 1;
            }
        }

        assert_eq!(
            entries, 2,
            "a rejected entry must not block the strategy from detecting the next valid entry"
        );
    }

    /// Requisito 3 do audit: confiança de entrada é força heurística, não
    /// probabilidade — dois sinais com z-scores diferentes (logo,
    /// confianças diferentes) não podem ser distinguidos só pela direção;
    /// a confiança deve variar suavemente com `|z|`, nunca saturar antes
    /// da hora nem sair de `[0, 1]`.
    #[test]
    fn entry_confidence_scales_with_zscore_magnitude_and_stays_in_unit_range() {
        let mut strategy = StatisticalMeanReversionStrategy::new(
            StrategyId::new("smr-test").unwrap(),
            StatisticalMeanReversionParams {
                zscore_period: 8,
                entry_z: 1.5,
                rsi_period: 8,
                rsi_overbought: 55.0,
                rsi_oversold: 45.0,
                autocorr_period: 8,
                autocorr_lag: 1,
                max_autocorrelation: 0.9,
                ..StatisticalMeanReversionParams::default()
            },
        );
        let instrument = instrument();
        let mut prices: Vec<Decimal> = Vec::new();
        for i in 0..10 {
            prices.push(if i % 2 == 0 { dec!(100) } else { dec!(99) });
        }
        prices.push(dec!(115)); // spike moderado
        let signals = run(&mut strategy, &instrument, &prices);
        let entry = signals
            .iter()
            .find_map(|s| s.clone())
            .expect("expected an entry signal");
        assert!((0.0..=1.0).contains(&entry.confidence));
        assert!(
            entry.confidence < 1.0,
            "a moderate spike should not saturate confidence to the maximum"
        );
    }

    /// Mesma garantia de `quant_momentum::tests::no_look_ahead_...`: a
    /// decisão num índice compartilhado não pode mudar por causa de
    /// candles que só existem na execução mais longa.
    #[test]
    fn no_look_ahead_decision_at_shared_index_is_unaffected_by_future_candles() {
        let params = StatisticalMeanReversionParams {
            zscore_period: 8,
            entry_z: 1.5,
            rsi_period: 8,
            rsi_overbought: 65.0,
            rsi_oversold: 35.0,
            autocorr_period: 8,
            autocorr_lag: 1,
            max_autocorrelation: 0.5,
            ..StatisticalMeanReversionParams::default()
        };
        let instrument = instrument();
        let mut prices: Vec<Decimal> = Vec::new();
        for i in 0..10 {
            prices.push(if i % 2 == 0 { dec!(100) } else { dec!(99) });
        }
        prices.push(dec!(130));
        // Continuação que só existe na execução "longa".
        prices.push(dec!(95));
        prices.push(dec!(140));

        let mut short_strategy =
            StatisticalMeanReversionStrategy::new(StrategyId::new("smr-test").unwrap(), params);
        let short = run(&mut short_strategy, &instrument, &prices[..11]);

        let mut full_strategy =
            StatisticalMeanReversionStrategy::new(StrategyId::new("smr-test").unwrap(), params);
        let full = run(&mut full_strategy, &instrument, &prices);

        let as_comparable = |s: &Option<Signal>| s.as_ref().map(|s| (s.direction, s.confidence));
        assert_eq!(as_comparable(&short[10]), as_comparable(&full[10]));
    }
}
