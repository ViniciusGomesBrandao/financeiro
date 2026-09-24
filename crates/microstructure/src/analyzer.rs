//! Mede o poder preditivo de cada feature de microestrutura sobre o
//! retorno futuro do mid-price, em vários horizontes — **pesquisa, não
//! estratégia**: não seleciona parâmetros, não decide entrar/sair de
//! nada, só reporta o que os dados já mostram. Opera sobre a grade
//! determinística de 1s (`grid::snapshot_grid`, persistida via
//! `store::FeatureGridRow`), o que torna "retorno em N segundos" uma
//! busca por timestamp exato em vez de reamostragem.
//!
//! Cada feature entra numa de duas categorias:
//! - **Direcional** (`bid_ask_imbalance`, `book_imbalance`,
//!   `trade_imbalance`, `volume_delta`, `order_flow_imbalance`,
//!   `microprice_deviation`): tem um sinal com significado (positivo =
//!   pressão compradora esperada) — só essas entram no hit rate
//!   direcional.
//! - **De magnitude** (`spread_abs`, `spread_pct`, `microprice`,
//!   `trade_intensity`): sempre `>= 0` (ou, no caso de `microprice`, um
//!   nível de preço sem sinal direcional próprio), sem "direção
//!   prevista" — reportadas com correlação/decis, hit rate fica `None`
//!   com o motivo explícito.
//!
//! `microprice_deviation = microprice - mid_price` é a única feature
//! **derivada** aqui (não persistida em `FeatureGridRow`): o nível bruto
//! do microprice se move junto com o mid_price (é outro preço, não um
//! sinal), então testá-lo isoladamente contra retorno futuro mede a
//! mesma coisa que testar o próprio mid_price — pouco informativo. O
//! desvio em relação ao mid é a leitura padrão de uso do microprice como
//! sinal (a mesma lógica documentada em `engine::microprice`). Incluída
//! ao lado das 9 features cruas, claramente rotulada como derivada, para
//! não pular a checagem mais relevante do microprice sem deixar de
//! também reportar o campo bruto tal como ele existe.

use std::collections::HashMap;

use serde::Serialize;

use crate::store::FeatureGridRow;

/// Mesmos defaults já estabelecidos em outras partes do projeto
/// (`app::config::AppConfig::from_env`, `experiments::matrix`) — não
/// escolhidos para esta análise, reaproveitados por consistência.
pub const PROJECT_TAKER_FEE: f64 = 0.001;
pub const PROJECT_SLIPPAGE_BPS: f64 = 3.0;

/// Horizontes padrão pedidos: 1s, 5s, 15s, 30s, 60s, 5min.
pub const DEFAULT_HORIZONS_SECS: [i64; 6] = [1, 5, 15, 30, 60, 300];

/// Amostra mínima abaixo da qual as estatísticas ainda são calculadas
/// (nunca escondidas), mas `n` pequeno já avisa o leitor a não confiar
/// nelas — mesmo limiar documentado (não ajustado a nenhum resultado)
/// já usado em `experiments::diagnostic::LOW_SAMPLE_THRESHOLD`.
pub const LOW_SAMPLE_THRESHOLD: usize = 30;

type Extractor = fn(&FeatureGridRow) -> Option<f64>;

const FEATURES: &[(&str, bool, Extractor)] = &[
    ("spread_abs", false, |r| r.spread_abs),
    ("spread_pct", false, |r| r.spread_pct),
    ("microprice", false, |r| r.microprice),
    ("microprice_deviation", true, |r| {
        match (r.microprice, r.mid_price) {
            (Some(mp), Some(m)) => Some(mp - m),
            _ => None,
        }
    }),
    ("bid_ask_imbalance", true, |r| r.bid_ask_imbalance),
    ("book_imbalance", true, |r| r.book_imbalance),
    ("trade_imbalance", true, |r| r.trade_imbalance),
    ("volume_delta", true, |r| r.volume_delta),
    ("trade_intensity", false, |r| r.trade_intensity),
    ("order_flow_imbalance", true, |r| r.order_flow_imbalance),
];

pub fn feature_names() -> Vec<&'static str> {
    FEATURES.iter().map(|(name, _, _)| *name).collect()
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FeatureHorizonStats {
    pub feature: &'static str,
    pub is_directional: bool,
    pub horizon_secs: i64,
    pub n: usize,
    /// Tamanho efetivo da amostra, descontando a dependência introduzida
    /// por janelas de retorno sobrepostas (ver `effective_sample_size`).
    /// **Sempre `<= n`**, igual a `n` só quando `horizon_secs ==
    /// GRID_INTERVAL_SECS` (sem sobreposição nenhuma entre janelas
    /// consecutivas).
    pub n_effective: usize,
    pub pearson_correlation: Option<f64>,
    /// Intervalo de confiança de 95% para `pearson_correlation`, via
    /// bootstrap por blocos móveis (não IID) — ver
    /// `block_bootstrap_pearson_ci`. `None` quando a amostra é pequena
    /// demais para o bootstrap rodar de forma confiável.
    pub pearson_ci95_lo: Option<f64>,
    pub pearson_ci95_hi: Option<f64>,
    pub spearman_correlation: Option<f64>,
    /// Retorno médio futuro por decil do valor da feature, do menor (0)
    /// ao maior (9) — `None` se `n` não for suficiente para 10 grupos
    /// não-vazios.
    pub decile_mean_returns: Option<[f64; 10]>,
    /// `decile_mean_returns[9] - decile_mean_returns[0]` — o "espalhamento"
    /// de retorno entre os extremos da feature; é o "movimento esperado"
    /// comparado a `round_trip_cost_pct`.
    pub decile_edge_spread: Option<f64>,
    /// Fração de amostras onde `sign(feature) == sign(retorno futuro)`
    /// (zeros de qualquer lado excluídos do numerador/denominador) —
    /// `None` para features de magnitude (sem direção prevista).
    pub hit_rate: Option<f64>,
    pub first_half_correlation: Option<f64>,
    pub second_half_correlation: Option<f64>,
    /// `true` se a correlação tem o mesmo sinal nas duas metades da
    /// amostra (e nenhuma das duas é exatamente zero) — um efeito que
    /// muda de sinal entre a primeira e a segunda metade da amostra é
    /// instável/provavelmente ruído, não um sinal persistente.
    pub stable_sign: Option<bool>,
    pub round_trip_cost_pct: f64,
    /// `|decile_edge_spread| > round_trip_cost_pct` — se o movimento
    /// esperado nem cobre o custo estimado de ida e volta, o sinal não é
    /// operável mesmo que estatisticamente real.
    pub edge_exceeds_cost: Option<bool>,
}

