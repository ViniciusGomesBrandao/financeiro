//! # Momentum Quantitativo
//!
//! **Hipótese:** a mesma premissa de `MomentumStrategy` (tendência
//! recente tende a persistir) — mas medindo "tendência" de um jeito mais
//! robusto que o retorno ponta-a-ponta da baseline (que depende só de dois
//! preços, o primeiro e o último da janela, e é sensível a ruído em
//! qualquer um dos dois). Aqui a tendência é a inclinação de uma regressão
//! linear sobre toda a janela, só considerada quando (a) ela realmente
//! explica boa parte do movimento observado e (b) vem acompanhada de
//! volume acima da média — um movimento "limpo" e "participado", não um
//! ziguezague que por acaso terminou mais alto.
//!
//! **Features utilizadas** (`features::FeatureSnapshot`):
//! - `regression.slope` — direção e magnitude da tendência (sinal decide
//!   Long/Short na entrada; reversão de sinal também dispara a saída).
//! - `regression.r_squared` — qualidade da tendência: fração da variância
//!   do preço explicada pela reta ajustada, em `[0, 1]`. Um slope grande
//!   com R² baixo é uma tendência "ruidosa" (o preço ricocheteou bastante
//!   ao redor da reta) — não é o que esta estratégia quer capturar, nem
//!   para entrar nem para continuar segurando uma posição.
//! - `relative_volume` — confirma participação: uma tendência genuína
//!   costuma vir acompanhada de volume acima da média; uma tendência sem
//!   volume tem mais chance de ser um movimento fino/pouco líquido, mais
//!   fácil de reverter.
//!
//! **Combinação dos sinais:** `r_squared` e `relative_volume` são *filtros*
//! (a tendência só é considerada válida se ambos passarem do limite
//! configurado), não termos de uma soma ponderada. A confiança do sinal de
//! entrada é o próprio `r_squared` — já normalizado em `[0, 1]` pela
//! própria definição estatística de R², então não há necessidade de
//! inventar nenhuma escala ou peso adicional para transformá-lo em
//! confiança.
//!
//! **Entradas:** candles OHLCV fechados de um instrumento.
//!
//! **Condição de entrada (Long):** `regression.slope > 0` **e**
//! `regression.r_squared >= min_r_squared` **e**
//! `relative_volume >= min_relative_volume`.
//!
//! **Condição de entrada (Short):** `regression.slope < 0` **e** as mesmas
//! duas condições de confirmação.
//!
//! **Condição de saída (própria, explícita):** uma vez posicionada, a
//! estratégia para de avaliar novas entradas para aquele instrumento e
//! passa a monitorar só o enfraquecimento da tendência: emite
//! `SignalDirection::Flat` assim que `regression.r_squared <
//! exit_r_squared` (a tendência deixou de ser "limpa" — por construção,
//! `exit_r_squared < min_r_squared`) **ou** o sinal do `slope` inverte em
//! relação à direção da posição (a tendência que justificou a entrada
//! acabou ou se inverteu). Como em `StatisticalMeanReversionStrategy`,
//! isto é deliberado: a estratégia nunca depende de outra estratégia (ou
//! de um stop loss/take profit externo) para fechar o que ela mesma abriu.
//!
//! **Quando NÃO deve operar:**
//! - Antes que `regression`/`relative_volume` tenham histórico suficiente.
//! - Mercado lateral/ruidoso, onde `r_squared` fica abaixo do limite — a
//!   reta ajustada não descreve bem o que o preço está fazendo.
//! - Movimentos de baixa liquidez/baixo volume, mesmo que a tendência
//!   pareça limpa no preço isolado — `relative_volume` abaixo do limite.
//! - Enquanto acredita estar posicionada (ver limitação abaixo), não
//!   avalia novas entradas.
//! - Mesma limitação estrutural da baseline: reversões de tendência são
//!   detectadas só depois de acontecerem (a regressão é sempre sobre o
//!   passado); esta estratégia não antecipa pontos de inflexão — inclusive
//!   na saída, que só reage depois que `r_squared`/`slope` já mudaram.
//!
//! **Rastreamento de posição:** `on_features` recebe `positions: &dyn
//! PositionQuery` e consulta isso a cada chamada para saber se há de fato
//! uma posição real aberta — nunca guarda essa informação por conta própria
//! (ver `crate::position_query`). Uma entrada rejeitada pelo Risk Engine
//! nunca aparece em `positions`, então a estratégia continua livre para
//! avaliar novas entradas; uma entrada de fato executada aparece assim que
//! `positions` refletir o Portfolio real, e só então a saída própria passa
//! a ser monitorada.
//!
//! **Confiança:** força heurística da entrada (`r_squared`, em `[0, 1]`
//! pela própria definição estatística), nunca uma probabilidade calibrada
//! de lucro. O sinal de saída sempre reporta confiança `1.0` — um fato
//! binário (a condição de saída disparou ou não), não uma força graduada.
//! O motor de risco nunca lê `confidence` para dimensionar a ordem — ver
//! `risk::engine::tests::signal_confidence_does_not_affect_position_sizing`.
//!
//! **Parâmetros configuráveis:** `regression_period`, `min_r_squared`,
//! `exit_r_squared`, `relative_volume_period`, `min_relative_volume`.

