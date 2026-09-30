//! `ai` — IA de clube: escalação, mercado de transferências, finanças.
//!
//! Duas fatias reais: [`lineup::select_starting_eleven`] (escalação
//! automática) e [`market::run_market_day`] (primeira IA de mercado) —
//! ambas `docs/07-roadmap.md` M1. Finanças de verdade (receita, folha
//! salarial, `docs/01 §2.8`) continuam sem nenhuma linha; táticas/escalação
//! completa com formação escolhida, capitão e cobradores (RF-PA-01,
//! `docs/01 §2.5`) e o mercado completo com negociação/contratos/reputação
//! (RF-TR-01 a 09, `docs/01 §2.4`) são M2/M3.
//!
//! Depende só de `domain` (`docs/02-arquitetura.md §2`: "motor, IA e mundo
//! dependem de domínio — nunca o inverso") — é `world` quem depende de
//! `ai`, nunca ao contrário, então nada aqui sabe o que é um `pack` ou uma
//! `GameSession`.
#![warn(clippy::all)]

mod lineup;
mod market;

pub use lineup::{FORMATION, PlayerRating, select_starting_eleven};
pub use market::{MarketPlayer, Transfer, market_value, run_market_day};
