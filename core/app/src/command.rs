//! Comandos — tudo que muda o estado de uma [`crate::GameSession`] — e sua
//! codificação no log do save (`persist::SaveFile::command_log`).
//!
//! O log é a sequência, na ordem, dos comandos despachados com sucesso; cada
//! um vira `tag (1 byte)` + payload fixo por tag, little-endian
//! (`docs/02 §8.1`). **Nunca renumerar nem reutilizar uma tag**: saves
//! existentes dependem disso. Comando novo = tag nova + payload novo; o
//! formato do arquivo (`persist`) não muda.

use ai::Formation;
use domain::{ClubId, Money, NationId, PlayerId};
use world::{Mentality, Pressing, Training};

use crate::error::AppError;

/// Tamanho máximo do nome do técnico, em bytes UTF-8 — cabe num prefixo de
/// 1 byte no log e numa linha de UI de celular.
pub const MAX_COACH_NAME_BYTES: usize = 48;

/// Perfil do técnico (RF-CA-01, `docs/01 §2.8`) — fatia mínima: nome,
/// nacionalidade e reputação inicial. "Experiência prévia" do requisito
/// fica de fora: sem histórico de carreira ainda (RF-CL-08, M5), seria um
/// número sem consumidor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoachProfile {
    pub name: String,
    pub nation: NationId,
    /// 0-100. Ainda sem consumidor (diretoria/ofertas de emprego, RF-CA-02/03,
    /// são M3) — guardada para não precisar mudar o log depois.
    pub reputation: u8,
}

/// Comando: muda o estado da sessão. Mais chegam junto de
/// elenco/transferências/treino (M2, `docs/07-roadmap.md`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Command {
    /// Simula o que falta da temporada atual (ou uma inteira, se ainda não
    /// começou) e aplica promoção/rebaixamento para a seguinte. Equivale a
    /// despachar [`Command::AdvanceRound`] até a temporada acabar — o
    /// recibo é a soma dos recibos das rodadas.
    AdvanceSeason,
    /// Joga uma rodada (dia de jogos) de todas as ligas. O primeiro
    /// `AdvanceRound` de uma temporada faz antes a pré-temporada (dívida,
    /// folha, mercado, lesões, calendário); o último faz depois o fechamento
    /// (acesso/queda, copa, premiação, TV, patrocínio, fadiga, progressão,
    /// contratos) e marca [`crate::CommandReceipt::season_completed`].
    AdvanceRound,
    /// Assume o comando de um clube: a partir daqui as decisões do técnico
    /// (escalação, mercado, treino) valem para ele, e a IA deixa de decidir
    /// por esse clube. Só vale **uma vez**, **antes da primeira rodada** da
    /// carreira. Sem `TakeClub` a sessão é só observadora (modo headless:
    /// `calibrate`, `play`, golden master).
    TakeClub { club: ClubId, coach: CoachProfile },
    /// Muda a formação mantendo a escalação **automática** (o melhor onze
    /// disponível a cada rodada) — descarta uma escalação manual anterior.
    /// Exige um clube sob comando.
    SetFormation { formation: Formation },
    /// Escalação manual: os 11 titulares na ordem dos espaços da formação
    /// (goleiro, defensores, meio-campistas, atacantes). Todos do clube,
    /// distintos, sem lesão nem suspensão na próxima rodada; jogar fora da
    /// posição natural é permitido, mas rende menos
    /// (`world::position_fit_permille`). Se alguém ficar indisponível
    /// depois, o melhor substituto entra sozinho na rodada.
    SetLineup {
        formation: Formation,
        starters: Vec<PlayerId>,
    },
    /// Mentalidade e pressão do time (RF-PA-02, subconjunto — ver
    /// `world::setup`). Valem até serem trocadas.
    SetInstructions {
        mentality: Mentality,
        pressing: Pressing,
    },
    /// Começa a próxima partida do clube sob comando **ao vivo**: faz a
    /// pré-temporada se a temporada ainda não começou, prepara a rodada e
    /// deixa a partida parada no minuto 0. Depois, [`Command::PlayMinutes`],
    /// [`Command::Substitute`] e [`Command::SetInstructions`] conduzem o
    /// jogo; [`Command::AdvanceRound`] termina o que faltar e conclui a
    /// rodada (as outras partidas são simuladas nesse momento). Se o clube
    /// não joga nesta rodada, nada fica ao vivo.
    StartMatch,
    /// Joga até `minutes` minutos da partida ao vivo (menos, se ela acabar).
    PlayMinutes { minutes: u8 },
    /// Substitui `out` (em campo) por `incoming` (no banco) na partida ao
    /// vivo — até 3 por partida, valendo do próximo minuto.
    Substitute { out: PlayerId, incoming: PlayerId },
    /// Faz a pré-temporada (dívida, folha, mercado da IA — sem mexer no
    /// clube sob comando —, lesões, calendário) sem jogar rodada nenhuma,
    /// abrindo a janela de verão para o técnico negociar **antes** da
    /// primeira rodada. `AdvanceRound`/`StartMatch` fazem isto sozinhos se
    /// ainda não foi feito. Erro se a temporada já começou.
    BeginSeason,
    /// Proposta de compra de um jogador de outro clube, só com a janela
    /// aberta: `fee` ao clube vendedor e, ao jogador, `wage_permille` (em
    /// milésimos do salário-base do CA dele; `1000` = base) por `seasons`
    /// temporadas. Responde com um [`world::OfferOutcome`] em
    /// `CommandReceipt::offer` — recusa e contraproposta não são erro, e o
    /// técnico ajusta e tenta de novo.
    MakeOffer {
        player: PlayerId,
        fee: Money,
        wage_permille: u16,
        seasons: u8,
    },
    /// Renova o contrato de um jogador do próprio clube, a qualquer
    /// momento (resposta em `CommandReceipt::offer`).
    RenewContract {
        player: PlayerId,
        wage_permille: u16,
        seasons: u8,
    },
    /// Responde a uma proposta da IA por um jogador seu
    /// (`Query::IncomingOffers`): `accept` fecha a venda ao preço ofertado
    /// (só com a janela aberta e sem deixar o elenco abaixo do mínimo);
    /// recusar a descarta.
    RespondToOffer { offer: u32, accept: bool },
    /// Define a intensidade de treino de um jogador do clube sob comando
    /// (RF-TC-01): treino intenso acelera a evolução de CA (mais nos
    /// jovens) ao custo de mais lesão e menos condição; o leve faz o
    /// inverso. Vale até ser trocada e é aplicada no fim da temporada.
    SetTraining {
        player: PlayerId,
        training: Training,
    },
    /// [`Command::SetTraining`] para o elenco inteiro do clube.
    SetTeamTraining { training: Training },
}