use domain::{AssetClass, Instrument, MarketDataKind, Side, Signal, SignalDirection, StrategyId};
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
pub struct QuantMomentumParams {
    pub regression_period: usize,
    /// R² mínimo (em `[0, 1]`) para considerar a tendência ajustada
    /// confiável o bastante para *entrar*.
    pub min_r_squared: f64,
    /// R² abaixo do qual a tendência é considerada "enfraquecida demais"
    /// para continuar segurando a posição. Deve ser menor que
    /// `min_r_squared`.
    pub exit_r_squared: f64,
    pub relative_volume_period: usize,
    /// Volume relativo mínimo (`1.0` = volume médio da janela) exigido
    /// para confirmar participação na tendência.
    pub min_relative_volume: f64,
}

impl Default for QuantMomentumParams {
    fn default() -> Self {
        Self {
            regression_period: 20,
            min_r_squared: 0.6,
            exit_r_squared: 0.3,
            relative_volume_period: 20,
            min_relative_volume: 1.0,
        }
    }
}

pub struct QuantMomentumStrategy {
    id: StrategyId,
    params: QuantMomentumParams,
    requirements: StrategyRequirements,
}

impl QuantMomentumStrategy {
    pub fn new(id: StrategyId, params: QuantMomentumParams) -> Self {
        assert!(
            params.regression_period >= 2,
            "regression_period must be at least 2"
        );
        assert!(
            (0.0..=1.0).contains(&params.min_r_squared),
            "min_r_squared must be in [0, 1]"
        );
        assert!(
            params.exit_r_squared >= 0.0 && params.exit_r_squared < params.min_r_squared,
            "exit_r_squared must be non-negative and smaller than min_r_squared"
        );
        assert!(
            params.min_relative_volume >= 0.0,
            "min_relative_volume must be non-negative"
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

impl FeatureStrategy for QuantMomentumStrategy {
    fn id(&self) -> &StrategyId {
        &self.id
    }

    fn requirements(&self) -> &StrategyRequirements {
        &self.requirements
    }

    fn feature_config(&self) -> FeatureConfig {
        FeatureConfig {
            regression_period: self.params.regression_period,
            relative_volume_period: self.params.relative_volume_period,
            ..FeatureConfig::default()
        }
    }

    fn on_features(
        &mut self,
        instrument: &Instrument,
        snapshot: &features::FeatureSnapshot,
        positions: &dyn PositionQuery,
    ) -> Option<Signal> {
        if let Some(side) = positions.open_position_side(instrument.id) {
            // Posição real (via `positions`, nunca uma suposição interna):
            // só avalia a saída própria, nunca uma nova entrada — é isso
            // que garante que esta estratégia nunca depende de outra para
            // se fechar.
            //
            // `regression` fica `None` quando a janela tem `SS_tot == 0`
            // (ver `features::ohlcv::regression`) — ou seja, o preço ficou
            // completamente parado dentro da janela inteira. Isso não é
            // "sem evidência de enfraquecimento": é a forma mais extrema
            // possível de enfraquecimento (a tendência morreu de vez), e
            // deve fechar a posição, não mantê-la às cegas.
            let (trend_weakened, trend_reversed, slope, r_squared) = match snapshot.regression {
                Some(regression) => {
                    let weakened = regression.r_squared < self.params.exit_r_squared;
                    let reversed = match side {
                        Side::Buy => regression.slope <= 0.0,
                        Side::Sell => regression.slope >= 0.0,
                    };
                    (
                        weakened,
                        reversed,
                        Some(regression.slope),
                        Some(regression.r_squared),
                    )
                }
                None => (true, false, None, None),
            };
            if !trend_weakened && !trend_reversed {
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
                json!({
                    "exit_reason": if trend_reversed { "trend_reversed" } else { "trend_weakened" },
                    "slope": slope,
                    "r_squared": r_squared,
                }),
            )
            .ok();
        }

        let regression = snapshot.regression?;
        let relative_volume = snapshot.relative_volume?;

        if regression.r_squared < self.params.min_r_squared {
            return None;
        }
        if relative_volume < self.params.min_relative_volume {
            return None;
        }

        let direction = if regression.slope > 0.0 {
            SignalDirection::Long
        } else if regression.slope < 0.0 {
            SignalDirection::Short
        } else {
            return None;
        };

        let confidence = regression.r_squared.clamp(0.0, 1.0);
        Signal::new(
            self.id.clone(),
            instrument.id,
            direction,
            confidence,
            snapshot.timestamp,
            None,
            None,
            json!({
                "slope": regression.slope,
                "r_squared": regression.r_squared,
                "relative_volume": relative_volume,
            }),
        )
        .ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{TimeZone, Utc};
    use domain::{Asset, Exchange, InstrumentId, MarketType, Symbol};
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;
    use std::collections::HashMap;

    fn instrument() -> Instrument {
        let base = Asset::new("ETH").unwrap();
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

    fn candle(
        instrument: &Instrument,
        close: Decimal,
        volume: Decimal,
        minute: i64,
    ) -> domain::Candle {
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
            volume,
            is_closed: true,
        }
    }

    /// Alimenta `prices`/`volumes` pelo pipeline de features e repassa cada
    /// snapshot para `on_features`, simulando o Risk Engine aprovando toda
    /// entrada/saída emitida (o caso feliz) — sincroniza `positions` como
    /// `portfolio::PortfolioManager` faria de verdade após cada fill, para
    /// que os testes de lógica de entrada/saída pura continuem válidos. A
    /// simulação de uma entrada *rejeitada* (o outro requisito do audit)
    /// tem seu próprio teste dedicado abaixo, sem passar por este helper.
    fn run(
        strategy: &mut QuantMomentumStrategy,
        instrument: &Instrument,
        prices: &[Decimal],
        volumes: &[Decimal],
    ) -> Vec<Option<Signal>> {
        let mut engine = features::FeatureEngine::new(instrument.id, strategy.feature_config());
        let mut positions: HashMap<InstrumentId, Side> = HashMap::new();
        prices
            .iter()
            .zip(volumes.iter())
            .enumerate()
            .map(|(i, (p, v))| {
                let c = candle(instrument, *p, *v, i as i64);
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
    fn clean_uptrend_with_average_volume_emits_long_entry() {
        let mut strategy = QuantMomentumStrategy::new(
            StrategyId::new("qm-test").unwrap(),
            QuantMomentumParams {
                regression_period: 5,
                min_r_squared: 0.9,
                relative_volume_period: 5,
                min_relative_volume: 1.0,
                ..QuantMomentumParams::default()
            },
        );
        let instrument = instrument();
        // Reta perfeita (R² = 1) + volume constante (relative_volume = 1.0
        // assim que a janela enche).
        let prices: Vec<Decimal> = (0..6).map(|i| dec!(100) + Decimal::from(i * 2)).collect();
        let volumes = vec![dec!(10); 6];
        let signals = run(&mut strategy, &instrument, &prices, &volumes);
        let entry = signals
            .iter()
            .find_map(|s| s.clone())
            .expect("expected a long entry to fire once the window fills");
        assert_eq!(entry.direction, SignalDirection::Long);
    }

    #[test]
    fn choppy_series_with_low_r_squared_emits_no_signal() {
        let mut strategy = QuantMomentumStrategy::new(
            StrategyId::new("qm-test").unwrap(),
            QuantMomentumParams {
                regression_period: 6,
                min_r_squared: 0.5,
                relative_volume_period: 6,
                min_relative_volume: 0.0,
                ..QuantMomentumParams::default()
            },
        );
        let instrument = instrument();
        // Sobe-desce sem tendência líquida clara na janela -> R² baixo.
        let prices: Vec<Decimal> = vec![
            dec!(100),
            dec!(105),
            dec!(98),
            dec!(103),
            dec!(97),
            dec!(104),
            dec!(99),
        ];
        let volumes = vec![dec!(10); 7];
        let signals = run(&mut strategy, &instrument, &prices, &volumes);
        assert!(signals.iter().all(|s| s.is_none()));
    }

    #[test]
    fn trend_without_volume_confirmation_emits_no_signal() {
        let mut strategy = QuantMomentumStrategy::new(
            StrategyId::new("qm-test").unwrap(),
            QuantMomentumParams {
                regression_period: 5,
                min_r_squared: 0.9,
                relative_volume_period: 5,
                min_relative_volume: 2.0, // exige o dobro do volume médio
                ..QuantMomentumParams::default()
            },
        );
        let instrument = instrument();
        let prices: Vec<Decimal> = (0..6).map(|i| dec!(100) + Decimal::from(i * 2)).collect();
        let volumes = vec![dec!(10); 6]; // volume constante -> relative_volume = 1.0, abaixo de 2.0
        let signals = run(&mut strategy, &instrument, &prices, &volumes);
        assert!(
            signals.iter().all(|s| s.is_none()),
            "a clean trend without above-average volume must not trigger an entry"
        );
    }

    /// Requisito 2 do audit: a estratégia precisa fechar sua própria
    /// posição sozinha. Constrói uma tendência de alta limpa (entrada
    /// Long), depois um platô (preço constante) que faz `slope` cair a
    /// zero e `r_squared` cair — a tendência que justificou a entrada
    /// deixou de existir — e confirma que a PRÓPRIA estratégia emite
    /// `Flat`, sem qualquer outra estratégia/sinal Short envolvido.
    #[test]
    fn emits_its_own_flat_exit_once_the_trend_weakens() {
        let mut strategy = QuantMomentumStrategy::new(
            StrategyId::new("qm-test").unwrap(),
            QuantMomentumParams {
                regression_period: 5,
                min_r_squared: 0.9,
                exit_r_squared: 0.3,
                relative_volume_period: 5,
                min_relative_volume: 0.0,
            },
        );
        let instrument = instrument();
        let mut prices: Vec<Decimal> = (0..6).map(|i| dec!(100) + Decimal::from(i * 2)).collect();
        // Platô: preço para de se mover -> slope cai a ~0 e r_squared
        // desmorona assim que candles do platô dominam a janela.
        for _ in 0..6 {
            prices.push(*prices.last().unwrap());
        }
        let volumes = vec![dec!(10); prices.len()];

        let signals = run(&mut strategy, &instrument, &prices, &volumes);
        let entry_index = signals
            .iter()
            .position(|s| matches!(s.as_ref().map(|s| s.direction), Some(SignalDirection::Long)))
            .expect("expected a long entry to have fired");
        let exit_index = signals[entry_index + 1..]
            .iter()
            .position(|s| matches!(s.as_ref().map(|s| s.direction), Some(SignalDirection::Flat)))
            .map(|i| i + entry_index + 1)
            .expect("expected the strategy to emit its own Flat exit once the trend weakens");

        assert!(exit_index > entry_index);
    }

    /// Requisito extra do audit (parte 2 — elimina posições fantasma): se o
    /// Risk Engine rejeitar a entrada (aqui simulado por nunca atualizar o
    /// `positions` que a estratégia consulta), ela não pode ficar "presa"
    /// acreditando estar posicionada — deve continuar apta a detectar uma
    /// entrada válida em candles futuros.
    #[test]
    fn rejected_entry_leaves_the_strategy_able_to_detect_a_new_valid_entry_later() {
        let mut strategy = QuantMomentumStrategy::new(
            StrategyId::new("qm-test").unwrap(),
            QuantMomentumParams {
                regression_period: 5,
                min_r_squared: 0.9,
                relative_volume_period: 5,
                min_relative_volume: 0.0,
                ..QuantMomentumParams::default()
            },
        );
        let instrument = instrument();
        // Reta perfeita e longa o bastante para a condição de entrada
        // continuar satisfeita em mais de um candle consecutivo, já que a
        // janela de regressão desliza mas o R² permanece 1.0 candle após
        // candle.
        let prices: Vec<Decimal> = (0..8).map(|i| dec!(100) + Decimal::from(i * 2)).collect();
        let volumes = vec![dec!(10); prices.len()];

        let mut engine = features::FeatureEngine::new(instrument.id, strategy.feature_config());
        // Nunca atualizado: simula o Risk Engine rejeitando toda entrada
        // emitida (ex.: saldo insuficiente, limite de exposição).
        let positions: HashMap<InstrumentId, Side> = HashMap::new();

        let mut entries = 0;
        for (i, (p, v)) in prices.iter().zip(volumes.iter()).enumerate() {
            let c = candle(&instrument, *p, *v, i as i64);
            let Some(snapshot) = engine.update(&c) else {
                continue;
            };
            if let Some(signal) = strategy.on_features(&instrument, &snapshot, &positions) {
                assert_eq!(
                    signal.direction,
                    SignalDirection::Long,
                    "only long entries are expected in this clean uptrend"
                );
                entries += 1;
            }
        }

        assert!(
            entries >= 2,
            "a rejected entry must not block the strategy from re-detecting a valid entry on a \
             later candle — expected at least 2 entry signals, got {entries}"
        );
    }

    /// Requisito 3 do audit: confiança de entrada varia com a força real
    /// do sinal (aqui, R²) e nunca sai de `[0, 1]` — não é uma
    /// probabilidade fixa nem um valor binário.
    #[test]
    fn entry_confidence_equals_r_squared_and_stays_in_unit_range() {
        let mut strategy = QuantMomentumStrategy::new(
            StrategyId::new("qm-test").unwrap(),
            QuantMomentumParams {
                regression_period: 5,
                min_r_squared: 0.5,
                relative_volume_period: 5,
                min_relative_volume: 0.0,
                ..QuantMomentumParams::default()
            },
        );
        let instrument = instrument();
        // Tendência de alta com ruído (não uma reta perfeita) -> R² entre
        // 0 e 1, não saturado em 1.0.
        let prices: Vec<Decimal> = vec![
            dec!(100),
            dec!(103),
            dec!(101),
            dec!(106),
            dec!(104),
            dec!(109),
        ];
        let volumes = vec![dec!(10); prices.len()];
        let signals = run(&mut strategy, &instrument, &prices, &volumes);
        let entry = signals
            .iter()
            .find_map(|s| s.clone())
            .expect("expected an entry signal");
        assert!((0.0..=1.0).contains(&entry.confidence));
        assert!(
            (entry.confidence - entry.confidence.clamp(0.0, 1.0)).abs() < 1e-12,
            "confidence must already be exactly r_squared, no extra scaling applied"
        );
    }

    /// Garante que nenhum candle futuro pode alterar uma decisão já
    /// tomada: roda um prefixo de candles isoladamente e roda de novo o
    /// mesmo prefixo seguido de candles adicionais (com uma instância nova
    /// da estratégia, mesmos parâmetros) — a decisão no índice compartilhado
    /// precisa ser idêntica nas duas execuções. Compara direção e
    /// confiança, não o `Signal` inteiro: cada `Signal::new` sorteia um
    /// `SignalId` novo, então dois sinais "iguais" em tudo que importa para
    /// a decisão nunca seriam `==` por causa só do id aleatório.
    #[test]
    fn no_look_ahead_decision_at_shared_index_is_unaffected_by_future_candles() {
        let params = QuantMomentumParams {
            regression_period: 5,
            min_r_squared: 0.5,
            relative_volume_period: 5,
            min_relative_volume: 0.0,
            ..QuantMomentumParams::default()
        };
        let instrument = instrument();
        let prices: Vec<Decimal> = vec![
            dec!(100),
            dec!(102),
            dec!(99),
            dec!(105),
            dec!(101),
            dec!(150), // continuação só na execução "longa"
            dec!(90),
        ];
        let volumes = vec![dec!(10); prices.len()];

        let mut short_strategy =
            QuantMomentumStrategy::new(StrategyId::new("qm-test").unwrap(), params);
        let short = run(
            &mut short_strategy,
            &instrument,
            &prices[..5],
            &volumes[..5],
        );

        let mut full_strategy =
            QuantMomentumStrategy::new(StrategyId::new("qm-test").unwrap(), params);
        let full = run(&mut full_strategy, &instrument, &prices, &volumes);

        let as_comparable = |s: &Option<Signal>| s.as_ref().map(|s| (s.direction, s.confidence));
        assert_eq!(as_comparable(&short[4]), as_comparable(&full[4]));
    }
}
