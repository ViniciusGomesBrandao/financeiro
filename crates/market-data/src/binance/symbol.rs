use domain::{Asset, Instrument};

/// Converte um `Instrument` no formato de símbolo de transmissão da Binance
/// (por exemplo `BTCUSDT`, sem separador). Este é o *único* lugar do crate
/// que deve construir essa string — todo o resto precisa usar
/// `domain::Symbol` ou `InstrumentId`.
pub fn to_wire_symbol(base: &Asset, quote: &Asset) -> String {
    format!("{base}{quote}")
}

pub fn instrument_wire_symbol(instrument: &Instrument) -> String {
    to_wire_symbol(&instrument.base_asset, &instrument.quote_asset)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_without_separator_uppercase() {
        let base = Asset::new("btc").unwrap();
        let quote = Asset::new("usdt").unwrap();
        assert_eq!(to_wire_symbol(&base, &quote), "BTCUSDT");
    }
}
