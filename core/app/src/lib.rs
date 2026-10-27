//! `app` — a única fronteira pública do núcleo (`docs/02-arquitetura.md §4`):
//! `dispatch` muda estado, `query` lê, nunca o contrário. É por aqui que a
//! futura ponte `flutter_rust_bridge` vai falar com o resto do núcleo — a UI
//! nunca importa `world`/`rules`/`engine`/`pack` diretamente.
//!
//! O que existe: comandos de carreira (assumir um clube, escalar, instruir,
//! jogar uma rodada, a temporada ou uma partida ao vivo minuto a minuto) e
//! consultas (competições, tabelas, elenco, calendário, partida). Tudo é
//! reproduzível: o save guarda o log de comandos e [`GameSession::load`]
//! reconstrói a sessão repetindo-o (ver `persist::save`). Falta o stream de
//! `events()` (notificações, caixa de entrada — F8) e a ponte Flutter, que
//! não dá para construir nem verificar sem o SDK Dart.
#![warn(clippy::all)]

mod command;
mod error;
mod inbox;
mod query;
mod session;
mod slots;

pub use ai::Formation;
pub use command::{CoachProfile, Command, MAX_COACH_NAME_BYTES};
pub use engine::Side;
pub use error::AppError;
pub use inbox::{INBOX_CAPACITY, InboxKind, InboxMessage};
pub use query::{
    AttributeValue, ClubDetail, ClubRow, CompetitionKind, CompetitionSummary, CupChampionRow,
    FixtureList, FixtureRow, IncomingOfferRow, LineupInfo, LineupSlot, ManagerInfo, MatchLineRow,
    MatchScore, MatchView, PitchPlayer, PlayerDetail, PlayerMatchRow, PlayerRef, PlayerRow, Query,
    QueryResult, ResultRow, RoundResults, SearchPage, SeasonProgress, StandingsRow,
    TransferWindowInfo,
};
pub use query::{InboxPage, MAX_INBOX_PAGE};
pub use query::{MAX_SEARCH_PAGE, PlayerFilter};
pub use session::{CommandReceipt, GameSession};
pub use slots::{SAVE_SLOT_COUNT, autosave_path, slot_path};
pub use world::{MatchLineKind, Mentality, OfferOutcome, Pressing, Refusal, Training};
