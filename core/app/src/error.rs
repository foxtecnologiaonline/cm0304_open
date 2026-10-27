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
    /// O log de comandos do save tem um byte que não corresponde a nenhum
    /// [`crate::Command`] conhecido — save de uma versão mais nova do jogo,
    /// ou corrompido de um jeito que o checksum não pegou.
    InvalidCommandLog { position: usize, tag: u8 },
    /// [`crate::Command::TakeClub`] com um clube que não existe no pack.
    UnknownClub(domain::ClubId),
    /// [`crate::Command::TakeClub`] com perfil de técnico inválido (nome
    /// vazio/longo demais, nacionalidade inexistente, reputação > 100).
    InvalidCoach(&'static str),
    /// [`crate::Command::TakeClub`] depois de já haver um clube assumido.
    ClubAlreadyTaken,
    /// [`crate::Command::TakeClub`] depois da primeira rodada da carreira:
    /// trocar de clube no meio do jogo é demissão/convite (RF-CA-03, M3).
    CareerAlreadyStarted,
    /// Comando de técnico (escalação, instruções) sem nenhum clube sob
    /// comando — falta `Command::TakeClub`.
    NoClubTaken,
    /// Escalação recusada (motivo em português, mostrável direto na UI).
    InvalidLineup(&'static str),
    /// Comando que não vale com uma partida ao vivo em andamento (trocar
    /// formação/escalação, iniciar outra partida).
    MatchInProgress,
    /// Comando de partida ao vivo sem nenhuma partida em andamento
    /// (`Command::StartMatch` primeiro).
    NoLiveMatch,
    /// Substituição recusada (motivo em português, mostrável direto na UI).
    InvalidSubstitution(&'static str),
    /// Compra fora de uma janela de transferência (RF-TR-01).
    TransferWindowClosed,
    /// Proposta mal formada (jogador inexistente ou do próprio clube,
    /// salário ou duração fora da faixa) — nunca uma recusa de negociação,
    /// que é um [`world::OfferOutcome`] normal.
    InvalidOffer(&'static str),
    /// `Command::BeginSeason` com a temporada já começada.
    SeasonAlreadyBegun,
    /// `Command::RespondToOffer` com uma proposta que não existe (já
    /// respondida ou expirada).
    UnknownOffer,
    /// A proposta existe, mas o comprador já não tem o dinheiro.
    OfferNoLongerValid,
    /// Aceitar deixaria o seu elenco abaixo do mínimo (14 jogadores e uma
    /// opção por linha).
    SquadTooSmall,
    /// Comando que só vale para jogador do clube sob comando (treino, ...).
    NotYourPlayer,
    /// `Command::MarkInboxRead` com uma mensagem que não existe (id errado
    /// ou já descartada pelo limite da caixa).
    UnknownMessage,
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
            AppError::MatchInProgress => write!(f, "há uma partida ao vivo em andamento"),
            AppError::NoLiveMatch => write!(f, "nenhuma partida ao vivo em andamento"),
            AppError::InvalidSubstitution(reason) => write!(f, "substituição inválida: {reason}"),
            AppError::TransferWindowClosed => write!(f, "a janela de transferências está fechada"),
            AppError::InvalidOffer(reason) => write!(f, "proposta inválida: {reason}"),
            AppError::SeasonAlreadyBegun => write!(f, "a temporada já começou"),
            AppError::UnknownOffer => write!(f, "essa proposta não existe mais"),
            AppError::OfferNoLongerValid => write!(f, "o comprador já não tem o dinheiro"),
            AppError::SquadTooSmall => {
                write!(
                    f,
                    "vender deixaria o elenco abaixo do mínimo (14 jogadores)"
                )
            }
            AppError::NotYourPlayer => write!(f, "esse jogador não é do seu clube"),
            AppError::UnknownMessage => write!(f, "essa mensagem não existe mais"),
            AppError::NoClubTaken => write!(f, "nenhum clube sob seu comando ainda"),
            AppError::InvalidLineup(reason) => write!(f, "escalação inválida: {reason}"),
            AppError::UnknownClub(club) => write!(f, "o pack não tem o clube {club}"),
            AppError::InvalidCoach(reason) => write!(f, "perfil de técnico inválido: {reason}"),
            AppError::ClubAlreadyTaken => write!(f, "já há um clube sob seu comando"),
            AppError::CareerAlreadyStarted => write!(
                f,
                "só dá para assumir um clube antes da primeira rodada da carreira"
            ),
            AppError::InvalidCommandLog { position, tag } => write!(
                f,
                "log de comandos do save inválido: comando desconhecido (código {tag}) na \
                 posição {position}"
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
            | AppError::SavePackMismatch { .. }
            | AppError::InvalidCommandLog { .. }
            | AppError::UnknownClub(_)
            | AppError::InvalidCoach(_)
            | AppError::ClubAlreadyTaken
            | AppError::CareerAlreadyStarted
            | AppError::NoClubTaken
            | AppError::InvalidLineup(_)
            | AppError::MatchInProgress
            | AppError::NoLiveMatch
            | AppError::InvalidSubstitution(_)
            | AppError::TransferWindowClosed
            | AppError::InvalidOffer(_)
            | AppError::SeasonAlreadyBegun
            | AppError::UnknownOffer
            | AppError::OfferNoLongerValid
            | AppError::SquadTooSmall
            | AppError::NotYourPlayer
            | AppError::UnknownMessage => None,
        }
    }
}
