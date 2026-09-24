use async_trait::async_trait;
use domain::{AssetClass, Candle, Instrument, MarketDataKind, MarketEvent, Timeframe};
use tokio::sync::mpsc;

use crate::error::MarketDataError;

/// O que um `MarketDataProvider` é capaz de fornecer: quais tipos de dados
/// de mercado, para quais classes de ativos. `strategies::check_compatible`
/// é verificado contra isso — uma estratégia não pode ser registrada para
/// um instrumento cujo provider não declara os dados que ela exige.
#[derive(Debug, Clone)]
pub struct ProviderCapabilities {
    pub market_data_kinds: Vec<MarketDataKind>,
    pub asset_classes: Vec<AssetClass>,
}

/// Uma fonte de dados de mercado, desacoplada de qualquer exchange
/// específica.
///
/// As implementações mantêm internamente todos os formatos de transmissão
/// específicos da exchange (veja `binance::dto`) e nunca devem vazá-los
/// através desta trait: todo método aqui retorna tipos puros de `domain`.
/// Adicionar uma nova exchange significa adicionar uma nova implementação
/// de `MarketDataProvider`, e não mexer nesta trait nem em quem a consome.
#[async_trait]
pub trait MarketDataProvider: Send + Sync {
    fn capabilities(&self) -> ProviderCapabilities;

    /// Busca os `limit` candles fechados mais recentes de `instrument` no
    /// `timeframe`. Usado para aquecimento (preencher o estado dos
    /// indicadores da estratégia) antes de passar para o stream ao vivo.
    async fn fetch_recent_candles(
        &self,
        instrument: &Instrument,
        timeframe: Timeframe,
        limit: u32,
    ) -> Result<Vec<Candle>, MarketDataError>;

    /// Busca os metadados atuais do instrumento negociável (tick size, lot
    /// size, mínimos) para `base`/`quote`, de modo que o resto do sistema
    /// nunca precise deixar os filtros da exchange fixos no código.
    async fn fetch_instrument(
        &self,
        base: &domain::Asset,
        quote: &domain::Asset,
    ) -> Result<Instrument, MarketDataError>;

    /// Abre um stream ao vivo de eventos de mercado para `instruments` no
    /// `timeframe`. O canal retornado é alimentado por uma task em segundo
    /// plano que reconecta automaticamente em caso de falha; a task só para
    /// quando o receptor é descartado.
    async fn stream(
        &self,
        instruments: Vec<Instrument>,
        timeframe: Timeframe,
    ) -> Result<mpsc::UnboundedReceiver<MarketEvent>, MarketDataError>;
}
