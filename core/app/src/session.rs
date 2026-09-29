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
    /// Identidade do pack (`pack::PackManifest::id`/`version`) — guardada
    /// para gravar no save e para `load` recusar reabrir a carreira contra
    /// um pack diferente (`AppError::SavePackMismatch`).
    pack_id: String,
    pack_version: String,
    /// Competição atual de cada clube, indexada por id denso — começa como
    /// `pack.clubs[i].competition` e muda com promoção/rebaixamento
    /// (mesmo desenho de `world::season`, só que preservado entre
    /// despachos em vez de interno a uma chamada só).
    membership: Vec<CompetitionId>,
    strengths: Vec<engine::TeamStrength>,
    /// CA e idade de cada jogador nesta carreira (`world::progression`) —
    /// separado de `pack.players`, que nunca muda. Recalculado em força de
    /// clube a cada `AdvanceSeason` (`world::generate_strengths_from_roster`)
    /// e envelhecido/progredido ao final de cada uma
    /// (`world::advance_season`), então a força de um clube muda de
    /// temporada pra temporada mesmo sem promoção/rebaixamento.
    roster: Vec<world::PlayerState>,
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

        let roster = world::initial_roster(&report.pack);
        let strengths = world::generate_strengths_from_roster(&report.pack, &roster, world_seed);
        let membership: Vec<CompetitionId> =
            report.pack.clubs.iter().map(|c| c.competition).collect();

        Ok(Self {
            pack: report.pack,
            pack_id: report.manifest.id,
            pack_version: report.manifest.version,
            membership,
            strengths,
            roster,
            world_seed,
            history: Vec::new(),
        })
    }

    /// Reabre uma carreira salva: carrega o pack do zero e **repete** cada
    /// temporada já jogada via [`Self::dispatch`], em vez de restaurar um
    /// instantâneo. Funciona porque a sessão inteira é uma função pura de
    /// `(pack, world_seed, quantos `AdvanceSeason` já rodaram)`
    /// (`ADR 0002`) — é a mesma ideia por trás do "log de comandos" de
    /// `docs/02 §4`, só que hoje o log é um contador porque existe um único
    /// tipo de comando sem parâmetros (ver `persist::save` para o porquê
    /// disso não ser um atalho, e o que muda quando `Command` crescer).
    pub fn load(pack_path: &Path, save: &persist::SaveFile) -> Result<Self, AppError> {
        let mut session = Self::new(pack_path, save.world_seed)?;
        if session.pack_id != save.pack_id || session.pack_version != save.pack_version {
            return Err(AppError::SavePackMismatch {
                save_pack_id: save.pack_id.clone(),
                save_pack_version: save.pack_version.clone(),
                loaded_pack_id: session.pack_id,
                loaded_pack_version: session.pack_version,
            });
        }
        for _ in 0..save.seasons_advanced {
            session.dispatch(Command::AdvanceSeason)?;
        }
        Ok(session)
    }

    /// Lê um arquivo de save em `save_path` e reabre a carreira contra o
    /// pack em `pack_path` — o par de operações que `managerfc-cli
    /// load`/uma futura UI de fato chamam; [`Self::load`] (a partir de um
    /// [`persist::SaveFile`] já em memória) existe separado para ser
    /// testável sem tocar disco.
    pub fn load_from_path(pack_path: &Path, save_path: &Path) -> Result<Self, AppError> {
        let save = persist::load_from_path(save_path).map_err(AppError::Persist)?;
        Self::load(pack_path, &save)
    }

    /// Extrai o [`persist::SaveFile`] desta sessão — puro, não toca disco
    /// (ver [`Self::save_to_path`] para isso).
    #[must_use]
    pub fn to_save(&self) -> persist::SaveFile {
        persist::SaveFile {
            pack_id: self.pack_id.clone(),
            pack_version: self.pack_version.clone(),
            world_seed: self.world_seed,
            seasons_advanced: self.history.len() as u32,
        }
    }

    /// Grava esta sessão em disco (escrita atômica — `persist::save_to_path`).
    pub fn save_to_path(&self, path: &Path) -> Result<(), AppError> {
        persist::save_to_path(path, &self.to_save()).map_err(AppError::Persist)
    }

    /// Processa um comando, mudando o estado da sessão.
    pub fn dispatch(&mut self, command: Command) -> Result<CommandReceipt, AppError> {
        match command {
            Command::AdvanceSeason => {
                let season_index = self.history.len() as u32;
                // Recalcula a força a partir do roster atual — reflete
                // qualquer progressão aplicada ao final da temporada
                // anterior (`world::advance_season`, abaixo). Em `season_index
                // == 0` é idêntico ao que `Self::new` já calculou.
                self.strengths = world::generate_strengths_from_roster(
                    &self.pack,
                    &self.roster,
                    self.world_seed,
                );
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
                world::advance_season(&mut self.roster, self.world_seed, season_index);

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

    #[test]
    fn to_save_reflete_seed_e_temporadas_avancadas() {
        let mut session = GameSession::new(&example_pack_path(), 99).unwrap();
        session.dispatch(Command::AdvanceSeason).unwrap();
        session.dispatch(Command::AdvanceSeason).unwrap();

        let save = session.to_save();
        assert_eq!(save.pack_id, "example.two-tier");
        assert_eq!(save.world_seed, 99);
        assert_eq!(save.seasons_advanced, 2);
    }

    #[test]
    fn avancar_temporadas_envelhece_o_elenco_e_muda_a_forca_dos_clubes() {
        // Teste de caixa branca (acessa campos privados — este módulo de
        // teste é filho de `session`): prova que a progressão
        // (`world::advance_season`) está de fato ligada ao dispatch, não só
        // existe isolada em `world`. Sem isto, seria fácil o CA do roster
        // nunca evoluir e ninguém perceber — as tabelas continuariam
        // plausíveis mesmo com a força de clube congelada.
        let mut session = GameSession::new(&example_pack_path(), 5).unwrap();
        let initial_ages: Vec<i32> = session.roster.iter().map(|p| p.age_years).collect();
        let initial_strengths = session.strengths.clone();

        for _ in 0..5 {
            session.dispatch(Command::AdvanceSeason).unwrap();
        }

        let final_ages: Vec<i32> = session.roster.iter().map(|p| p.age_years).collect();
        assert!(
            initial_ages
                .iter()
                .zip(&final_ages)
                .all(|(before, after)| *after == before + 5),
            "cada jogador deveria ter envelhecido exatamente 5 anos"
        );
        assert_ne!(
            initial_strengths, session.strengths,
            "força dos clubes deveria mudar após 5 temporadas de progressão"
        );
    }

    #[test]
    fn load_a_partir_do_save_reproduz_a_mesma_tabela_por_replay() {
        let mut original = GameSession::new(&example_pack_path(), 7).unwrap();
        let id = tier1_id(&original);
        for _ in 0..3 {
            original.dispatch(Command::AdvanceSeason).unwrap();
        }
        let save = original.to_save();

        let reloaded = GameSession::load(&example_pack_path(), &save).unwrap();

        assert_eq!(
            reloaded.query(Query::CurrentSeason),
            original.query(Query::CurrentSeason)
        );
        assert_eq!(
            reloaded.query(Query::Standings { competition: id }),
            original.query(Query::Standings { competition: id })
        );
    }

    #[test]
    fn save_to_path_e_load_from_path_fazem_round_trip_em_disco() {
        let dir =
            std::env::temp_dir().join(format!("managerfc-app-save-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let save_path = dir.join("slot0.cm0304save");

        let mut original = GameSession::new(&example_pack_path(), 3).unwrap();
        let id = tier1_id(&original);
        original.dispatch(Command::AdvanceSeason).unwrap();
        original.save_to_path(&save_path).unwrap();

        let reloaded = GameSession::load_from_path(&example_pack_path(), &save_path).unwrap();
        assert_eq!(
            reloaded.query(Query::Standings { competition: id }),
            original.query(Query::Standings { competition: id })
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn load_recusa_save_de_outro_pack() {
        let save = persist::SaveFile {
            pack_id: "outro.pack".to_string(),
            pack_version: "9.9.9".to_string(),
            world_seed: 1,
            seasons_advanced: 0,
        };
        let err = GameSession::load(&example_pack_path(), &save).unwrap_err();
        assert!(matches!(err, AppError::SavePackMismatch { .. }));
    }

    #[test]
    fn load_from_path_propaga_erro_tipado_de_persist_para_save_ausente() {
        let err = GameSession::load_from_path(
            &example_pack_path(),
            Path::new("/tmp/managerfc-save-que-nao-existe.cm0304save"),
        )
        .unwrap_err();
        assert!(matches!(err, AppError::Persist(_)));
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

        /// Generaliza `load_a_partir_do_save_reproduz_a_mesma_tabela_por_replay`
        /// para seed e número de temporadas arbitrários (dentro de uma faixa
        /// pequena, já que cada temporada roda partidas de verdade).
        #[test]
        fn save_e_load_reproduzem_a_mesma_sessao_para_qualquer_seed_e_temporadas(
            world_seed: u64,
            seasons in 0u32..4,
        ) {
            let mut original = GameSession::new(&example_pack_path(), world_seed).unwrap();
            for _ in 0..seasons {
                original.dispatch(Command::AdvanceSeason).unwrap();
            }
            let save = original.to_save();
            let reloaded = GameSession::load(&example_pack_path(), &save).unwrap();

            prop_assert_eq!(
                reloaded.query(Query::CurrentSeason),
                original.query(Query::CurrentSeason)
            );
            let QueryResult::Competitions(comps) = original.query(Query::Competitions) else {
                unreachable!()
            };
            for comp in comps {
                prop_assert_eq!(
                    reloaded.query(Query::Standings { competition: comp.id }),
                    original.query(Query::Standings { competition: comp.id })
                );
            }
        }
    }
}
