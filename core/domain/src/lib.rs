//! `domain` — tipos puros do núcleo do ManagerFC.
//!
//! Este crate não depende de I/O, de tempo de sistema nem de nenhum outro
//! crate do workspace (`docs/02-arquitetura.md §2`: "domínio não depende de
//! nada"). É a base sobre a qual `rules`, `engine`, `world` e `ai` são
//! construídos, e é o único lugar onde as regras de determinismo do
//! `ADR 0002` são impostas mecanicamente: o crate nega
//! `clippy::float_arithmetic`, então introduzir `f32`/`f64` em qualquer
//! cálculo aqui já falha a build antes de chegar à revisão.
#![warn(clippy::all)]
#![allow(clippy::module_name_repetitions)]

pub mod attributes;
pub mod date;
pub mod fixed;
pub mod ids;
pub mod rng;

pub use attributes::{Ability, Attribute, PlayerAttributes};
pub use date::GameDate;
pub use fixed::Fixed;
pub use ids::{
    ClubId, CompetitionId, ContractId, FixtureId, NationId, PersonId, PlayerId, SeasonId,
};
pub use rng::DeterministicRng;
