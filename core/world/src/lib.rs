//! `world` — o loop que faz o mundo girar sozinho, sem UI: calendário,
//! temporada, tabela, promoção e rebaixamento, progressão de jogadores,
//! lesões e mercado. Consome um `pack::LoadedPack`, o `engine` de partida e
//! a `ai` de clube; é o portão do M1
//! (`docs/07-roadmap.md#m1--kick-off-headless-10-semanas`).
//!
//! Ainda não implementado: regens, aposentadoria, moral, calendário mensal
//! (`docs/07-roadmap.md`). Clubes sem elenco no pack usam força e
//! orçamento sintéticos ([`strength`], [`finance`]), substitutos
//! documentados, não uma simulação de elenco. Copa ([`cup`]) é fase única,
//! sem sorteio de confronto e com empate decidido por moeda — escopo mais
//! estreito que uma copa de verdade, documentado em [`cup`]. Condição
//! ([`condition`]) é por temporada, não cumulativa e não cobre fadiga
//! acumulada de carreira nem frescor dia a dia — documentado em
//! [`condition`]. Suspensão ([`discipline`]) é a única coisa aqui com
//! granularidade de rodada (liga simulada rodada a rodada no caminho de
//! carreira, `season::run_season`) — sem cartão amarelo acumulado, sem
//! reincidência, documentado em [`discipline`].
#![warn(clippy::all)]

mod condition;
mod cup;
mod discipline;
mod finance;
mod fixture_key;
mod injuries;
mod market;
mod progression;
mod quality;
mod season;
mod strength;

pub use condition::{FULL_CONDITION, apply_season_fatigue};
pub use cup::{CupMatch, CupResult, cup_participants, run_cup};
pub use finance::generate_budgets;
pub use injuries::roll_injuries;
pub use market::run_market_day;
pub use progression::{PlayerState, advance_season, initial_roster};
pub use quality::{
    generate_match_profiles, generate_match_profiles_from_roster, starters_from_roster_all_clubs,
};
pub use season::{SeasonResult, run_season, run_seasons};
pub use strength::{generate_strengths, generate_strengths_from_roster, strength_of};