fn mean(values: &[f64]) -> f64 {
    values.iter().sum::<f64>() / values.len() as f64
}

fn pearson(xs: &[f64], ys: &[f64]) -> Option<f64> {
    let n = xs.len();
    if n < 2 {
        return None;
    }
    let mx = mean(xs);
    let my = mean(ys);
    let mut cov = 0.0;
    let mut vx = 0.0;
    let mut vy = 0.0;
    for i in 0..n {
        let dx = xs[i] - mx;
        let dy = ys[i] - my;
        cov += dx * dy;
        vx += dx * dx;
        vy += dy * dy;
    }
    if vx == 0.0 || vy == 0.0 {
        return None;
    }
    Some(cov / (vx.sqrt() * vy.sqrt()))
}

/// Ranks com média nas posições empatadas (o método padrão para
/// correlação de Spearman com valores repetidos).
fn ranks(values: &[f64]) -> Vec<f64> {
    let n = values.len();
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|&a, &b| values[a].partial_cmp(&values[b]).unwrap());

    let mut result = vec![0.0; n];
    let mut i = 0;
    while i < n {
        let mut j = i;
        while j + 1 < n && values[order[j + 1]] == values[order[i]] {
            j += 1;
        }
        // posições i..=j (0-based) empatadas -> rank médio (1-based)
        let avg_rank = ((i + 1) + (j + 1)) as f64 / 2.0;
        for slot in order.iter().take(j + 1).skip(i) {
            result[*slot] = avg_rank;
        }
        i = j + 1;
    }
    result
}

fn spearman(xs: &[f64], ys: &[f64]) -> Option<f64> {
    if xs.len() < 2 {
        return None;
    }
    pearson(&ranks(xs), &ranks(ys))
}

fn deciles(xs: &[f64], ys: &[f64]) -> Option<([f64; 10], f64)> {
    let n = xs.len();
    if n < 10 {
        return None;
    }
    let mut pairs: Vec<(f64, f64)> = xs.iter().copied().zip(ys.iter().copied()).collect();
    pairs.sort_by(|a, b| a.0.partial_cmp(&b.0).unwrap());

    // Grupos contíguos de tamanho `n/10` (+/- 1) — `d*n/10 .. (d+1)*n/10`
    // cobre todo `n` em exatamente 10 grupos não-vazios sempre que
    // `n >= 10` (divisão inteira), diferente de `chunks(n.div_ceil(10))`,
    // que pode gerar menos de 10 grupos quando `n` não é múltiplo de 10.
    let mut means = [0.0; 10];
    for (d, slot) in means.iter_mut().enumerate() {
        let start = d * n / 10;
        let end = (d + 1) * n / 10;
        let returns: Vec<f64> = pairs[start..end].iter().map(|(_, y)| *y).collect();
        *slot = mean(&returns);
    }
    let spread = means[9] - means[0];
    Some((means, spread))
}

/// Espaçamento da grade que `build_samples`/`analyze` assumem (a mesma
/// que `grid::snapshot_grid` usa por default nesta fase — "inicialmente
/// em intervalos de 1s"). Fixo aqui, não descoberto a partir dos dados,
/// porque `FeatureGridRow` não carrega essa informação; se a grade um dia
/// rodar num espaçamento diferente, este valor precisa acompanhar.
const GRID_INTERVAL_SECS: i64 = 1;

/// Auditoria: "inferência estatística considera dependência causada por
/// horizontes sobrepostos" — janelas de retorno de horizonte `h`,
/// amostradas a cada `GRID_INTERVAL_SECS`, se sobrepõem como um processo
/// de médias móveis de ordem `h/intervalo - 1`: os retornos em `t` e
/// `t+intervalo` compartilham `h-intervalo` segundos da mesma janela.
/// `n` bruto conta cada ponto da grade como uma observação independente,
/// o que superestima a informação real disponível por um fator de
/// aproximadamente `h/intervalo` — a mesma ideia por trás da correção de
/// Richardson & Smith (1991) para estudos de retorno com janelas
/// sobrepostas. `n_effective = n / (h/intervalo)` é a aproximação mais
/// simples e conservadora dentro dessa família: **determinística**, não
/// estimada a partir de nenhuma autocorrelação observada (logo, não é
/// "tuning" — é uma função fixa de `horizon_secs`, conhecida antes de
/// olhar qualquer dado).
fn effective_sample_size(n: usize, horizon_secs: i64) -> usize {
    if n == 0 {
        return 0;
    }
    let steps_per_horizon = (horizon_secs / GRID_INTERVAL_SECS).max(1) as usize;
    (n / steps_per_horizon).max(1)
}

