//! Erros de (de)serialização e I/O de um save (`docs/03-modelo-de-dados.md §8`).
//!
//! Mesma filosofia de `pack::PackError` (`docs/02 §9.2`): um save corrompido,
//! truncado ou de um `schema_version` que este binário não entende nunca
//! deve panicar — vira um `Result` tipado que a UI transforma em mensagem
//! acionável ("save corrompido, restaurando do autosave anterior"), nunca
//! em "abrir meio corrompido" (RNF-25, `docs/02 §8.3`).

use std::fmt;
use std::path::PathBuf;

/// Falha ao codificar, decodificar ou ler/escrever um save em disco.
#[derive(Debug)]
pub enum PersistError {
    /// Os bytes acabaram no meio de um campo — arquivo truncado (disco
    /// cheio na escrita anterior, cópia incompleta, etc.).
    Truncated,
    /// Os primeiros bytes não são o `MAGIC` esperado — não é um save deste
    /// jogo (ou é de um jogo diferente, ou é lixo).
    BadMagic,
    /// `schema_version` do arquivo é maior do que este binário sabe ler.
    /// Nunca lemos "melhor esforço" um schema desconhecido — dado
    /// parcialmente entendido é pior que um erro claro (`docs/02 §8.3`).
    UnsupportedSchemaVersion { found: u16 },
    /// O checksum gravado não bate com o dos bytes lidos — corrupção.
    ChecksumMismatch,
    /// Uma string do save não é UTF-8 válido.
    InvalidUtf8,
    /// Um campo de texto (ex.: id do pack) excede o limite de tamanho do
    /// formato (65535 bytes, o maior que cabe no prefixo de comprimento
    /// `u16`) — nunca deveria acontecer com dado do próprio jogo, mas
    /// `encode` prefere devolver erro a truncar silenciosamente.
    FieldTooLong { field: &'static str, len: usize },
    /// Falha de E/S lendo ou escrevendo o arquivo de save.
    Io {
        path: PathBuf,
        source: std::io::Error,
    },
}

impl fmt::Display for PersistError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            PersistError::Truncated => write!(f, "save truncado — arquivo incompleto"),
            PersistError::BadMagic => write!(f, "não é um arquivo de save do ManagerFC"),
            PersistError::UnsupportedSchemaVersion { found } => write!(
                f,
                "schema de save v{found} não é suportado por este binário"
            ),
            PersistError::ChecksumMismatch => {
                write!(f, "checksum do save não confere — arquivo corrompido")
            }
            PersistError::InvalidUtf8 => write!(f, "save contém texto que não é UTF-8 válido"),
            PersistError::FieldTooLong { field, len } => write!(
                f,
                "campo '{field}' do save tem {len} bytes, acima do limite de 65535"
            ),
            PersistError::Io { path, source } => {
                write!(f, "erro de E/S em {}: {source}", path.display())
            }
        }
    }
}

impl std::error::Error for PersistError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            PersistError::Io { source, .. } => Some(source),
            _ => None,
        }
    }
}
