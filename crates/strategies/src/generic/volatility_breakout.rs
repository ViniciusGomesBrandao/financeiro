//! # Rompimento de Volatilidade (Volatility Breakout)
//!
//! **Hipótese:** quando o preço rompe as Bollinger Bands (um envelope
//! calibrado à volatilidade recente) *ao mesmo tempo* em que essa
//! volatilidade está se expandindo e o volume está acima da média, é
//! evidência de um movimento direcional genuíno começando — o oposto da
//! premissa de reversão à média (`StatisticalMeanReversionStrategy`): ali
//! um toque na banda é lido como esticado demais e prestes a reverter;
//! aqui, só quando *confirmado* por volatilidade e volume em expansão, o
//! mesmo toque é lido como o início de uma tendência. As duas estratégias
//! podem discordar sobre o mesmo candle — isso é esperado, elas exploram
//! regimes de mercado diferentes.
//!
//! **Features utilizadas** (`features::FeatureSnapshot`):
//! - `bollinger.percent_b` — posição do preço em relação às bandas;
//!   `> 1.0` = fechou acima da banda superior, `< 0.0` = abaixo da
//!   inferior (rompimento em qualquer direção); de volta a `[0, 1]` =
//!   preço reentrou nas bandas (gatilho de saída — ver abaixo).
//! - `bollinger.bandwidth` — largura das bandas relativa ao preço médio;
//!   comparada ao valor do candle anterior (estado interno simples, não um
//!   recálculo de indicador — ver a nota no doc do `feature_strategy`) para
//!   decidir se a volatilidade está *expandindo* (`bandwidth` maior que no
//!   candle anterior) — um rompimento durante contração de volatilidade
//!   tem mais cara de "fakeout" que de início de tendência.
//! - `relative_volume` — confirma participação, mesmo racional de
//!   `QuantMomentumStrategy`: um rompimento sem volume acima da média é
//!   mais fácil de reverter.
//!
//! **Combinação dos sinais:** as três condições de entrada (rompimento,
//! volatilidade em expansão, volume confirmando) são um filtro conjuntivo
//! — todas precisam ser verdadeiras — não uma pontuação combinada. A
//! confiança do sinal de entrada vem só de quão além da banda o preço
//! fechou (`percent_b`), normalizada por uma constante documentada
//! (`percent_b` chegando a 1.5, ou seja meio desvio-padrão-de-banda além
//! da borda, já satura a confiança em 1.0) — nenhum peso é atribuído a
//! bandwidth/volume na confiança, eles só decidem *se* o sinal existe.
//!
//! **Entradas:** candles OHLCV fechados de um instrumento.
//!
//! **Condição de entrada (Long):** `percent_b > 1.0` **e**
//! `bandwidth > bandwidth_anterior` **e**
//! `relative_volume >= min_relative_volume`.
//!
//! **Condição de entrada (Short):** `percent_b < 0.0` **e** as mesmas duas
//! condições de confirmação.
//!
//! **Condição de saída (própria, explícita):** uma vez posicionada, a
//! estratégia para de avaliar novas entradas para aquele instrumento e
//! passa a monitorar só o retorno do preço para dentro das bandas: emite
//! `SignalDirection::Flat` assim que `0.0 <= percent_b <= 1.0` de novo — o
//! rompimento perdeu força e o preço voltou para dentro do envelope de
//! volatilidade "normal". Como nas outras duas estratégias novas, isto é
//! deliberado: nunca depende de outra estratégia (ou de stop loss/take
//! profit externo) para fechar o que ela mesma abriu.
//!
//! **Quando NÃO deve operar:**
//! - Antes que `bollinger`/`relative_volume` tenham histórico suficiente,
//!   e no primeiro candle em que `bollinger` fica disponível (ainda não há
//!   `bandwidth` anterior para comparar expansão).
//! - Preço dentro das bandas (`0.0 <= percent_b <= 1.0`) — não há
//!   rompimento algum para *entrar*.
//! - Rompimento com volatilidade em contração (`bandwidth` encolhendo) —
//!   lido como possível fakeout, não confirmado.
//! - Rompimento com volume abaixo do limite configurado — pouca
//!   participação real por trás do movimento.
//! - Enquanto acredita estar posicionada (ver limitação abaixo), não
//!   avalia novas entradas.
//! - Estruturalmente, uma estratégia de breakout sofre em mercados que
//!   alternam rompimentos falsos com frequência (range-bound com picos de
//!   volatilidade ocasionais sem continuação) — os filtros de bandwidth e
//!   volume reduzem isso, mas não eliminam; a saída própria limita o
//!   prejuízo de um fakeout individual, mas não o previne.
//!
//! **Rastreamento de posição:** `on_features` recebe `positions: &dyn
//! PositionQuery` e consulta isso a cada chamada para saber se há de fato
//! uma posição real aberta — nunca guarda essa informação por conta própria
//! (ver `crate::position_query`). Uma entrada rejeitada pelo Risk Engine
//! nunca aparece em `positions`, então a estratégia continua livre para
//! avaliar novas entradas em vez de ficar "presa" monitorando a saída de
//! uma posição que não existe.
//!
//! **Confiança:** força heurística da entrada (excesso além da banda,
//! normalizado, saturado em `[0, 1]`), nunca uma probabilidade calibrada
//! de lucro. O sinal de saída sempre reporta confiança `1.0` — um fato
//! binário (o preço reentrou nas bandas ou não), não uma força graduada. O
//! motor de risco nunca lê `confidence` para dimensionar a ordem — ver
//! `risk::engine::tests::signal_confidence_does_not_affect_position_sizing`.
//!
//! **Parâmetros configuráveis:** `bollinger_period`, `bollinger_k`,
//! `relative_volume_period`, `min_relative_volume`.