const TAG_ADVANCE_SEASON: u8 = 1;
const TAG_ADVANCE_ROUND: u8 = 2;
const TAG_TAKE_CLUB: u8 = 3;
const TAG_SET_FORMATION: u8 = 4;
const TAG_SET_LINEUP: u8 = 5;
const TAG_SET_INSTRUCTIONS: u8 = 6;
const TAG_START_MATCH: u8 = 7;
const TAG_PLAY_MINUTES: u8 = 8;
const TAG_SUBSTITUTE: u8 = 9;
const TAG_BEGIN_SEASON: u8 = 10;
const TAG_MAKE_OFFER: u8 = 11;
const TAG_RENEW_CONTRACT: u8 = 12;
const TAG_RESPOND_TO_OFFER: u8 = 13;
const TAG_SET_TRAINING: u8 = 14;
const TAG_SET_TEAM_TRAINING: u8 = 15;

impl Command {
    /// Acrescenta a codificação deste comando ao log.
    pub(crate) fn encode(&self, out: &mut Vec<u8>) {
        match self {
            Command::AdvanceSeason => out.push(TAG_ADVANCE_SEASON),
            Command::AdvanceRound => out.push(TAG_ADVANCE_ROUND),
            Command::TakeClub { club, coach } => {
                out.push(TAG_TAKE_CLUB);
                out.extend_from_slice(&club.index().to_le_bytes());
                out.extend_from_slice(&coach.nation.index().to_le_bytes());
                out.push(coach.reputation);
                let name = coach.name.as_bytes();
                // `dispatch` já validou `name.len() <= MAX_COACH_NAME_BYTES`.
                out.push(name.len() as u8);
                out.extend_from_slice(name);
            }
            Command::SetFormation { formation } => {
                out.push(TAG_SET_FORMATION);
                out.push(formation.code());
            }
            Command::SetLineup {
                formation,
                starters,
            } => {
                out.push(TAG_SET_LINEUP);
                out.push(formation.code());
                // `dispatch` já validou `starters.len() == 11`.
                for id in starters {
                    out.extend_from_slice(&id.index().to_le_bytes());
                }
            }
            Command::SetInstructions {
                mentality,
                pressing,
            } => {
                out.push(TAG_SET_INSTRUCTIONS);
                out.push(mentality.code());
                out.push(pressing.code());
            }
            Command::StartMatch => out.push(TAG_START_MATCH),
            Command::PlayMinutes { minutes } => {
                out.push(TAG_PLAY_MINUTES);
                out.push(*minutes);
            }
            Command::Substitute {
                out: leaving,
                incoming,
            } => {
                out.push(TAG_SUBSTITUTE);
                out.extend_from_slice(&leaving.index().to_le_bytes());
                out.extend_from_slice(&incoming.index().to_le_bytes());
            }
            Command::BeginSeason => out.push(TAG_BEGIN_SEASON),
            Command::MakeOffer {
                player,
                fee,
                wage_permille,
                seasons,
            } => {
                out.push(TAG_MAKE_OFFER);
                out.extend_from_slice(&player.index().to_le_bytes());
                out.extend_from_slice(&fee.cents().to_le_bytes());
                out.extend_from_slice(&wage_permille.to_le_bytes());
                out.push(*seasons);
            }
            Command::RenewContract {
                player,
                wage_permille,
                seasons,
            } => {
                out.push(TAG_RENEW_CONTRACT);
                out.extend_from_slice(&player.index().to_le_bytes());
                out.extend_from_slice(&wage_permille.to_le_bytes());
                out.push(*seasons);
            }
            Command::RespondToOffer { offer, accept } => {
                out.push(TAG_RESPOND_TO_OFFER);
                out.extend_from_slice(&offer.to_le_bytes());
                out.push(u8::from(*accept));
            }
            Command::SetTraining { player, training } => {
                out.push(TAG_SET_TRAINING);
                out.extend_from_slice(&player.index().to_le_bytes());
                out.push(training.code());
            }
            Command::SetTeamTraining { training } => {
                out.push(TAG_SET_TEAM_TRAINING);
                out.push(training.code());
            }
        }
    }

