//! Comandos — tudo que muda o estado de uma [`crate::GameSession`] — e sua
//! codificação no log do save (`persist::SaveFile::command_log`).
//!
//! O log é a sequência, na ordem, dos comandos despachados com sucesso; cada
//! um vira `tag (1 byte)` + payload fixo por tag, little-endian
//! (`docs/02 §8.1`). **Nunca renumerar nem reutilizar uma tag**: saves
//! existentes dependem disso. Comando novo = tag nova + payload novo; o
//! formato do arquivo (`persist`) não muda.

use domain::{ClubId, NationId};

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
}

const TAG_ADVANCE_SEASON: u8 = 1;
const TAG_ADVANCE_ROUND: u8 = 2;
const TAG_TAKE_CLUB: u8 = 3;

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

    #[test]
    fn todo_comando_faz_round_trip_no_log() {
        round_trip(&[
            take_club(),
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
        let mut log = Vec::new();
        take_club().encode(&mut log);
        assert_eq!(log[0], 3);
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
        let mut full = Vec::new();
        take_club().encode(&mut full);
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
