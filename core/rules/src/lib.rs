//! `rules` — regras de competição dirigidas por dados
//! (`docs/03-modelo-de-dados.md §7`).
//!
//! Duas responsabilidades, ambas funções puras (sem I/O, sem RNG): gerar o
//! calendário de um turno/returno ([`fixture`]) e calcular a tabela de
//! classificação a partir de resultados, aplicando os critérios de
//! desempate configurados no pack ([`table`]). Quem chama isso com dados de
//! verdade — e decide quando cada rodada acontece no calendário do mundo —
//! é o `world` (M1).
#![warn(clippy::all)]

mod fixture;
mod table;

pub use fixture::{Fixture, FixtureError, round_robin, single_round_robin};
pub use table::{Score, TableRow, compute_table};
