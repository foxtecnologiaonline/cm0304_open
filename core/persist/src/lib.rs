//! `persist` — serialização do save (`docs/03-modelo-de-dados.md §8`).
//!
//! Primeira fatia real: um formato binário pequeno, versionado, com
//! checksum e escrita atômica — o suficiente para `app::GameSession`
//! salvar/carregar por *replay* (ver o doc de [`save`] para o porquê disso
//! ser a escolha certa hoje, não um atalho). Migrações (`docs/02 §8.3`) e
//! os índices SQLite (`docs/02 §8.2`) ainda não existem — chegam quando
//! houver estado mutável de verdade (elenco, contratos) para indexar.
#![warn(clippy::all)]

mod error;
mod save;

pub use error::PersistError;
pub use save::{
    SCHEMA_VERSION, SIM_VERSION, SaveFile, decode, encode, load_from_path, save_to_path,
};
