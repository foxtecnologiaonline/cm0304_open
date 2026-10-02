//! `app` — a única fronteira pública do núcleo (`docs/02-arquitetura.md §4`):
//! `dispatch` muda estado, `query` lê, nunca o contrário. É por aqui que a
//! futura ponte `flutter_rust_bridge` vai falar com o resto do núcleo — a UI
//! nunca importa `world`/`rules`/`engine`/`pack` diretamente.
//!
//! O que existe hoje é a primeira fatia vertical real, não o contrato
//! final: um comando ([`Command::AdvanceSeason`]) e três consultas
//! ([`Query`]), o suficiente para uma UI carregar um pack, avançar
//! temporadas e mostrar uma tabela de classificação. [`GameSession::save_to_path`]/
//! [`GameSession::load_from_path`] (via `persist`) cobrem save/load por
//! *replay* — ver o doc de `persist::save` para o porquê disso bastar hoje.
//! Falta o stream de `events()` (chega junto de eventos de partida
//! navegáveis, M2/M3); quando `Command` ganhar variantes com parâmetros
//! (transferência, escalação...), o save por replay-de-contador deixa de
//! bastar e precisa virar um log de comandos de verdade.
#![warn(clippy::all)]

mod error;
mod query;
mod session;
mod slots;

pub use error::AppError;
pub use query::{
    CompetitionKind, CompetitionSummary, CupChampionRow, Query, QueryResult, StandingsRow,
};
pub use session::{Command, CommandReceipt, GameSession};
pub use slots::{SAVE_SLOT_COUNT, autosave_path, slot_path};
