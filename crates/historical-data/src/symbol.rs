use domain::Asset;

/// O nome de diretório/arquivo usado para um par de ativos — sem separador,
/// mesma convenção "wire" que a integração Binance já usa internamente
/// (`market_data::binance::symbol`, privado a esse crate, por isso este
/// pequeno helper equivalente aqui em vez de reexportá-lo).
pub fn folder_symbol(base: &Asset, quote: &Asset) -> String {
    format!("{base}{quote}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn joins_base_and_quote_without_separator() {
        let base = Asset::new("BTC").unwrap();
        let quote = Asset::new("USDT").unwrap();
        assert_eq!(folder_symbol(&base, &quote), "BTCUSDT");
    }
}
