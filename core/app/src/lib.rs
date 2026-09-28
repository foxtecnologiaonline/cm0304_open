//! `app` — a única fronteira pública do núcleo (`docs/02-arquitetura.md §4`):
//! `dispatch` muda estado, `query` lê, nunca o contrário. É por aqui que a
//! futura ponte `flutter_rust_bridge` vai falar com o resto do núcleo — a UI
//! nunca importa `world`/`rules`/`engine`/`pack` diretamente.
//!
//! O que existe hoje é a primeira fatia vertical real, não o contrato
//! final: um comando ([`Command::AdvanceSeason`]) e três consultas
//! ([`Query`]), o suficiente para uma UI carregar um pack, avançar
//! temporadas e mostrar uma tabela de classificação. Faltam: o log de
//! comandos serializável (a base do save, `docs/02 §4` e `§8`) e o stream
//! de `events()` (chega junto de eventos de partida navegáveis, M2/M3).
#![warn(clippy::all)]

mod error;
mod query;
mod session;

pub use error::AppError;
pub use query::{CompetitionSummary, Query, QueryResult, StandingsRow};
pub use session::{Command, CommandReceipt, GameSession};
