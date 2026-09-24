//! Estratégias específicas de ações: lógica que depende de dados ou de estrutura
//! de mercado sem equivalente em cripto (sessões de negociação, gaps de abertura
//! de mercado, eventos corporativos, ...), portanto nunca deve declarar suporte
//! a `AssetClass::Crypto`.
//!
//! Nada existe aqui ainda — ações não são implementadas nesta fase do projeto de
//! forma alguma (veja `docs/architecture.md`, "Adding equities"). O primeiro
//! habitante natural, uma vez que existam um `MarketDataProvider` e um
//! `TradingCalendar` de ações, é uma estratégia consciente de sessões:
//!
//! ```text
//! MarketOpenStrategy
//!   requires:  MarketDataKind::Ohlcv, MarketDataKind::TradingSession
//!   supports:  AssetClass::Equity only
//! ```
//!
//! Não generalize uma estratégia de ações para `generic/` só para "reaproveitar
//! código", a menos que o cálculo subjacente seja genuinamente agnóstico de
//! classe de ativo — veja o princípio do `README.md` na raiz: "não force uma
//! estratégia específica de um mercado a funcionar em outro".
