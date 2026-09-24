//! Volume relativo.
//!
//! **Significado**: quantas vezes o volume do candle atual está acima
//! (`> 1`) ou abaixo (`< 1`) do volume médio recente — um jeito rápido de
//! sinalizar "isto é um candle de atividade incomum", útil para filtrar
//! sinais gerados em barras de baixa liquidez.
//!
//! **Fórmula**: `volume_relativo = volume_t / média(volume, período)`, com
//! a janela **incluindo** o candle atual (é informação disponível no
//! instante em que o candle fecha, então não há motivo para excluí-la).
//!
//! **Limitações**: não distingue volume comprador de vendedor (isso exige
//! dados de trade/order flow — ver o aviso de escopo no doc do crate raiz
//! sobre um futuro módulo de microestrutura). Indefinido quando a média da
//! janela é zero (instrumento sem nenhum volume no período).

use crate::primitives::RollingWindow;

#[derive(Debug, Clone)]
pub struct RelativeVolume {
    window: RollingWindow,
}

impl RelativeVolume {
    pub fn new(period: usize) -> Self {
        Self {
            window: RollingWindow::new(period),
        }
    }

    pub fn update(&mut self, volume: f64) -> Option<f64> {
        self.window.push(volume);
        if !self.window.is_full() {
            return None;
        }
        let mean = self.window.mean()?;
        if mean == 0.0 {
            return None;
        }
        Some(volume / mean)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn relative_volume_of_average_bar_is_one() {
        let mut rv = RelativeVolume::new(3);
        rv.update(10.0);
        rv.update(10.0);
        let out = rv.update(10.0).unwrap();
        assert!((out - 1.0).abs() < 1e-12);
    }

    #[test]
    fn spike_bar_reads_above_one() {
        let mut rv = RelativeVolume::new(3);
        rv.update(10.0);
        rv.update(10.0);
        rv.update(10.0);
        // janela agora [10,10,30], média=16.666...
        let out = rv.update(30.0).unwrap();
        assert!((out - 30.0 / (50.0 / 3.0)).abs() < 1e-9);
    }
}
