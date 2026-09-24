//! Tradução de textos de `reason` legados para português, só na camada de
//! apresentação — nunca altera `risk::RejectionReason` nem seu `Display`
//! (que continuam em inglês, ver `risk/src/decision.rs`).
//!
//! `app::pipeline::translate_rejection_reason` já traduz toda decisão nova
//! na escrita (ver `crates/app/src/pipeline.rs`), então este módulo só
//! existe para as linhas que já estavam em `risk_decisions.reason` antes
//! desse fix — presas em inglês no Postgres, mas ainda dentro da janela das
//! 100 decisões mais recentes que a timeline exibe. Reconstrói o mesmo
//! texto em português a partir do formato exato de
//! `risk::RejectionReason::Display` (comentado aqui só para referência —
//! não é lido de `risk`, é reimplementado via parsing de string, já que a
//! entrada aqui é só o texto já persistido, não o enum tipado).

/// Traduz um texto de motivo já persistido, se reconhecer o formato
/// (inglês, produzido por uma versão anterior do código). Se não reconhecer
/// (formato desconhecido, já em português, ou texto livre), devolve o texto
/// original sem alterações — nunca esconde ou adivinha um motivo.
pub fn translate_legacy_reason(text: &str) -> String {
    if let Some(t) = translate_fixed(text) {
        return t.to_string();
    }
    if let Some(t) = translate_max_open_positions(text) {
        return t;
    }
    if let Some(t) = translate_two_values(
        text,
        "position size ",
        " exceeds max ",
        "tamanho da posição",
        "excede o máximo permitido",
    ) {
        return t;
    }
    if let Some(t) = translate_two_values(
        text,
        "projected exposure ",
        " exceeds max ",
        "exposição projetada",
        "excede o máximo permitido",
    ) {
        return t;
    }
    if let Some((required, available)) = text
        .strip_prefix("insufficient balance: need ")
        .and_then(|rest| rest.split_once(", have "))
    {
        return format!("saldo insuficiente: necessário {required}, disponível {available}");
    }
    if let Some((realized_today, limit)) = text
        .strip_prefix("daily loss limit breached: realized ")
        .and_then(|rest| rest.split_once(", limit "))
    {
        return format!(
            "limite de perda diária atingido: realizado {realized_today}, limite {limit}"
        );
    }
    if let Some(t) = translate_two_values(
        text,
        "order notional ",
        " below exchange minimum ",
        "valor da ordem",
        "abaixo do mínimo exigido pela corretora",
    ) {
        return t;
    }
    text.to_string()
}

fn translate_fixed(text: &str) -> Option<&'static str> {
    match text {
        "no open position to flatten" => Some("nenhuma posição aberta para zerar"),
        "short selling is not supported on spot; no position to sell" => {
            Some("venda a descoberto não é suportada (mercado à vista); não há posição para vender")
        }
        "position already open for this instrument" => {
            Some("já existe uma posição aberta para este ativo")
        }
        "stop_loss triggered" => Some("stop loss acionado"),
        "take_profit triggered" => Some("take profit acionado"),
        _ => None,
    }
}

fn translate_max_open_positions(text: &str) -> Option<String> {
    let inside = text
        .strip_prefix("max open positions reached (")?
        .strip_suffix(')')?;
    let (current, max) = inside.split_once('/')?;
    Some(format!(
        "limite de posições abertas atingido ({current}/{max})"
    ))
}

/// Formatos no padrão `"{prefix}{valor1}{middle}{valor2}"` (sem sufixo),
/// como os de `PositionSizeExceeded`/`ExposureLimitExceeded`/`BelowExchangeMinimum`.
fn translate_two_values(
    text: &str,
    prefix: &str,
    middle: &str,
    label1_pt: &str,
    label2_pt: &str,
) -> Option<String> {
    let (value1, value2) = text.strip_prefix(prefix)?.split_once(middle)?;
    Some(format!("{label1_pt} {value1} {label2_pt} {value2}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn translates_fixed_reasons() {
        assert_eq!(
            translate_legacy_reason("position already open for this instrument"),
            "já existe uma posição aberta para este ativo"
        );
        assert_eq!(
            translate_legacy_reason("short selling is not supported on spot; no position to sell"),
            "venda a descoberto não é suportada (mercado à vista); não há posição para vender"
        );
        assert_eq!(
            translate_legacy_reason("no open position to flatten"),
            "nenhuma posição aberta para zerar"
        );
    }

    #[test]
    fn translates_legacy_exit_triggers() {
        assert_eq!(
            translate_legacy_reason("stop_loss triggered"),
            "stop loss acionado"
        );
        assert_eq!(
            translate_legacy_reason("take_profit triggered"),
            "take profit acionado"
        );
    }

    #[test]
    fn translates_max_open_positions_reached() {
        assert_eq!(
            translate_legacy_reason("max open positions reached (3/5)"),
            "limite de posições abertas atingido (3/5)"
        );
    }

    #[test]
    fn translates_money_pair_variants() {
        assert_eq!(
            translate_legacy_reason("position size 1234.56 exceeds max 2000"),
            "tamanho da posição 1234.56 excede o máximo permitido 2000"
        );
        assert_eq!(
            translate_legacy_reason("projected exposure 9500 exceeds max 10000"),
            "exposição projetada 9500 excede o máximo permitido 10000"
        );
        assert_eq!(
            translate_legacy_reason("insufficient balance: need 1000, have 500"),
            "saldo insuficiente: necessário 1000, disponível 500"
        );
        assert_eq!(
            translate_legacy_reason("daily loss limit breached: realized -2100, limit 2000"),
            "limite de perda diária atingido: realizado -2100, limite 2000"
        );
        assert_eq!(
            translate_legacy_reason("order notional 5 below exchange minimum 10"),
            "valor da ordem 5 abaixo do mínimo exigido pela corretora 10"
        );
    }

    #[test]
    fn leaves_unrecognized_text_unchanged() {
        assert_eq!(
            translate_legacy_reason("já traduzido — texto qualquer"),
            "já traduzido — texto qualquer"
        );
        assert_eq!(
            translate_legacy_reason("formato desconhecido"),
            "formato desconhecido"
        );
    }
}
