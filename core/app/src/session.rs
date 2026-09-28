//! `GameSession` — o estado de uma carreira em memória, e o único lugar
//! onde `dispatch`/`query` (`docs/02-arquitetura.md §4`) ganham corpo.
//! Ainda não é o `dispatch`/`query` genérico e serializável que o contrato
//! final descreve (isso pede o log de comandos de `docs/02 §4` e o formato
//! de save de `docs/03 §8`, nenhum dos dois existe ainda — `persist`
//! continua vazio); é a primeira fatia vertical real: **um** comando
//! (avançar a temporada) e **três** consultas, o suficiente para uma UI
//! carregar um pack, rodar temporadas e mostrar uma tabela.

use std::path::Path;

use domain::CompetitionId;

use crate::error::AppError;
use crate::query::{CompetitionSummary, Query, QueryResult, StandingsRow};

/// Comando: muda o estado da sessão. Hoje só existe um — mais chegam junto
/// de elenco/transferências/treino (M2, `docs/07-roadmap.md`).
#[derive(Debug, Clone, Copy)]
pub enum Command {
    /// Simula a próxima temporada inteira (todas as competições do pack) e
    /// aplica promoção/rebaixamento para a seguinte.
    AdvanceSeason,
}

/// Confirmação de que um [`Command`] foi processado, com o suficiente para
/// a UI decidir o que mostrar em seguida — nunca o estado inteiro (isso é
/// papel de uma `Query`).
#[derive(Debug, Clone, Copy)]
pub struct CommandReceipt {
    /// Índice da temporada que acabou de ser simulada (0-based).
    pub season_index: u32,
    pub matches_played: u32,
    /// Quantos clubes mudaram de competição para a próxima temporada.
    pub movements: usize,
}

/// Uma carreira carregada em memória.
#[derive(Debug)]
pub struct GameSession {
    pack: pack::LoadedPack,
    /// Competição atual de cada clube, indexada por id denso — começa como
    /// `pack.clubs[i].competition` e muda com promoção/rebaixamento
    /// (mesmo desenho de `world::season`, só que preservado entre
    /// despachos em vez de interno a uma chamada só).
    membership: Vec<CompetitionId>,
    strengths: Vec<engine::TeamStrength>,
    world_seed: u64,
    /// Uma entrada por temporada já simulada — a fonte de verdade para
    /// toda `Query` sobre o estado atual do mundo.
    history: Vec<world::SeasonResult>,
}

impl GameSession {
    /// Carrega um data pack e começa uma sessão nova, temporada 0, ainda
    /// sem nenhuma partida simulada. Recusa um pack inválido — a sessão
    /// nunca começa com um mundo inconsistente (`docs/03 §7`).
    pub fn new(pack_path: &Path, world_seed: u64) -> Result<Self, AppError> {
        let report = pack::load_and_validate(pack_path).map_err(AppError::PackLoad)?;
        if !report.is_valid() {
            return Err(AppError::PackInvalid {
                issues: report.issues,
            });
        }
        if report.pack.competitions.is_empty() {
            return Err(AppError::NoCompetitions);
        }

        let strengths = world::generate_strengths(&report.pack, world_seed);
        let membership: Vec<CompetitionId> =
            report.pack.clubs.iter().map(|c| c.competition).collect();

        Ok(Self {
            pack: report.pack,
            membership,
            strengths,
            world_seed,
            history: Vec::new(),
        })
    }

    /// Processa um comando, mudando o estado da sessão.
    pub fn dispatch(&mut self, command: Command) -> Result<CommandReceipt, AppError> {
        match command {
            Command::AdvanceSeason => {
                let season_index = self.history.len() as u32;
                let result = world::run_season(
                    &self.pack,
                    &self.membership,
                    &self.strengths,
                    self.world_seed,
                    season_index,
                );

                for &(club, _from, to) in &result.movements {
                    self.membership[club.as_usize()] = to;
                }

                let receipt = CommandReceipt {
                    season_index,
                    matches_played: result.matches_played,
                    movements: result.movements.len(),
                };
                self.history.push(result);
                Ok(receipt)
            }
        }
    }

    /// Responde uma consulta — nunca muda estado (`docs/02 §4`).
    #[must_use]
    pub fn query(&self, query: Query) -> QueryResult {
        match query {
            Query::Competitions => QueryResult::Competitions(self.competitions()),
            Query::Standings { competition } => QueryResult::Standings(self.standings(competition)),
            Query::CurrentSeason => QueryResult::CurrentSeason(self.history.len() as u32),
        }
    }

    fn competitions(&self) -> Vec<CompetitionSummary> {
        self.pack
            .competitions
            .iter()
            .map(|comp| CompetitionSummary {
                id: comp.id,
                name: comp.name.clone(),
                nation_name: self.pack.nation(comp.nation).name.clone(),
            })
            .collect()
    }