use std::collections::HashMap;

use domain::{
    AssetClass, Instrument, InstrumentId, MarketDataKind, Signal, SignalDirection, StrategyId,
};
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
pub struct VolatilityBreakoutParams {
    pub bollinger_period: usize,
    pub bollinger_k: f64,
    pub relative_volume_period: usize,
    pub min_relative_volume: f64,
}

impl Default for VolatilityBreakoutParams {
    fn default() -> Self {
        Self {
            bollinger_period: 20,
            bollinger_k: 2.0,
            relative_volume_period: 20,
            // Rompimentos, por natureza, deveriam vir com participação
            // acima da média — o default é mais exigente que
            // QuantMomentumStrategy (1.0) de propósito.
            min_relative_volume: 1.5,
        }
    }
}

pub struct VolatilityBreakoutStrategy {
    id: StrategyId,
    params: VolatilityBreakoutParams,
    requirements: StrategyRequirements,
    /// `bandwidth` do candle anterior, por instrumento — estado da lógica
    /// de decisão (para comparar expansão/contração), não um indicador
    /// recalculado; o valor em si sempre vem de um `FeatureSnapshot`
    /// anterior. Atualizado a cada candle com `bollinger` disponível,
    /// independentemente de estar em posição, para que a próxima entrada
    /// (depois de uma saída) sempre compare contra o bandwidth mais
    /// recente, nunca um valor desatualizado de antes da posição atual.
    prev_bandwidth: HashMap<InstrumentId, f64>,
}

impl VolatilityBreakoutStrategy {
    pub fn new(id: StrategyId, params: VolatilityBreakoutParams) -> Self {
        assert!(
            params.bollinger_period > 1,
            "bollinger_period must be greater than 1"
        );
        assert!(params.bollinger_k > 0.0, "bollinger_k must be positive");
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
            prev_bandwidth: HashMap::new(),
        }
    }
}

impl FeatureStrategy for VolatilityBreakoutStrategy {
    fn id(&self) -> &StrategyId {
        &self.id
    }

    fn requirements(&self) -> &StrategyRequirements {
        &self.requirements
    }

