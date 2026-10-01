//! `engine` — motor de partida (`docs/04-motor-de-partida.md`).
//!
//! Este crate implementa o **v0.5** do plano evolutivo (`docs/04 §7`):
//! força relativa decide o volume de finalizações, e uma comparação
//! ponderada finalização-vs-goleiro decide se cada uma vira gol — a
//! primeira fatia real da camada de evento (`§2.3`), ainda sem a camada
//! tática (`§2.1`) nem a de posse por zona (`§2.2`). O resto do contrato de
//! [`MatchEvent`] (`§5`) chega em v1/v2, junto de `world`/`rules` (M1) e de
//! jogadores em campo de verdade (M2/M3).
//!
//! Depende só de `domain` — não sabe nada sobre `pack`, `world` ou
//! escalação. Recebe um [`TeamMatchProfile`] por lado e devolve um fluxo de
//! [`MatchEvent`]; de onde cada número vem é problema de quem chama
//! (`docs/02 §2`: "engine... depende de domínio — nunca o inverso").
#![warn(clippy::all)]

mod event;
mod simulate;

pub use event::{MatchEvent, Side, score, shots};
pub use simulate::{
    FinishingQuality, GoalkeepingQuality, MatchContext, TeamMatchProfile, TeamStrength, simulate,
};
