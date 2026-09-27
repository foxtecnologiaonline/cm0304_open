//! `engine` — motor de partida (`docs/04-motor-de-partida.md`).
//!
//! Este crate implementa o **v0** do plano evolutivo (`docs/04 §7`): só a
//! camada estatística, "força relativa → placar". As camadas de tática
//! (`docs/04 §2.1`), posse por zona (`§2.2`) e o resto do contrato de
//! [`MatchEvent`] (`§5`) chegam em v1/v2, junto de `world`/`rules`
//! (M1) e de jogadores em campo de verdade (M2/M3).
//!
//! Depende só de `domain` — não sabe nada sobre `pack`, `world` ou
//! escalação. Recebe duas [`TeamStrength`] e devolve um fluxo de
//! [`MatchEvent`]; de onde a força vem é problema de quem chama
//! (`docs/02 §2`: "engine... depende de domínio — nunca o inverso").
#![warn(clippy::all)]

mod event;
mod simulate;

pub use event::{MatchEvent, Side, score};
pub use simulate::{MatchContext, TeamStrength, simulate};