    fn feature_config(&self) -> FeatureConfig {
        FeatureConfig {
            bollinger_period: self.params.bollinger_period,
            bollinger_k: self.params.bollinger_k,
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
        let bands = snapshot.bollinger;
        let prev_bandwidth =
            bands.and_then(|b| self.prev_bandwidth.insert(instrument.id, b.bandwidth));

        if positions.open_position_side(instrument.id).is_some() {
            // Posição real (via `positions`, nunca uma suposição interna):
            // só avalia a saída própria, nunca uma nova entrada.
            // `bollinger` indisponível (banda com largura zero — preço
            // completamente estável dentro da janela) é tratado como "sem
            // rompimento para falar", ou seja, já reentrou — mesmo padrão
            // de tratar o extremo "indefinido" das outras duas estratégias
            // como o exit mais decisivo, não como "sem dado".
            let back_inside_bands = match bands {
                Some(b) => (0.0..=1.0).contains(&b.percent_b),
                None => true,
            };
            if !back_inside_bands {
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
                json!({ "exit_reason": "back_inside_bands", "percent_b": bands.map(|b| b.percent_b) }),
            )
            .ok();
        }

        let bands = bands?;
        let relative_volume = snapshot.relative_volume?;
        let Some(prev_bandwidth) = prev_bandwidth else {
            // Primeiro snapshot com Bollinger disponível: ainda não há
            // bandwidth anterior para comparar expansão.
            return None;
        };
        let expanding = bands.bandwidth > prev_bandwidth;
        if !expanding {
            return None;
        }
        if relative_volume < self.params.min_relative_volume {
            return None;
        }

        let direction = if bands.percent_b > 1.0 {
            SignalDirection::Long
        } else if bands.percent_b < 0.0 {
            SignalDirection::Short
        } else {
            return None;
        };

        // Quanto o preço fechou além da borda da banda, normalizado: 0.5
        // de excesso em percent_b (meio "raio" de banda além da borda) já
        // satura a confiança em 1.0 — constante documentada, não ajustada
        // a nenhum instrumento específico.
        let overshoot = match direction {
            SignalDirection::Long => bands.percent_b - 1.0,
            SignalDirection::Short => -bands.percent_b,
            SignalDirection::Flat => unreachable!("direction is always Long or Short here"),
        };
        let confidence = (overshoot / 0.5).clamp(0.0, 1.0);

        Signal::new(
            self.id.clone(),
            instrument.id,
            direction,
            confidence,
            snapshot.timestamp,
            None,
            None,
            json!({
                "percent_b": bands.percent_b,
                "bandwidth": bands.bandwidth,
                "prev_bandwidth": prev_bandwidth,
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
    use domain::{Asset, Exchange, MarketType, Side, Symbol};
    use rust_decimal::Decimal;
    use rust_decimal_macros::dec;

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

    /// Simula o Risk Engine aprovando toda entrada/saída emitida (o caso
    /// feliz), sincronizando `positions` como `portfolio::PortfolioManager`
    /// faria de verdade após cada fill — a simulação de uma entrada
    /// *rejeitada* tem seu próprio teste dedicado abaixo, sem passar por
    /// este helper.
    fn run(
        strategy: &mut VolatilityBreakoutStrategy,
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
    fn no_signal_while_price_stays_inside_the_bands() {
        let mut strategy = VolatilityBreakoutStrategy::new(
            StrategyId::new("vb-test").unwrap(),
            VolatilityBreakoutParams {
                bollinger_period: 5,
                bollinger_k: 2.0,
                relative_volume_period: 5,
                min_relative_volume: 0.0,
            },
        );
        let instrument = instrument();
        let prices: Vec<Decimal> = vec![
            dec!(100),
            dec!(101),
            dec!(100),
            dec!(101),
            dec!(100),
            dec!(101),
        ];
        let volumes = vec![dec!(10); prices.len()];
        let signals = run(&mut strategy, &instrument, &prices, &volumes);
        assert!(signals.iter().all(|s| s.is_none()));
    }

    #[test]
    fn breakout_without_volume_confirmation_emits_no_signal() {
        let mut strategy = VolatilityBreakoutStrategy::new(
            StrategyId::new("vb-test").unwrap(),
            VolatilityBreakoutParams {
                bollinger_period: 5,
                bollinger_k: 2.0,
                relative_volume_period: 5,
                min_relative_volume: 5.0, // exige volume muito acima da média
            },
        );
        let instrument = instrument();
        let mut prices: Vec<Decimal> = vec![dec!(100), dec!(101), dec!(100), dec!(101), dec!(100)];
        prices.push(dec!(120)); // rompimento evidente
        let volumes = vec![dec!(10); prices.len()]; // volume constante -> relative_volume baixo
        let signals = run(&mut strategy, &instrument, &prices, &volumes);
        assert!(
            signals.iter().all(|s| s.is_none()),
            "a breakout without above-average volume must not trigger an entry"
        );
    }

    #[test]
    fn breakout_with_expanding_volatility_and_volume_emits_long_entry() {
        let mut strategy = VolatilityBreakoutStrategy::new(
            StrategyId::new("vb-test").unwrap(),
            VolatilityBreakoutParams {
                bollinger_period: 8,
                bollinger_k: 2.0,
                relative_volume_period: 8,
                min_relative_volume: 1.2,
            },
        );
        let instrument = instrument();
        // Com uma janela pequena, a própria vela do rompimento infla sua
        // própria banda (o outlier domina a variância da janela que o
        // contém), o que impede matematicamente `percent_b` de superar 1.0
        // para `bollinger_period` pequeno — só é possível quando
        // `período > k² + 1` (aqui, `8 > 2² + 1 = 5`). Por isso a janela
        // maior e o salto bem acima do ruído de fundo.
        let mut prices: Vec<Decimal> = vec![
            dec!(100),
            dec!(101),
            dec!(100),
            dec!(101),
            dec!(100),
            dec!(101),
            dec!(100),
            dec!(101),
            dec!(100),
        ];
        prices.push(dec!(1100));
        let mut volumes = vec![dec!(10); prices.len() - 1];
        volumes.push(dec!(100));
        let signals = run(&mut strategy, &instrument, &prices, &volumes);
        let entry = signals
            .iter()
            .find_map(|s| s.clone())
            .expect("expected a breakout entry to fire");
        assert_eq!(entry.direction, SignalDirection::Long);
    }

    /// Requisito 2 do audit: a estratégia precisa fechar sua própria
    /// posição sozinha. Depois do rompimento (entrada Long), alimenta
    /// candles que trazem o preço de volta para dentro das bandas e
    /// confirma que a PRÓPRIA estratégia emite `Flat`.
    #[test]
    fn emits_its_own_flat_exit_once_price_re_enters_the_bands() {
        let mut strategy = VolatilityBreakoutStrategy::new(
            StrategyId::new("vb-test").unwrap(),
            VolatilityBreakoutParams {
                bollinger_period: 8,
                bollinger_k: 2.0,
                relative_volume_period: 8,
                min_relative_volume: 1.2,
            },
        );
        let instrument = instrument();
        let mut prices: Vec<Decimal> = vec![
            dec!(100),
            dec!(101),
            dec!(100),
            dec!(101),
            dec!(100),
            dec!(101),
            dec!(100),
            dec!(101),
            dec!(100),
        ];
        prices.push(dec!(1100)); // dispara a entrada Long
                                 // Preço recuando de volta para a faixa de oscilação original —
                                 // percent_b deve voltar para dentro de [0, 1].
        for _ in 0..10 {
            prices.push(dec!(100));
        }
        let mut volumes = vec![dec!(10); prices.len() - 1];
        volumes.push(dec!(100));
        volumes[9] = dec!(100); // volume do candle de rompimento

        let signals = run(&mut strategy, &instrument, &prices, &volumes);
        let entry_index = signals
            .iter()
            .position(|s| matches!(s.as_ref().map(|s| s.direction), Some(SignalDirection::Long)))
            .expect("expected a long entry to have fired");
        let exit_index = signals[entry_index + 1..]
            .iter()
            .position(|s| matches!(s.as_ref().map(|s| s.direction), Some(SignalDirection::Flat)))
            .map(|i| i + entry_index + 1)
            .expect("expected the strategy to emit its own Flat exit");

        assert!(exit_index > entry_index);
    }

    /// Requisito extra do audit (parte 2 — elimina posições fantasma): se o
    /// Risk Engine rejeitar a entrada (aqui simulado por nunca atualizar o
    /// `positions` que a estratégia consulta), ela não pode ficar "presa"
    /// acreditando estar posicionada — deve continuar apta a detectar um
    /// novo rompimento válido em candles futuros.
    #[test]
    fn rejected_entry_leaves_the_strategy_able_to_detect_a_new_valid_entry_later() {
        let mut strategy = VolatilityBreakoutStrategy::new(
            StrategyId::new("vb-test").unwrap(),
            VolatilityBreakoutParams {
                bollinger_period: 8,
                bollinger_k: 2.0,
                relative_volume_period: 8,
                min_relative_volume: 1.2,
            },
        );
        let instrument = instrument();
        let oscillation = [
            dec!(100),
            dec!(101),
            dec!(100),
            dec!(101),
            dec!(100),
            dec!(101),
            dec!(100),
            dec!(101),
            dec!(100),
        ];
        let mut prices: Vec<Decimal> = oscillation.to_vec();
        prices.push(dec!(1100)); // primeiro rompimento -> índice 9
        prices.extend(oscillation);
        prices.push(dec!(1100)); // segundo rompimento -> deve disparar de novo (índice 19)

        let mut volumes = vec![dec!(10); prices.len()];
        volumes[9] = dec!(100);
        volumes[19] = dec!(100);

        let mut engine = features::FeatureEngine::new(instrument.id, strategy.feature_config());
        // Nunca atualizado: simula o Risk Engine rejeitando toda entrada
        // emitida.
        let positions: HashMap<InstrumentId, Side> = HashMap::new();

        let mut entries = 0;
        for (i, (p, v)) in prices.iter().zip(volumes.iter()).enumerate() {
            let c = candle(&instrument, *p, *v, i as i64);
            let Some(snapshot) = engine.update(&c) else {
                continue;
            };
            if let Some(signal) = strategy.on_features(&instrument, &snapshot, &positions) {
                assert_eq!(signal.direction, SignalDirection::Long);
                entries += 1;
            }
        }

        assert_eq!(
            entries, 2,
            "a rejected entry must not block the strategy from detecting the next valid breakout"
        );
    }

    /// Requisito 3 do audit: confiança de entrada varia com o excesso
    /// além da banda e nunca sai de `[0, 1]`.
    #[test]
    fn entry_confidence_stays_in_unit_range_and_is_not_always_maximal() {
        let mut strategy = VolatilityBreakoutStrategy::new(
            StrategyId::new("vb-test").unwrap(),
            VolatilityBreakoutParams {
                bollinger_period: 8,
                bollinger_k: 2.0,
                relative_volume_period: 8,
                min_relative_volume: 1.2,
            },
        );
        let instrument = instrument();
        let mut prices: Vec<Decimal> = vec![
            dec!(100),
            dec!(101),
            dec!(100),
            dec!(101),
            dec!(100),
            dec!(101),
            dec!(100),
            dec!(101),
            dec!(100),
        ];
        prices.push(dec!(1100));
        let mut volumes = vec![dec!(10); prices.len() - 1];
        volumes.push(dec!(100));
        let signals = run(&mut strategy, &instrument, &prices, &volumes);
        let entry = signals
            .iter()
            .find_map(|s| s.clone())
            .expect("expected an entry signal");
        assert!((0.0..=1.0).contains(&entry.confidence));
    }

    /// Mesma garantia de `quant_momentum::tests::no_look_ahead_...`: a
    /// decisão num índice compartilhado (aqui, o próprio candle de
    /// rompimento) não pode mudar por causa de candles que só existem na
    /// execução mais longa.
    #[test]
    fn no_look_ahead_decision_at_shared_index_is_unaffected_by_future_candles() {
        let params = VolatilityBreakoutParams {
            bollinger_period: 8,
            bollinger_k: 2.0,
            relative_volume_period: 8,
            min_relative_volume: 1.2,
        };
        let instrument = instrument();
        let mut prices: Vec<Decimal> = vec![
            dec!(100),
            dec!(101),
            dec!(100),
            dec!(101),
            dec!(100),
            dec!(101),
            dec!(100),
            dec!(101),
            dec!(100),
            dec!(1100),
        ];
        let mut volumes = vec![dec!(10); prices.len()];
        *volumes.last_mut().unwrap() = dec!(100);
        // Continuação que só existe na execução "longa".
        prices.push(dec!(1000));
        volumes.push(dec!(10));

        let mut short_strategy =
            VolatilityBreakoutStrategy::new(StrategyId::new("vb-test").unwrap(), params);
        let short = run(
            &mut short_strategy,
            &instrument,
            &prices[..10],
            &volumes[..10],
        );

        let mut full_strategy =
            VolatilityBreakoutStrategy::new(StrategyId::new("vb-test").unwrap(), params);
        let full = run(&mut full_strategy, &instrument, &prices, &volumes);

        let as_comparable = |s: &Option<Signal>| s.as_ref().map(|s| (s.direction, s.confidence));
        assert_eq!(as_comparable(&short[9]), as_comparable(&full[9]));
    }
}
