//! `ai` — IA de clube: escalação, mercado de transferências, finanças.
//!
//! Primeira fatia real: [`lineup::select_starting_eleven`] (escalação
//! automática, `docs/07-roadmap.md` M1). Mercado de transferências e
//! finanças continuam sem nenhuma linha (mercado: `docs/01-requisitos.md
//! §2.4`; táticas/escalação completa com formação escolhida, capitão e
//! cobradores, RF-PA-01: `docs/01 §2.5`).
//!
//! Depende só de `domain` (`docs/02-arquitetura.md §2`: "motor, IA e mundo
//! dependem de domínio — nunca o inverso") — é `world` quem depende de
//! `ai`, nunca ao contrário, então nada aqui sabe o que é um `pack` ou uma
//! `GameSession`.
#![warn(clippy::all)]

mod lineup;

pub use lineup::{FORMATION, PlayerRating, select_starting_eleven};
