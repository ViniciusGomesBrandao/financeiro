//! Estratégias específicas de cripto: lógica que depende de dados ou de
//! estrutura de mercado sem equivalente em ações, portanto nunca deve declarar
//! suporte a `AssetClass::Equity`.
//!
//! Nada existe aqui ainda. Este módulo (e seu irmão `equities/`) existe para
//! manter o *formato* da base de código correto desde o primeiro dia — veja o
//! `README.md` na raiz, seção "Strategy compatibility" — mesmo antes de uma
//! estratégia concreta precisar dele.
//!
//! O primeiro habitante natural é uma estratégia de funding rate para futuros
//! perpétuos:
//!
//! ```text
//! FundingRateStrategy
//!   requires:  MarketDataKind::FundingRate
//!   supports:  AssetClass::CryptoDerivative only
//! ```
//!
//! Ainda não está implementada porque o crate `market-data` ainda não obtém
//! dados de funding rate (API de futuros USD-M da Binance, não Spot). Adicione
//! primeiro a capacidade no provedor, depois a estratégia — não implemente uma
//! estratégia contra um feed de dados que o sistema não consegue fornecer de
//! fato.
