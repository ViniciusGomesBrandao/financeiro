/// Converte `base`/`quote` (ex. `"BTC"`, `"USDT"`) no formato de símbolo de
/// transmissão da Binance (`"BTCUSDT"`, sem separador, maiúsculo). Mesma
/// convenção de `market_data::binance::symbol::to_wire_symbol`, redefinida
/// aqui porque aquela função é privada ao crate `market-data`.
pub fn wire_symbol(base: &str, quote: &str) -> String {
    format!("{}{}", base.to_uppercase(), quote.to_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_without_separator_uppercase() {
        assert_eq!(wire_symbol("btc", "usdt"), "BTCUSDT");
        assert_eq!(wire_symbol("ETH", "USDT"), "ETHUSDT");
    }
}