    fn standings(&self, competition: CompetitionId) -> Option<Vec<StandingsRow>> {
        let latest = self.history.last()?;
        let (_, table) = latest.tables.iter().find(|(id, _)| *id == competition)?;

        Some(
            table
                .iter()
                .enumerate()
                .map(|(index, row)| StandingsRow {
                    position: index as u32 + 1,
                    club_name: self.pack.club(row.club).name.clone(),
                    played: row.played,
                    wins: row.wins,
                    draws: row.draws,
                    losses: row.losses,
                    goals_for: row.goals_for,
                    goals_against: row.goals_against,
                    points: row.points(),
                })
                .collect(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn example_pack_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/example-two-tier")
    }

    fn tier1_id(session: &GameSession) -> CompetitionId {
        match session.query(Query::Competitions) {
            QueryResult::Competitions(list) => {
                list.iter()
                    .find(|c| c.name.contains("Primeira"))
                    .unwrap()
                    .id
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn nova_sessao_comeca_na_temporada_zero_sem_tabela() {
        let session = GameSession::new(&example_pack_path(), 42).unwrap();
        assert_eq!(
            session.query(Query::CurrentSeason),
            QueryResult::CurrentSeason(0)
        );
    }

    #[test]
    fn pack_inexistente_da_erro_tipado_nao_panic() {
        let err = GameSession::new(Path::new("/tmp/nao-existe-de-verdade"), 42).unwrap_err();
        assert!(matches!(err, AppError::PackLoad(_)));
    }

    #[test]
    fn listar_competicoes_traz_nomes_resolvidos() {
        let session = GameSession::new(&example_pack_path(), 42).unwrap();
        let QueryResult::Competitions(list) = session.query(Query::Competitions) else {
            panic!("esperava QueryResult::Competitions");
        };
        assert_eq!(list.len(), 2);
        assert!(list.iter().all(|c| c.nation_name == "Estrelônia"));
    }

    #[test]
    fn tabela_e_none_antes_de_qualquer_temporada_rodar() {
        let session = GameSession::new(&example_pack_path(), 42).unwrap();
        let id = tier1_id(&session);
        assert_eq!(
            session.query(Query::Standings { competition: id }),
            QueryResult::Standings(None)
        );
    }

    #[test]
    fn avancar_temporada_produz_recibo_e_tabela_consultavel() {
        let mut session = GameSession::new(&example_pack_path(), 42).unwrap();
        let id = tier1_id(&session);

        let receipt = session.dispatch(Command::AdvanceSeason).unwrap();
        assert_eq!(receipt.season_index, 0);
        assert_eq!(receipt.matches_played, 56 * 2); // 2 competições, 56 jogos cada
        assert_eq!(receipt.movements, 4); // 2 promovem + 2 rebaixam, por competição = 4 no total

        assert_eq!(
            session.query(Query::CurrentSeason),
            QueryResult::CurrentSeason(1)
        );

        let QueryResult::Standings(Some(table)) =
            session.query(Query::Standings { competition: id })
        else {
            panic!("esperava tabela preenchida após avançar a temporada");
        };
        assert_eq!(table.len(), 8);
        assert_eq!(table[0].position, 1);
        // Tabela vem ordenada: posição 1 tem pontos >= posição 2, sempre.
        assert!(table[0].points >= table[1].points);
    }

    #[test]
    fn avancar_varias_temporadas_e_deterministico() {
        let mut a = GameSession::new(&example_pack_path(), 7).unwrap();
        let mut b = GameSession::new(&example_pack_path(), 7).unwrap();
        let id = tier1_id(&a);

        for _ in 0..3 {
            a.dispatch(Command::AdvanceSeason).unwrap();
            b.dispatch(Command::AdvanceSeason).unwrap();
        }

        assert_eq!(
            a.query(Query::Standings { competition: id }),
            b.query(Query::Standings { competition: id })
        );
    }
}

/// Testes de propriedade (`docs/08 §1`) — generaliza
/// `avancar_varias_temporadas_e_deterministico` para seeds arbitrárias.
#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;
    use std::path::PathBuf;

    fn example_pack_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/example-two-tier")
    }

    proptest! {
        #[test]
        fn dispatch_e_determinista_para_qualquer_seed(world_seed: u64) {
            let mut a = GameSession::new(&example_pack_path(), world_seed).unwrap();
            let mut b = GameSession::new(&example_pack_path(), world_seed).unwrap();

            let receipt_a = a.dispatch(Command::AdvanceSeason).unwrap();
            let receipt_b = b.dispatch(Command::AdvanceSeason).unwrap();
            prop_assert_eq!(receipt_a.matches_played, receipt_b.matches_played);
            prop_assert_eq!(receipt_a.movements, receipt_b.movements);

            let QueryResult::Competitions(comps) = a.query(Query::Competitions) else {
                unreachable!()
            };
            for comp in comps {
                prop_assert_eq!(
                    a.query(Query::Standings { competition: comp.id }),
                    b.query(Query::Standings { competition: comp.id })
                );
            }
        }
    }
}