/// Gerador determinístico (xorshift32) — só para o bootstrap por blocos
/// abaixo; mesma seed sempre produz a mesma sequência, então o CI
/// reportado é reproduzível entre execuções, não um número que muda a
/// cada vez que o relatório é gerado.
struct DeterministicRng(u32);

impl DeterministicRng {
    fn new(seed: u32) -> Self {
        Self(seed.max(1))
    }

    fn next_unit(&mut self) -> f64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 17;
        x ^= x << 5;
        self.0 = x;
        (x as f64) / (u32::MAX as f64)
    }
}

fn percentile(sorted: &[f64], p: f64) -> f64 {
    let n = sorted.len();
    if n == 1 {
        return sorted[0];
    }
    let idx = p * (n - 1) as f64;
    let lo = idx.floor() as usize;
    let hi = idx.ceil() as usize;
    if lo == hi {
        sorted[lo]
    } else {
        sorted[lo] + (sorted[hi] - sorted[lo]) * (idx - lo as f64)
    }
}

/// Intervalo de confiança de 95% para a correlação de Pearson entre `xs`
/// e `ys`, via bootstrap por **blocos móveis** (não reamostragem IID
/// trade-a-trade) — cada réplica é montada colando blocos contíguos e
/// sobrepostos de comprimento `block_len`, sorteados com reposição até
/// completar `n` pontos, preservando a estrutura de dependência de médias
/// móveis descrita em `effective_sample_size` em vez de assumir
/// observações independentes (que subestimaria a variância real sempre
/// que `horizon_secs > GRID_INTERVAL_SECS`). `block_len` já vem calculado
/// pelo chamador (`2×` os passos de grade do horizonte, para cobrir a
/// dependência conhecida com folga) — mesmo padrão de bootstrap por
/// blocos já usado em `experiments` para séries de trades autocorrelacionadas.
fn block_bootstrap_pearson_ci(
    xs: &[f64],
    ys: &[f64],
    block_len: usize,
    replicates: usize,
    seed: u32,
) -> Option<(f64, f64)> {
    let n = xs.len();
    if n < 8 {
        return None;
    }
    let block_len = block_len.clamp(1, n);
    let mut rng = DeterministicRng::new(seed);
    let mut stats: Vec<f64> = Vec::with_capacity(replicates);

    for _ in 0..replicates {
        let mut rxs = Vec::with_capacity(n);
        let mut rys = Vec::with_capacity(n);
        while rxs.len() < n {
            let max_start = n - block_len;
            let start = ((rng.next_unit() * (max_start as f64 + 1.0)) as usize).min(max_start);
            for k in 0..block_len {
                if rxs.len() >= n {
                    break;
                }
                rxs.push(xs[start + k]);
                rys.push(ys[start + k]);
            }
        }
        if let Some(r) = pearson(&rxs, &rys) {
            stats.push(r);
        }
    }

    if stats.len() < replicates / 2 {
        return None;
    }
    stats.sort_by(|a, b| a.partial_cmp(b).unwrap());
    Some((percentile(&stats, 0.025), percentile(&stats, 0.975)))
}

/// Réplicas do bootstrap por blocos — fixo e documentado (não ajustado a
/// nenhum resultado): grande o bastante para percentis de cauda 2,5/97,5
/// estáveis, pequeno o bastante para rodar em lote sobre 10 features ×
/// 6 horizontes × N símbolos sem custo proibitivo.
const BOOTSTRAP_REPLICATES: usize = 1000;
/// Seed fixa — o CI reportado é reproduzível entre execuções do mesmo
/// dado, não um número que varia a cada vez que o relatório é gerado.
const BOOTSTRAP_SEED: u32 = 1337;

fn hit_rate(xs: &[f64], ys: &[f64]) -> Option<f64> {
    let mut hits = 0usize;
    let mut total = 0usize;
    for i in 0..xs.len() {
        if xs[i] == 0.0 || ys[i] == 0.0 {
            continue;
        }
        total += 1;
        if xs[i].signum() == ys[i].signum() {
            hits += 1;
        }
    }
    if total == 0 {
        None
    } else {
        Some(hits as f64 / total as f64)
    }
}

/// Custo estimado de ida e volta: `2×taxa_taker + spread_pct médio da
/// amostra + 2×slippage_bps`. O spread vem dos próprios dados (empírico,
/// não uma suposição externa); taxa e slippage reaproveitam os defaults
/// já estabelecidos no projeto (`PROJECT_TAKER_FEE`/`PROJECT_SLIPPAGE_BPS`).
///
/// **Auditoria do "×1" no spread, não "×2"**: taxa e slippage entram
/// dobradas porque incidem por perna (entrada e saída, cada uma paga sua
/// própria taxa/slippage). O spread é diferente: um round-trip a mercado
/// entra no ask (`mid + spread/2`) e sai no bid (`mid − spread/2`) — a
/// diferença entre as duas pernas já soma o spread **inteiro** uma única
/// vez (`(mid+spread/2) − (mid−spread/2) = spread`), não duas vezes.
/// Contar `2×spread_pct` aqui dobraria esse custo por engano. Verificado
/// com um exemplo numérico em
/// `round_trip_cost_matches_a_hand_worked_buy_at_ask_sell_at_bid_example`.
/// (Isso é diferente da convenção de `PaperBroker`/`experiments::matrix`,
/// que aplica um `spread_bps` *fixo e sintético* a cada perna
/// separadamente — apropriado lá porque não há book real para medir o
/// spread verdadeiro; aqui o spread vem de dado real, então a contagem
/// correta é uma vez, não duas.)
pub fn estimate_round_trip_cost_pct(rows: &[FeatureGridRow]) -> f64 {
    let spreads: Vec<f64> = rows.iter().filter_map(|r| r.spread_pct).collect();
    let avg_spread = if spreads.is_empty() {
        0.0
    } else {
        mean(&spreads)
    };
    2.0 * PROJECT_TAKER_FEE + avg_spread + 2.0 * (PROJECT_SLIPPAGE_BPS / 10000.0)
}

