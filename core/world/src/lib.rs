//! `world` — o loop que faz o mundo girar sozinho, sem UI: calendário,
//! temporada, tabela, promoção e rebaixamento. Consome um
//! `pack::LoadedPack` e o `engine` de partida; é o portão do M1
//! (`docs/07-roadmap.md#m1--kick-off-headless-10-semanas`).
//!
//! Ainda não implementado: progressão de jogadores, lesões, regens — tudo
//! isso pressupõe jogadores de verdade (`docs/01 §2.1-2.3`), que só chegam
//! com `ai`/elenco (M2/M3). A força de cada clube usada aqui é sintética
//! ([`strength`]), um substituto documentado, não uma simulação de elenco.
#![warn(clippy::all)]

mod season;
mod strength;

pub use season::{SeasonResult, run_season, run_seasons};
pub use strength::{generate_strengths, strength_of};