    /// Lê o próximo comando de `log` a partir de `*pos`, avançando-o.
    /// Devolve [`AppError::InvalidCommandLog`] (nunca panica) para tag
    /// desconhecida ou payload cortado/inválido.
    pub(crate) fn decode(log: &[u8], pos: &mut usize) -> Result<Command, AppError> {
        let start = *pos;
        let tag = *log.get(start).ok_or(AppError::InvalidCommandLog {
            position: start,
            tag: 0,
        })?;
        let bad = || AppError::InvalidCommandLog {
            position: start,
            tag,
        };
        let mut cursor = start + 1;
        let mut take = |n: usize| -> Result<&[u8], AppError> {
            let end = cursor.checked_add(n).ok_or_else(bad)?;
            let slice = log.get(cursor..end).ok_or_else(bad)?;
            cursor = end;
            Ok(slice)
        };
        let command = match tag {
            TAG_ADVANCE_SEASON => Command::AdvanceSeason,
            TAG_ADVANCE_ROUND => Command::AdvanceRound,
            TAG_TAKE_CLUB => {
                let club = u32::from_le_bytes(take(4)?.try_into().map_err(|_| bad())?);
                let nation = u32::from_le_bytes(take(4)?.try_into().map_err(|_| bad())?);
                let reputation = take(1)?[0];
                let len = take(1)?[0] as usize;
                let name = String::from_utf8(take(len)?.to_vec()).map_err(|_| bad())?;
                Command::TakeClub {
                    club: ClubId::new(club),
                    coach: CoachProfile {
                        name,
                        nation: NationId::new(nation),
                        reputation,
                    },
                }
            }
            TAG_SET_FORMATION => Command::SetFormation {
                formation: Formation::from_code(take(1)?[0]).ok_or_else(bad)?,
            },
            TAG_SET_LINEUP => {
                let formation = Formation::from_code(take(1)?[0]).ok_or_else(bad)?;
                let mut starters = Vec::with_capacity(11);
                for _ in 0..11 {
                    let id = u32::from_le_bytes(take(4)?.try_into().map_err(|_| bad())?);
                    starters.push(PlayerId::new(id));
                }
                Command::SetLineup {
                    formation,
                    starters,
                }
            }
            TAG_SET_INSTRUCTIONS => Command::SetInstructions {
                mentality: Mentality::from_code(take(1)?[0]).ok_or_else(bad)?,
                pressing: Pressing::from_code(take(1)?[0]).ok_or_else(bad)?,
            },
            TAG_START_MATCH => Command::StartMatch,
            TAG_PLAY_MINUTES => Command::PlayMinutes {
                minutes: take(1)?[0],
            },
            TAG_SUBSTITUTE => {
                let leaving = u32::from_le_bytes(take(4)?.try_into().map_err(|_| bad())?);
                let incoming = u32::from_le_bytes(take(4)?.try_into().map_err(|_| bad())?);
                Command::Substitute {
                    out: PlayerId::new(leaving),
                    incoming: PlayerId::new(incoming),
                }
            }
            TAG_BEGIN_SEASON => Command::BeginSeason,
            TAG_MAKE_OFFER => {
                let player = u32::from_le_bytes(take(4)?.try_into().map_err(|_| bad())?);
                let fee = i64::from_le_bytes(take(8)?.try_into().map_err(|_| bad())?);
                let wage = u16::from_le_bytes(take(2)?.try_into().map_err(|_| bad())?);
                let seasons = take(1)?[0];
                Command::MakeOffer {
                    player: PlayerId::new(player),
                    fee: Money::from_cents(fee),
                    wage_permille: wage,
                    seasons,
                }
            }
            TAG_RENEW_CONTRACT => {
                let player = u32::from_le_bytes(take(4)?.try_into().map_err(|_| bad())?);
                let wage = u16::from_le_bytes(take(2)?.try_into().map_err(|_| bad())?);
                let seasons = take(1)?[0];
                Command::RenewContract {
                    player: PlayerId::new(player),
                    wage_permille: wage,
                    seasons,
                }
            }
            TAG_RESPOND_TO_OFFER => {
                let offer = u32::from_le_bytes(take(4)?.try_into().map_err(|_| bad())?);
                let accept = match take(1)?[0] {
                    0 => false,
                    1 => true,
                    _ => return Err(bad()),
                };
                Command::RespondToOffer { offer, accept }
            }
            TAG_SET_TRAINING => {
                let player = u32::from_le_bytes(take(4)?.try_into().map_err(|_| bad())?);
                Command::SetTraining {
                    player: PlayerId::new(player),
                    training: Training::from_code(take(1)?[0]).ok_or_else(bad)?,
                }
            }
            TAG_SET_TEAM_TRAINING => Command::SetTeamTraining {
                training: Training::from_code(take(1)?[0]).ok_or_else(bad)?,
            },
            _ => return Err(bad()),
        };
        *pos = cursor;
        Ok(command)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn take_club() -> Command {
        Command::TakeClub {
            club: ClubId::new(5),
            coach: CoachProfile {
                name: "Zé Treinador".to_string(),
                nation: NationId::new(0),
                reputation: 40,
            },
        }
    }

    fn round_trip(commands: &[Command]) {
        let mut log = Vec::new();
        for c in commands {
            c.encode(&mut log);
        }
        let mut pos = 0;
        let mut decoded = Vec::new();
        while pos < log.len() {
            decoded.push(Command::decode(&log, &mut pos).unwrap());
        }
        assert_eq!(decoded, commands);
    }

    fn set_lineup() -> Command {
        Command::SetLineup {
            formation: Formation::F433,
            starters: (0..11).map(|i| PlayerId::new(i * 3)).collect(),
        }
    }

    #[test]
    fn todo_comando_faz_round_trip_no_log() {
        round_trip(&[
            Command::SetFormation {
                formation: Formation::F532,
            },
            set_lineup(),
            Command::SetInstructions {
                mentality: Mentality::Attacking,
                pressing: Pressing::High,
            },
            take_club(),
            Command::StartMatch,
            Command::PlayMinutes { minutes: 45 },
            Command::Substitute {
                out: PlayerId::new(3),
                incoming: PlayerId::new(77),
            },
            Command::BeginSeason,
            Command::MakeOffer {
                player: PlayerId::new(5),
                fee: Money::from_cents(123_456_789),
                wage_permille: 1337,
                seasons: 3,
            },
            Command::RenewContract {
                player: PlayerId::new(6),
                wage_permille: 900,
                seasons: 2,
            },
            Command::RespondToOffer {
                offer: 41,
                accept: true,
            },
            Command::RespondToOffer {
                offer: 7,
                accept: false,
            },
            Command::SetTraining {
                player: PlayerId::new(9),
                training: Training::Intense,
            },
            Command::SetTeamTraining {
                training: Training::Light,
            },
            Command::AdvanceRound,
            Command::AdvanceSeason,
            Command::AdvanceRound,
        ]);
    }

    #[test]
    fn tags_sao_estaveis() {
        // Mudar estes valores quebra todo save existente.
        let mut log = Vec::new();
        Command::AdvanceSeason.encode(&mut log);
        Command::AdvanceRound.encode(&mut log);
        assert_eq!(log, vec![1, 2]);
        let tag_of = |c: Command| {
            let mut log = Vec::new();
            c.encode(&mut log);
            log[0]
        };
        assert_eq!(tag_of(take_club()), 3);
        assert_eq!(
            tag_of(Command::SetFormation {
                formation: Formation::F442
            }),
            4
        );
        assert_eq!(tag_of(set_lineup()), 5);
        assert_eq!(
            tag_of(Command::RespondToOffer {
                offer: 1,
                accept: true
            }),
            13
        );
        assert_eq!(
            tag_of(Command::SetTraining {
                player: PlayerId::new(1),
                training: Training::Normal
            }),
            14
        );
        assert_eq!(
            tag_of(Command::SetTeamTraining {
                training: Training::Normal
            }),
            15
        );
        assert_eq!(tag_of(Command::BeginSeason), 10);
        assert_eq!(
            tag_of(Command::MakeOffer {
                player: PlayerId::new(1),
                fee: Money::ZERO,
                wage_permille: 1000,
                seasons: 1
            }),
            11
        );
        assert_eq!(
            tag_of(Command::RenewContract {
                player: PlayerId::new(1),
                wage_permille: 1000,
                seasons: 1
            }),
            12
        );
        assert_eq!(tag_of(Command::StartMatch), 7);
        assert_eq!(tag_of(Command::PlayMinutes { minutes: 1 }), 8);
        assert_eq!(
            tag_of(Command::Substitute {
                out: PlayerId::new(1),
                incoming: PlayerId::new(2)
            }),
            9
        );
        assert_eq!(
            tag_of(Command::SetInstructions {
                mentality: Mentality::Balanced,
                pressing: Pressing::Normal
            }),
            6
        );
    }

    #[test]
    fn tag_desconhecida_e_payload_cortado_dao_erro_tipado_sem_panic() {
        let mut pos = 0;
        assert!(matches!(
            Command::decode(&[99], &mut pos),
            Err(AppError::InvalidCommandLog {
                position: 0,
                tag: 99
            })
        ));
        for command in [take_club(), set_lineup()] {
            let mut full = Vec::new();
            command.encode(&mut full);
            for cut in 1..full.len() {
                let mut pos = 0;
                assert!(
                    matches!(
                        Command::decode(&full[..cut], &mut pos),
                        Err(AppError::InvalidCommandLog { .. })
                    ),
                    "corte em {cut} deveria falhar"
                );
            }
        }
        // Códigos de enum desconhecidos também são erro tipado.
        for bad in [[4u8, 99], [6, 9], [6, 0]] {
            let mut pos = 0;
            let result = Command::decode(&bad, &mut pos);
            if bad == [6, 0] {
                assert!(result.is_err(), "falta o byte de pressão");
            } else {
                assert!(matches!(result, Err(AppError::InvalidCommandLog { .. })));
            }
        }
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// `decode` nunca panica, para nenhuma sequência de bytes — o log
        /// vem de um arquivo em disco (`docs/02 §9.2`).
        #[test]
        fn decode_nunca_panica_para_bytes_arbitrarios(bytes in prop::collection::vec(any::<u8>(), 0..64)) {
            let mut pos = 0;
            while pos < bytes.len() {
                if Command::decode(&bytes, &mut pos).is_err() {
                    break;
                }
            }
        }

        #[test]
        fn take_club_faz_round_trip_para_qualquer_perfil_valido(
            club: u32,
            nation: u32,
            reputation in 0u8..=100,
            name in "[a-zA-Z ]{1,48}",
        ) {
            let cmd = Command::TakeClub {
                club: ClubId::new(club),
                coach: CoachProfile { name, nation: NationId::new(nation), reputation },
            };
            let mut log = Vec::new();
            cmd.encode(&mut log);
            let mut pos = 0;
            prop_assert_eq!(Command::decode(&log, &mut pos).unwrap(), cmd);
            prop_assert_eq!(pos, log.len());
        }
    }
}