/// Constrói, para um horizonte, os pares (valor da feature em t, retorno
/// futuro do mid-price de t a t+horizonte) — casando por timestamp exato
/// (não por deslocamento de índice), então um buraco na grade só derruba
/// as amostras que o atravessam, sem desalinhar o resto.
///
/// **Garantia auditada**: `horizon_secs > 0` é obrigatório (`assert!`
/// abaixo, não um `debug_assert!` — este invariante importa em produção,
/// não só em teste) — isso garante que a chave buscada
/// (`row.timestamp_ms + horizon_ms`) nunca é igual à própria chave de
/// `row`, então a linha `j` usada para `m1` é sempre uma observação
/// estritamente posterior a `row` (nunca a mesma linha, nunca uma leitura
/// que já aconteceu). Testado explicitamente em
/// `future_row_is_always_strictly_later_than_the_feature_row`.
///
/// **Ressalva documentada, não uma falha**: o retorno usa `m0` (o
/// `mid_price` da própria linha `row`) como base — inevitável, é a
/// definição de "retorno a partir de T". Para features que **também**
/// embutem `mid_price` na própria fórmula (`microprice_deviation =
/// microprice - mid_price`; `spread_pct = spread_abs / mid_price`;
/// `microprice`, correlacionado com `mid_price` por construção, já que os
/// dois vêm do mesmo bid/ask), isso cria um canal de correlação puramente
/// mecânico (o efeito conhecido como "bid-ask bounce": ruído de medição em
/// `mid_price` aparece nos dois lados da conta) que **não é sinal
/// preditivo real** — ver o teste
/// `microprice_deviation_shows_mechanical_correlation_from_shared_mid_price_term`,
/// que demonstra o efeito isolado (sem nenhuma relação econômica real
/// entre as séries). Por isso essas três features pedem leitura mais
/// cética no relatório do que as que só usam *quantidades* (imbalances,
/// volume delta, trade intensity, OFI), que não compartilham nenhum termo
/// de preço com o retorno.
fn build_samples(
    rows: &[FeatureGridRow],
    extractor: Extractor,
    horizon_secs: i64,
) -> (Vec<f64>, Vec<f64>) {
    assert!(
        horizon_secs > 0,
        "horizon_secs must be strictly positive so the forward return always looks at a row \
         strictly later than the feature row, never the same row"
    );
    let by_timestamp: HashMap<i64, usize> = rows
        .iter()
        .enumerate()
        .map(|(i, r)| (r.timestamp_ms, i))
        .collect();
    let horizon_ms = horizon_secs * 1000;

    let mut xs = Vec::new();
    let mut ys = Vec::new();
    for row in rows {
        let (Some(x), Some(m0)) = (extractor(row), row.mid_price) else {
            continue;
        };
        let Some(&j) = by_timestamp.get(&(row.timestamp_ms + horizon_ms)) else {
            continue;
        };
        let Some(m1) = rows[j].mid_price else {
            continue;
        };
        if m0 == 0.0 {
            continue;
        }
        xs.push(x);
        ys.push((m1 - m0) / m0);
    }
    (xs, ys)
}

fn analyze_one(
    feature: &'static str,
    is_directional: bool,
    extractor: Extractor,
    horizon_secs: i64,
    rows: &[FeatureGridRow],
    round_trip_cost_pct: f64,
) -> FeatureHorizonStats {
    let (xs, ys) = build_samples(rows, extractor, horizon_secs);
    let n = xs.len();
    let n_effective = effective_sample_size(n, horizon_secs);

    let pearson_correlation = pearson(&xs, &ys);
    // Comprimento de bloco = 2x os passos de grade do horizonte (a
    // dependência de médias móveis dura exatamente `passos-1`; o dobro dá
    // folga para o bootstrap preservar essa estrutura mesmo com alguma
    // variação de fase entre réplicas), nunca menor que 2.
    let steps_per_horizon = (horizon_secs / GRID_INTERVAL_SECS).max(1) as usize;
    let block_len = (2 * steps_per_horizon).max(2);
    let (pearson_ci95_lo, pearson_ci95_hi) =
        match block_bootstrap_pearson_ci(&xs, &ys, block_len, BOOTSTRAP_REPLICATES, BOOTSTRAP_SEED)
        {
            Some((lo, hi)) => (Some(lo), Some(hi)),
            None => (None, None),
        };
    let spearman_correlation = spearman(&xs, &ys);
    let decile_result = deciles(&xs, &ys);
    let decile_mean_returns = decile_result.map(|(means, _)| means);
    let decile_edge_spread = decile_result.map(|(_, spread)| spread);
    let hit = if is_directional {
        hit_rate(&xs, &ys)
    } else {
        None
    };

    let mid = n / 2;
    let (first_half_correlation, second_half_correlation, stable_sign) = if mid >= 2 && n - mid >= 2
    {
        let fh = pearson(&xs[..mid], &ys[..mid]);
        let sh = pearson(&xs[mid..], &ys[mid..]);
        let stable = match (fh, sh) {
            (Some(a), Some(b)) if a != 0.0 && b != 0.0 => Some(a.signum() == b.signum()),
            _ => None,
        };
        (fh, sh, stable)
    } else {
        (None, None, None)
    };

    let edge_exceeds_cost = decile_edge_spread.map(|spread| spread.abs() > round_trip_cost_pct);

    FeatureHorizonStats {
        feature,
        is_directional,
        horizon_secs,
        n,
        n_effective,
        pearson_correlation,
        pearson_ci95_lo,
        pearson_ci95_hi,
        spearman_correlation,
        decile_mean_returns,
        decile_edge_spread,
        hit_rate: hit,
        first_half_correlation,
        second_half_correlation,
        stable_sign,
        round_trip_cost_pct,
        edge_exceeds_cost,
    }
}

