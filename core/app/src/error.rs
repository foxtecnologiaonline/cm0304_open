//! Erros da fronteira `app` — sempre tipados, nunca `panic!`
//! (`docs/02-arquitetura.md §9.2`: "núcleo devolve `Result` tipado; `panic`
//! é bug"). É o `app` quem primeiro traduz um erro de I/O do `pack` (ou uma
//! condição de estado inválida) em algo que a UI consegue mostrar como
//! mensagem acionável, sem nunca perder a sessão por um erro recuperável.

use std::fmt;

/// Falha ao processar um comando ou uma consulta.
#[derive(Debug)]
pub enum AppError {
    /// Falha ao carregar o data pack (I/O, JSON/TOML malformado).
    PackLoad(pack::PackError),
    /// O pack carregou, mas tem problemas de validação — a sessão se
    /// recusa a começar com um mundo inconsistente (`docs/03 §7`).
    PackInvalid { issues: Vec<String> },
    /// Nenhuma competição no pack para simular.
    NoCompetitions,
    /// Falha ao (de)serializar ou ler/escrever um save (`persist`).
    Persist(persist::PersistError),
    /// O save é de um pack diferente do que foi passado a
    /// [`crate::GameSession::load`] — carregar aplicaria o histórico de uma
    /// carreira sobre clubes/competições que podem nem existir mais
    /// (`docs/03 §2`: remapeamento entre packs ainda não existe).
    SavePackMismatch {
        save_pack_id: String,
        save_pack_version: String,
        loaded_pack_id: String,
        loaded_pack_version: String,
    },
}

impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            AppError::PackLoad(err) => write!(f, "falha ao carregar pack: {err}"),
            AppError::PackInvalid { issues } => {
                write!(f, "pack com {} problema(s) de validação", issues.len())
            }
            AppError::NoCompetitions => write!(f, "pack não tem nenhuma competição jogável"),
            AppError::Persist(err) => write!(f, "falha ao ler/gravar save: {err}"),
            AppError::SavePackMismatch {
                save_pack_id,
                save_pack_version,
                loaded_pack_id,
                loaded_pack_version,
            } => write!(
                f,
                "save é do pack '{save_pack_id}' v{save_pack_version}, mas foi carregado com \
                 '{loaded_pack_id}' v{loaded_pack_version}"
            ),
        }
    }
}

impl std::error::Error for AppError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            AppError::PackLoad(err) => Some(err),
            AppError::Persist(err) => Some(err),
            AppError::PackInvalid { .. }
            | AppError::NoCompetitions
            | AppError::SavePackMismatch { .. } => None,
        }
    }
}
