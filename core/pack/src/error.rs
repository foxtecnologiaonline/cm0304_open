//! Erros de carregamento de um data pack.
//!
//! `PackError` cobre só problemas **estruturais** (arquivo ausente, JSON/TOML
//! malformado) — um pack que carrega mas tem uma referência quebrada (ex.:
//! clube apontando para um país inexistente) não é um `PackError`, é uma
//! entrada na lista de `issues` de [`crate::PackReport`]. A distinção
//! importa: um validador que aborta no primeiro problema semântico obriga o
//! autor do pack a corrigir um erro por vez; acumular problemas é o que
//! `docs/03-modelo-de-dados.md §7` pede ("validador com mensagens de erro
//! úteis").

use std::fmt;
use std::path::PathBuf;

/// Falha ao ler ou desserializar um data pack do disco.
#[derive(Debug)]
pub enum PackError {
    /// Falha de I/O lendo um arquivo ou diretório do pack.
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
    /// `pack.toml` ausente na raiz do pack.
    MissingManifest { path: PathBuf },
    /// `pack.toml` presente mas não é TOML válido, ou falta um campo obrigatório.
    InvalidManifest {
        path: PathBuf,
        source: toml::de::Error,
    },
    /// Um arquivo dentro de `nations/`, `competitions/` ou `clubs/` não é JSON válido.
    InvalidJson {
        path: PathBuf,
        source: serde_json::Error,
    },
}

impl fmt::Display for PackError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PackError::Io { path, source } => {
                write!(f, "erro de E/S em {}: {source}", path.display())
            }
            PackError::MissingManifest { path } => {
                write!(f, "pack.toml não encontrado em {}", path.display())
            }
            PackError::InvalidManifest { path, source } => {
                write!(f, "pack.toml inválido em {}: {source}", path.display())
            }
            PackError::InvalidJson { path, source } => {
                write!(f, "JSON inválido em {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for PackError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PackError::Io { source, .. } => Some(source),
            PackError::InvalidManifest { source, .. } => Some(source),
            PackError::InvalidJson { source, .. } => Some(source),
            PackError::MissingManifest { .. } => None,
        }
    }
}