/// Roda a análise completa: todas as features cadastradas em `FEATURES`
/// x todos os horizontes de `horizons_secs`, sobre `rows` (a grade de 1s
/// de um símbolo/dia — ou vários dias concatenados, desde que já
/// ordenados por `timestamp_ms`).
pub fn analyze(rows: &[FeatureGridRow], horizons_secs: &[i64]) -> Vec<FeatureHorizonStats> {
    let round_trip_cost_pct = estimate_round_trip_cost_pct(rows);
    let mut out = Vec::with_capacity(FEATURES.len() * horizons_secs.len());
    for &(feature, is_directional, extractor) in FEATURES {
        for &horizon_secs in horizons_secs {
            out.push(analyze_one(
                feature,
                is_directional,
                extractor,
                horizon_secs,
                rows,
                round_trip_cost_pct,
            ));
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(ts_ms: i64, mid: f64, imbalance: Option<f64>) -> FeatureGridRow {
        FeatureGridRow {
            timestamp_ms: ts_ms,
            spread_abs: Some(0.01),
            spread_pct: Some(0.0001),
            mid_price: Some(mid),
            microprice: Some(mid),
            bid_ask_imbalance: imbalance,
            book_imbalance: imbalance,
            trade_imbalance: imbalance,
            volume_delta: imbalance,
            trade_intensity: Some(1.0),
            order_flow_imbalance: imbalance,
        }
    }

    #[test]
    fn pearson_matches_hand_computation_for_perfectly_correlated_series() {
        let xs = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let ys = vec![2.0, 4.0, 6.0, 8.0, 10.0]; // y = 2x, correlação = 1
        assert!((pearson(&xs, &ys).unwrap() - 1.0).abs() < 1e-9);
    }

    #[test]
    fn pearson_is_negative_one_for_perfectly_inverted_series() {
        let xs = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let ys = vec![5.0, 4.0, 3.0, 2.0, 1.0];
        assert!((pearson(&xs, &ys).unwrap() - (-1.0)).abs() < 1e-9);
    }

    #[test]
    fn spearman_is_one_for_a_monotonic_but_nonlinear_relationship() {
        let xs = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let ys = vec![1.0, 4.0, 9.0, 16.0, 25.0]; // y = x^2: monotônico, não linear
        assert!((spearman(&xs, &ys).unwrap() - 1.0).abs() < 1e-9);
        // Pearson não deve ser exatamente 1 para essa relação não-linear.
        assert!(pearson(&xs, &ys).unwrap() < 1.0);
    }

    /// Regressão: uma primeira versão usava `chunks(n.div_ceil(10))`, que
    /// produz *menos* de 10 grupos quando `n` não é múltiplo de 10 (ex.:
    /// `n=45` -> chunk_size=5 -> só 9 grupos), deixando o décimo decil
    /// zerado por padrão em vez de calculado. A divisão `d*n/10..(d+1)*n/10`
    /// sempre produz exatamente 10 grupos não-vazios para `n >= 10`.
    #[test]
    fn deciles_always_produce_exactly_ten_non_empty_groups_even_when_n_is_not_a_multiple_of_ten() {
        let n = 45;
        let xs: Vec<f64> = (0..n).map(|i| i as f64).collect();
        let ys: Vec<f64> = (0..n).map(|i| i as f64).collect();
        let (means, spread) = deciles(&xs, &ys).unwrap();
        // Estritamente crescente: nenhum grupo ficou vazio/zerado por engano.
        for pair in means.windows(2) {
            assert!(pair[1] > pair[0], "means não crescente: {means:?}");
        }
        assert!(spread > 0.0);
    }

    #[test]
    fn ranks_average_tied_values() {
        let r = ranks(&[10.0, 20.0, 20.0, 30.0]);
        // 10 -> rank 1; os dois 20 empatam nas posições 2 e 3 -> rank médio 2.5; 30 -> rank 4.
        assert_eq!(r, vec![1.0, 2.5, 2.5, 4.0]);
    }

    #[test]
    fn hit_rate_matches_hand_count_and_excludes_zeros() {
        let xs = vec![1.0, -1.0, 1.0, 0.0, -1.0];
        let ys = vec![0.5, -0.5, -0.5, 1.0, -0.5];
        // linha 0: sinais iguais (hit); linha 1: iguais (hit); linha 2: diferentes (miss);
        // linha 3: x=0 excluída; linha 4: iguais (hit). 3 hits em 4 válidas = 0.75.
        assert_eq!(hit_rate(&xs, &ys), Some(0.75));
    }

    #[test]
    fn build_samples_matches_by_exact_timestamp_not_index_shift() {
        let rows = vec![
            row(0, 100.0, Some(0.5)),
            // buraco de 1s aqui (sem row em 1000ms)
            row(2000, 101.0, Some(0.3)),
            row(3000, 102.0, Some(0.1)),
        ];
        // horizonte de 1s: só a linha em 2000ms tem par em 3000ms.
        let (xs, ys) = build_samples(&rows, |r| r.bid_ask_imbalance, 1);
        assert_eq!(xs.len(), 1);
        assert_eq!(xs[0], 0.3);
        assert!((ys[0] - ((102.0 - 101.0) / 101.0)).abs() < 1e-12);
    }

    /// Auditoria: "retorno futuro começa estritamente após T, sem
    /// reutilizar preço usado na feature" — verifica diretamente que a
    /// linha usada para `m1` (o preço futuro) nunca é a própria linha
    /// `row` (mesmo timestamp) e está sempre estritamente depois dela,
    /// para todo horizonte testado. `build_samples` já teria essa
    /// garantia por construção (a chave buscada é `row.timestamp_ms +
    /// horizon_ms`, nunca igual a `row.timestamp_ms` já que
    /// `horizon_secs>0`), mas este teste prova isso observando o
    /// resultado, não só lendo o código.
    #[test]
    fn future_row_is_always_strictly_later_than_the_feature_row() {
        // Precisa cobrir com folga o maior horizonte testado (5min=300s),
        // senão o horizonte não gera amostra nenhuma e o teste não prova
        // nada para ele.
        let rows: Vec<FeatureGridRow> = (0..400)
            .map(|i| row(i * 1000, 100.0 + i as f64, Some((i % 3) as f64 - 1.0)))
            .collect();
        let by_ts: std::collections::HashMap<i64, usize> = rows
            .iter()
            .enumerate()
            .map(|(i, r)| (r.timestamp_ms, i))
            .collect();

        for &horizon in &DEFAULT_HORIZONS_SECS {
            let (xs, _ys) = build_samples(&rows, |r| r.bid_ask_imbalance, horizon);
            assert!(
                !xs.is_empty(),
                "horizonte {horizon}s não gerou nenhuma amostra"
            );
            // Reconstrói, para cada linha com feature presente, qual `j`
            // build_samples teria usado, e confirma timestamp[j] > timestamp[row].
            for row_ref in &rows {
                if row_ref.bid_ask_imbalance.is_none() {
                    continue;
                }
                let future_ts = row_ref.timestamp_ms + horizon * 1000;
                if let Some(&j) = by_ts.get(&future_ts) {
                    assert!(
                        rows[j].timestamp_ms > row_ref.timestamp_ms,
                        "horizonte {horizon}s: linha futura não é estritamente posterior"
                    );
                    assert_ne!(
                        rows[j].timestamp_ms, row_ref.timestamp_ms,
                        "horizonte {horizon}s: retorno reutilizou a própria linha da feature"
                    );
                }
            }
        }
    }

    /// Auditoria: demonstra por que `microprice`, `microprice_deviation` e
    /// `spread_pct` (features que embutem `mid_price` na própria fórmula)
    /// merecem leitura mais cética — constrói uma série onde o "preço
    /// verdadeiro" nunca se move (zero edge econômico real possível) e
    /// `mid_price` só carrega ruído de medição independente
    /// (`mid = true_price + ruído`, no estilo bid-ask bounce clássico).
    /// `microprice_deviation = microprice - mid_price = -ruído` compartilha
    /// o mesmo termo de ruído que aparece no retorno futuro (que usa
    /// `mid_price` como base) — uma correlação mecânica aparece mesmo sem
    /// nenhuma relação econômica real entre as séries, confirmando que
    /// essa classe de feature precisa da ressalva documentada em
    /// `build_samples`.
    #[test]
    fn microprice_deviation_shows_mechanical_correlation_from_shared_mid_price_term() {
        // Padrão de ruído determinístico (não é preciso ser "aleatório" de
        // verdade — só não-trivial e sem relação com o índice de forma
        // óbvia) — o preço verdadeiro fica fixo em 100.0 o tempo todo, então
        // qualquer correlação encontrada só pode vir do termo de ruído
        // compartilhado, nunca de um movimento real de preço.
        let noise_pattern = [0.05, -0.03, 0.08, -0.07, 0.02, -0.04, 0.06, -0.01];
        let true_price = 100.0;
        let rows: Vec<FeatureGridRow> = (0..400)
            .map(|i| {
                let noise = noise_pattern[(i as usize) % noise_pattern.len()];
                let mid = true_price + noise;
                FeatureGridRow {
                    timestamp_ms: i * 1000,
                    spread_abs: Some(0.01),
                    spread_pct: Some(0.0001),
                    mid_price: Some(mid),
                    microprice: Some(true_price), // sem ruído -> desvio = -ruído do mid
                    bid_ask_imbalance: None,
                    book_imbalance: None,
                    trade_imbalance: None,
                    volume_delta: None,
                    trade_intensity: None,
                    order_flow_imbalance: None,
                }
            })
            .collect();

        let stats = analyze_one(
            "microprice_deviation",
            true,
            |r| match (r.microprice, r.mid_price) {
                (Some(mp), Some(m)) => Some(mp - m),
                _ => None,
            },
            1,
            &rows,
            0.0001,
        );

        let corr = stats
            .pearson_correlation
            .expect("deveria haver correlação mensurável vinda só do ruído compartilhado");
        assert!(
            corr.abs() > 0.1,
            "esperava uma correlação mecânica claramente não-nula (sem nenhum edge real \
             presente na série), veio {corr}"
        );
    }

    #[test]
    fn a_feature_that_perfectly_predicts_direction_gets_hit_rate_one_and_positive_correlation() {
        // Constrói uma série sintética onde o sinal do imbalance sempre
        // acerta a direção do próximo retorno — o caso "feature com
        // informação real" que o analyzer deve reconhecer claramente.
        let mut rows = Vec::new();
        let mut mid = 100.0;
        for i in 0..40 {
            let imbalance = if i % 2 == 0 { 0.8 } else { -0.8 };
            rows.push(row(i * 1000, mid, Some(imbalance)));
            mid += if imbalance > 0.0 { 1.0 } else { -1.0 };
        }
        let stats = analyze_one(
            "bid_ask_imbalance",
            true,
            |r| r.bid_ask_imbalance,
            1,
            &rows,
            0.0001,
        );
        assert_eq!(stats.hit_rate, Some(1.0));
        assert!(stats.pearson_correlation.unwrap() > 0.99);
        assert_eq!(stats.stable_sign, Some(true));
    }

    #[test]
    fn a_feature_with_no_relationship_to_return_gets_near_zero_correlation() {
        // imbalance alterna sem relação nenhuma com o retorno, que é
        // sempre o mesmo valor fixo -> variância de y é zero -> pearson
        // indefinido (None), não um número espúrio.
        let rows: Vec<FeatureGridRow> = (0..40)
            .map(|i| row(i * 1000, 100.0, Some(if i % 2 == 0 { 1.0 } else { -1.0 })))
            .collect();
        let stats = analyze_one(
            "bid_ask_imbalance",
            true,
            |r| r.bid_ask_imbalance,
            1,
            &rows,
            0.0001,
        );
        assert_eq!(stats.pearson_correlation, None);
    }

    #[test]
    fn magnitude_only_features_never_get_a_hit_rate() {
        let rows: Vec<FeatureGridRow> = (0..15)
            .map(|i| row(i * 1000, 100.0 + i as f64, None))
            .collect();
        let stats = analyze_one("spread_abs", false, |r| r.spread_abs, 1, &rows, 0.0001);
        assert_eq!(stats.hit_rate, None);
    }

    #[test]
    fn microprice_deviation_is_computed_as_microprice_minus_mid() {
        let mut r = row(0, 100.0, None);
        r.microprice = Some(100.3);
        let extractor: Extractor = |row| match (row.microprice, row.mid_price) {
            (Some(mp), Some(m)) => Some(mp - m),
            _ => None,
        };
        let deviation = extractor(&r).unwrap();
        assert!((deviation - 0.3).abs() < 1e-9);
    }

    #[test]
    fn estimate_round_trip_cost_uses_the_sample_own_average_spread() {
        let rows = vec![row(0, 100.0, None), row(1000, 100.0, None)];
        // spread_pct fixo em 0.0001 nas duas linhas (ver helper `row`).
        let cost = estimate_round_trip_cost_pct(&rows);
        let expected = 2.0 * PROJECT_TAKER_FEE + 0.0001 + 2.0 * (PROJECT_SLIPPAGE_BPS / 10000.0);
        assert!((cost - expected).abs() < 1e-12);
    }

    /// Auditoria: "comparação econômica usa custo round-trip completo" —
    /// reconstrói o custo de uma compra a mercado (preenche no ask) seguida
    /// de uma venda a mercado (preenche no bid) num book concreto, mais
    /// as duas taxas e os dois slippages, e confirma que bate exatamente
    /// com `estimate_round_trip_cost_pct`. Prova numérica de que o spread
    /// entra uma vez (não duas) e taxa/slippage entram duas vezes (uma por
    /// perna) — não só a fórmula, o resultado de um trade de ida e volta
    /// de verdade.
    #[test]
    fn round_trip_cost_matches_a_hand_worked_buy_at_ask_sell_at_bid_example() {
        let mid: f64 = 100.0;
        let spread_abs: f64 = 0.02; // bid=99.99, ask=100.01
        let spread_pct = spread_abs / mid; // 0.0002

        // Perna 1 (compra a mercado): preenche no ask.
        let buy_fill = mid + spread_abs / 2.0;
        // Perna 2 (venda a mercado): preenche no bid.
        let sell_fill = mid - spread_abs / 2.0;
        // Custo de spread do round-trip = o que se perde saindo vs. entrando,
        // comparado a um par de trades hipotético exatamente no mid:
        // (buy_fill - mid) + (mid - sell_fill) = spread_abs inteiro, uma vez.
        let hand_worked_spread_cost_pct = ((buy_fill - mid) + (mid - sell_fill)) / mid;
        assert!((hand_worked_spread_cost_pct - spread_pct).abs() < 1e-12);

        let hand_worked_total = 2.0 * PROJECT_TAKER_FEE
            + hand_worked_spread_cost_pct
            + 2.0 * (PROJECT_SLIPPAGE_BPS / 10000.0);

        let rows = vec![row(0, mid, None), row(1000, mid, None)]
            .into_iter()
            .map(|mut r| {
                r.spread_pct = Some(spread_pct);
                r
            })
            .collect::<Vec<_>>();
        let formula_result = estimate_round_trip_cost_pct(&rows);

        assert!(
            (formula_result - hand_worked_total).abs() < 1e-12,
            "fórmula deu {formula_result}, conta manual de comprar no ask + vender no bid deu {hand_worked_total}"
        );
    }

    /// Auditoria: "inferência estatística considera dependência causada
    /// por horizontes sobrepostos" — parte 1: `n_effective` cai conforme
    /// o horizonte cresce (mais sobreposição = menos informação
    /// independente), e nunca passa de `n`.
    #[test]
    fn effective_sample_size_shrinks_as_horizon_grows_and_never_exceeds_n() {
        let n = 3000;
        assert_eq!(effective_sample_size(n, 1), n); // sem sobreposição -> n_eff = n
        assert_eq!(effective_sample_size(n, 5), n / 5);
        assert_eq!(effective_sample_size(n, 300), n / 300);
        assert_eq!(effective_sample_size(0, 300), 0);

        let mut prev = n;
        for &h in &DEFAULT_HORIZONS_SECS {
            let eff = effective_sample_size(n, h);
            assert!(eff <= prev, "n_effective deveria ser não-crescente em h");
            assert!(eff <= n);
            prev = eff;
        }
    }

    /// Auditoria, parte 2: o CI por bootstrap de blocos é mais largo que
    /// um bootstrap ingênuo (IID, `block_len=1`) sobre a MESMA série
    /// autocorrelacionada — prova de que o método realmente reflete a
    /// dependência, não é só um campo decorativo que nunca muda nada.
    /// Constrói uma série de retornos com autocorrelação forte de
    /// propósito (blocos de 20 valores repetidos, do jeito que um
    /// horizonte grande produziria via sobreposição).
    #[test]
    fn block_bootstrap_ci_is_wider_than_a_naive_iid_bootstrap_on_autocorrelated_data() {
        let n = 400;
        let block_size = 20;
        let xs: Vec<f64> = (0..n).map(|i| ((i / block_size) % 5) as f64).collect();
        let ys: Vec<f64> = (0..n)
            .map(|i| ((i / block_size) % 5) as f64 * 0.5 + ((i % 3) as f64 - 1.0) * 0.1)
            .collect();

        let naive = block_bootstrap_pearson_ci(&xs, &ys, 1, BOOTSTRAP_REPLICATES, BOOTSTRAP_SEED)
            .expect("amostra grande o bastante para o bootstrap ingênuo");
        let block = block_bootstrap_pearson_ci(
            &xs,
            &ys,
            block_size * 2,
            BOOTSTRAP_REPLICATES,
            BOOTSTRAP_SEED,
        )
        .expect("amostra grande o bastante para o bootstrap por blocos");

        let naive_width = naive.1 - naive.0;
        let block_width = block.1 - block.0;
        assert!(
            block_width > naive_width,
            "bootstrap por blocos ({block_width:.4}) deveria ser mais largo que o ingênuo \
             ({naive_width:.4}) sobre dado autocorrelacionado — senão a correção não está \
             fazendo diferença nenhuma"
        );
    }

    /// Auditoria, parte 3: ponta a ponta via `analyze_one` — um horizonte
    /// maior (mais sobreposição) produz `n_effective` estritamente menor
    /// que um horizonte de 1s sobre a mesma série (garantia determinística
    /// de `effective_sample_size`, testada aqui através de `analyze_one`
    /// de ponta a ponta, não só na função isolada), e ambos os horizontes
    /// produzem um CI de Pearson calculável. A comparação de *largura* do
    /// CI entre horizontes diferentes não é uma garantia matemática rígida
    /// para qualquer dado arbitrário (depende da estrutura de correlação
    /// realizada em cada um) — essa comparação já foi feita de forma
    /// controlada, isolando só o efeito da dependência, no teste
    /// `block_bootstrap_ci_is_wider_than_a_naive_iid_bootstrap_on_autocorrelated_data`
    /// acima (mesma série, só o método de bootstrap muda).
    #[test]
    fn longer_horizon_yields_smaller_n_effective_end_to_end() {
        let rows: Vec<FeatureGridRow> = (0..6000)
            .map(|i| {
                let mid = 100.0 + (i as f64 / 50.0).sin() * 5.0;
                row(i * 1000, mid, Some(((i / 15) % 4) as f64 - 1.5))
            })
            .collect();

        let short = analyze_one(
            "bid_ask_imbalance",
            true,
            |r| r.bid_ask_imbalance,
            1,
            &rows,
            0.0001,
        );
        let long = analyze_one(
            "bid_ask_imbalance",
            true,
            |r| r.bid_ask_imbalance,
            300,
            &rows,
            0.0001,
        );

        assert!(long.n_effective < short.n_effective);
        assert_eq!(short.n_effective, short.n); // horizonte 1s -> sem desconto
        assert_eq!(long.n_effective, long.n / 300);
        assert!(short.pearson_ci95_lo.is_some() && short.pearson_ci95_hi.is_some());
        assert!(long.pearson_ci95_lo.is_some() && long.pearson_ci95_hi.is_some());
    }

    #[test]
    fn analyze_covers_every_registered_feature_and_horizon() {
        let rows: Vec<FeatureGridRow> = (0..400)
            .map(|i| {
                row(
                    i * 1000,
                    100.0 + (i % 7) as f64,
                    Some(((i % 5) as f64) - 2.0),
                )
            })
            .collect();
        let horizons = [1, 5];
        let stats = analyze(&rows, &horizons);
        assert_eq!(stats.len(), feature_names().len() * horizons.len());
    }
}
