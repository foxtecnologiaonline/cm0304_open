//! `world` — o loop que faz o mundo girar sozinho, sem UI: calendário,
//! temporada, tabela, promoção e rebaixamento, progressão de jogadores e
//! mercado. Consome um `pack::LoadedPack`, o `engine` de partida e a `ai`
//! de clube; é o portão do M1
//! (`docs/07-roadmap.md#m1--kick-off-headless-10-semanas`).
//!
//! Ainda não implementado: lesões, regens, aposentadoria, calendário
//! mensal (`docs/07-roadmap.md`). Clubes sem elenco no pack usam força e
//! orçamento sintéticos ([`strength`], [`finance`]), substitutos
//! documentados, não uma simulação de elenco.
#![warn(clippy::all)]

mod finance;
mod market;
mod progression;
mod season;
mod strength;

pub use finance::generate_budgets;
pub use market::run_market_day;
pub use progression::{PlayerState, advance_season, initial_roster};
pub use season::{SeasonResult, run_season, run_seasons};
pub use strength::{generate_strengths, generate_strengths_from_roster, strength_of};
