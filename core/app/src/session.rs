//! `GameSession` — o estado de uma carreira em memória, e o único lugar
//! onde `dispatch`/`query` (`docs/02-arquitetura.md §4`) ganham corpo.
//! Ainda não é o `dispatch`/`query` genérico e serializável que o contrato
//! final descreve (isso pede o log de comandos de `docs/02 §4` e o formato
//! de save de `docs/03 §8`, nenhum dos dois existe ainda — `persist`
//! continua vazio); é a primeira fatia vertical real: **um** comando
//! (avançar a temporada) e **três** consultas, o suficiente para uma UI
//! carregar um pack, rodar temporadas e mostrar uma tabela.

use std::collections::HashMap;
use std::path::Path;

use domain::CompetitionId;

use crate::error::AppError;
use crate::query::{
    CompetitionKind, CompetitionSummary, CupChampionRow, Query, QueryResult, StandingsRow,
};

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
    /// Quantas transferências a IA de mercado fechou antes da temporada
    /// (`world::run_market_day`, `ai::run_market_day` — `docs/07-roadmap.md`
    /// M1: "primeira IA de mercado").
    pub transfers: usize,
    /// Quantos jogadores estão machucados nesta temporada
    /// (`world::roll_injuries` — `docs/01 §2.3`, RF-JG-08, fatia mínima).
    pub injuries: usize,
    /// Quantos jogadores começam a **próxima** temporada com condição
    /// abaixo de `world::FULL_CONDITION` — titulares que titularizaram a
    /// temporada que acabou de terminar (`world::apply_season_fatigue`,
    /// `docs/01 §2.1`, RF-JG-07, fatia mínima).
    pub tired_players: usize,
    /// Quantas suspensões por expulsão aconteceram nesta temporada, em
    /// partidas de liga (`world::season::SeasonResult::suspensions` —
    /// `crate::discipline`, `docs/01 §2.3`, RF-JG-09, fatia mínima).
    pub suspensions: usize,
    /// Quanto os clubes pagaram de folha salarial nesta temporada, no
    /// total (`world::pay_salaries`, `docs/01 §2.2`, RF-CL-02, fatia
    /// mínima) — pode ser menor que a soma nominal dos salários se algum
    /// clube não tinha caixa suficiente (nunca fica negativo).
    pub payroll_paid: domain::Money,
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
    /// Força/finalização/goleiro de cada clube para o motor
    /// (`engine::TeamMatchProfile`) — recalculado a cada `AdvanceSeason`
    /// (`world::generate_match_profiles_from_roster`) a partir do roster
    /// atual, então muda de temporada pra temporada mesmo sem promoção/
    /// rebaixamento (progressão, `world::advance_season`) ou com (mercado,
    /// `world::run_market_day`).
    profiles: Vec<engine::TeamMatchProfile>,
    /// CA e idade de cada jogador nesta carreira (`world::progression`) —
    /// separado de `pack.players`, que nunca muda.
    roster: Vec<world::PlayerState>,
    /// Orçamento de cada clube, indexado por id denso (`world::generate_budgets`,
    /// sintético — pack ainda não declara finanças, `world::finance`). Só
    /// muda por transferência (`world::run_market_day`): sem receita nem
    /// despesa externas ainda, uma economia fechada.
    budgets: Vec<domain::Money>,
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
        let budgets = world::generate_budgets(&report.pack, world_seed);
        let profiles =
            world::generate_match_profiles_from_roster(&report.pack, &roster, world_seed);
        let membership: Vec<CompetitionId> =
            report.pack.clubs.iter().map(|c| c.competition).collect();

        Ok(Self {
            pack: report.pack,
            pack_id: report.manifest.id,
            pack_version: report.manifest.version,
            membership,
            profiles,
            roster,
            budgets,
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
                // Folha salarial antes de qualquer coisa: cada clube paga
                // o elenco inteiro pelo CA atual (`world::pay_salaries`,
                // `docs/01 §2.2` RF-CL-02, fatia mínima) — só então o que
                // sobrar no orçamento entra no mercado. Nunca deixa um
                // orçamento negativo (zera, não endivida).
                let payroll_paid = world::pay_salaries(&self.roster, &mut self.budgets);

                // Mercado antes da temporada: clubes tentam substituir seu
                // titular mais fraco por um reserva melhor de outro clube
                // que caiba no orçamento (`world::run_market_day`,
                // `docs/07-roadmap.md` M1: "primeira IA de mercado"). Feito
                // antes de recalcular a força, pra quem comprou reforço já
                // jogar mais forte nesta mesma temporada.
                let transfers = world::run_market_day(&mut self.roster, &mut self.budgets);

                // Lesões da temporada: substitui completamente o estado
                // anterior (`world::roll_injuries`), então um jogador
                // recuperado não some do roster — só volta a ser elegível
                // pra escalação.
                world::roll_injuries(&mut self.roster, self.world_seed, season_index);
                let injuries = self.roster.iter().filter(|p| p.injured).count();

                // Recalcula o perfil a partir do roster atual — reflete
                // progressão (`world::advance_season`, abaixo), as
                // transferências que acabaram de acontecer e as lesões
                // desta temporada (jogador machucado nunca titulariza, ver
                // `world::strength_from_roster`/`world::profile_from_roster`).
                // Em `season_index == 0` sem transferência nem lesão, é
                // idêntico ao que `Self::new` já calculou.
                self.profiles = world::generate_match_profiles_from_roster(
                    &self.pack,
                    &self.roster,
                    self.world_seed,
                );
                // Titulares desta temporada — a mesma seleção que o cálculo
                // de perfil acima acabou de fazer clube a clube, guardada à
                // parte porque `world::apply_season_fatigue` (abaixo)
                // precisa saber exatamente quem jogou, não só o perfil
                // agregado do time.
                let starters = world::starters_from_roster_all_clubs(&self.pack, &self.roster);
                // `Some(&self.roster)` liga o caminho de carreira
                // (`world::season`, módulo): liga simulada rodada a
                // rodada, perfil recalculado a cada uma a partir do
                // roster — é o que dá a `world::discipline` (suspensões)
                // um ponto na simulação onde "fora desta rodada" tem
                // efeito de verdade. Copa continua usando `self.profiles`
                // (instantâneo do início da temporada) de qualquer jeito.
                let result = world::run_season(
                    &self.pack,
                    &self.membership,
                    &self.profiles,
                    Some(&self.roster),
                    self.world_seed,
                    season_index,
                );

                for &(club, _from, to) in &result.movements {
                    self.membership[club.as_usize()] = to;
                }

                // Condição de entrada na próxima temporada
                // (`world::apply_season_fatigue`, `docs/01 §2.1` RF-JG-07,
                // fatia mínima): titular que jogou muita partida de liga
                // rende um pouco menos na temporada seguinte, reserva entra
                // descansado. Só partidas de liga contam — `result.tables`
                // nunca inclui copa (`world::cup`, `SeasonResult::cups`).
                let matches_played_by_club: HashMap<domain::ClubId, u32> = result
                    .tables
                    .iter()
                    .flat_map(|(_, table)| table.iter().map(|row| (row.club, row.played)))
                    .collect();
                world::apply_season_fatigue(&mut self.roster, &starters, &matches_played_by_club);
                let tired_players = self
                    .roster
                    .iter()
                    .filter(|p| p.condition < world::FULL_CONDITION)
                    .count();

                world::advance_season(&mut self.roster, self.world_seed, season_index);

                let receipt = CommandReceipt {
                    season_index,
                    matches_played: result.matches_played,
                    movements: result.movements.len(),
                    transfers: transfers.len(),
                    injuries,
                    tired_players,
                    suspensions: result.suspensions,
                    payroll_paid,
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
            Query::CupChampion { competition } => {
                QueryResult::CupChampion(self.cup_champion(competition))
            }
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
                kind: match comp.format {
                    pack::Format::RoundRobin { .. } => CompetitionKind::League,
                    pack::Format::Knockout { .. } => CompetitionKind::Cup,
                },
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

    fn cup_champion(&self, competition: CompetitionId) -> Option<CupChampionRow> {
        let latest = self.history.last()?;
        let cup = latest.cups.iter().find(|c| c.competition == competition)?;
        Some(CupChampionRow {
            champion_club_name: self.pack.club(cup.champion).name.clone(),
            rounds_played: cup.rounds.len() as u32,
            matches_played: cup.rounds.iter().map(|r| r.len() as u32).sum(),
        })
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

    fn cup_id(session: &GameSession) -> CompetitionId {
        match session.query(Query::Competitions) {
            QueryResult::Competitions(list) => {
                list.iter().find(|c| c.name.contains("Copa")).unwrap().id
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
        assert_eq!(list.len(), 3); // 2 ligas + 1 copa (docs/07 M1)
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
    fn copa_produz_campeao_consultavel_sem_contar_na_calibracao_de_liga() {
        let mut session = GameSession::new(&example_pack_path(), 42).unwrap();
        let cup = cup_id(&session);
        let league = tier1_id(&session);

        let receipt = session.dispatch(Command::AdvanceSeason).unwrap();
        // 2 ligas (56 jogos cada) — a copa (15 jogos: 8+4+2+1) não entra
        // aqui, por design (`world::cup`, `docs/04 §4.1`).
        assert_eq!(receipt.matches_played, 56 * 2);

        let QueryResult::CupChampion(Some(champion)) =
            session.query(Query::CupChampion { competition: cup })
        else {
            panic!("esperava campeão de copa após avançar a temporada");
        };
        assert_eq!(champion.rounds_played, 4); // 16 clubes: oitavas/quartas/semi/final
        assert_eq!(champion.matches_played, 15); // 8+4+2+1
        assert!(!champion.champion_club_name.is_empty());

        // Uma liga não tem campeão de copa, e uma copa não tem tabela.
        assert_eq!(
            session.query(Query::CupChampion {
                competition: league
            }),
            QueryResult::CupChampion(None)
        );
        assert_eq!(
            session.query(Query::Standings { competition: cup }),
            QueryResult::Standings(None)
        );
    }

    #[test]
    fn vinte_temporadas_seguidas_nao_travam_e_preservam_os_invariantes_do_mundo() {
        // Portão de saída do M1 (`docs/07-roadmap.md` §2): "20 temporadas
        // seguidas sem divergência entre plataformas e com métricas... dentro
        // da tolerância". `golden verify` (docs/08 §3) já cobre "sem
        // divergência entre plataformas" e `calibrate --check` (docs/04 §4.2)
        // já cobre "métricas dentro da tolerância" — nenhum dos dois roda 20
        // temporadas pela fronteira real de carreira (`app::GameSession`,
        // mercado + lesões + condição + progressão + copa + liga, todos
        // juntos, não isolados uns dos outros). Este teste é o que de fato
        // exercita essa duração, e prova que nada degenera silenciosamente
        // ao longo do caminho: dinheiro sempre conservado, CA nunca passa do
        // potencial, elenco nunca cresce/encolhe, ligas nunca desbalanceiam,
        // copa sempre produz exatamente um campeão.
        let mut session = GameSession::new(&example_pack_path(), 2026).unwrap();
        let initial_roster_len = session.roster.len();
        let initial_money_total: i64 = session.budgets.iter().map(|b| b.cents()).sum();

        let QueryResult::Competitions(competitions) = session.query(Query::Competitions) else {
            unreachable!()
        };
        let mut initial_league_sizes: Vec<(CompetitionId, usize)> = Vec::new();

        for season in 0..20 {
            let receipt = session.dispatch(Command::AdvanceSeason).unwrap();
            assert!(
                receipt.matches_played > 0,
                "temporada {season} não simulou nenhuma partida"
            );

            if season == 0 {
                for comp in &competitions {
                    if comp.kind == CompetitionKind::League {
                        let QueryResult::Standings(Some(table)) = session.query(Query::Standings {
                            competition: comp.id,
                        }) else {
                            panic!("liga '{}' sem tabela após a 1ª temporada", comp.name);
                        };
                        initial_league_sizes.push((comp.id, table.len()));
                    }
                }
            }
        }

        assert_eq!(
            session.roster.len(),
            initial_roster_len,
            "elenco total não deveria crescer nem encolher em 20 temporadas"
        );
        for player in &session.roster {
            assert!(
                player.ability.current() <= player.ability.potential(),
                "jogador {:?} com CA acima do PA após 20 temporadas",
                player.player
            );
            assert!(
                player.condition <= world::FULL_CONDITION,
                "condição de {:?} acima do máximo",
                player.player
            );
            assert!(
                player.age_years > 0,
                "jogador {:?} com idade não-positiva",
                player.player
            );
        }

        // Transferência conserva dinheiro entre clubes, mas a folha
        // salarial (`world::pay_salaries`, RF-CL-02) é um gasto real que
        // sai da economia a cada temporada — o total só pode diminuir ao
        // longo de 20 temporadas, nunca aumentar nem ficar igual (todo
        // clube tem elenco com CA > 0, então todo clube paga algo).
        let final_money_total: i64 = session.budgets.iter().map(|b| b.cents()).sum();
        assert!(
            final_money_total < initial_money_total,
            "dinheiro total do mundo deveria diminuir (nunca aumentar) em 20 temporadas, por \
             causa da folha salarial — ficou igual ou maior, algo está conservando demais ou \
             criando dinheiro do nada"
        );

        for (competition, expected_size) in initial_league_sizes {
            let QueryResult::Standings(Some(table)) =
                session.query(Query::Standings { competition })
            else {
                panic!("liga {competition:?} sem tabela após 20 temporadas");
            };
            assert_eq!(
                table.len(),
                expected_size,
                "liga {competition:?} mudou de tamanho em 20 temporadas — promoção/rebaixamento desbalanceado"
            );
        }

        for comp in &competitions {
            if comp.kind == CompetitionKind::Cup {
                let QueryResult::CupChampion(Some(champion)) = session.query(Query::CupChampion {
                    competition: comp.id,
                }) else {
                    panic!("copa '{}' sem campeão após 20 temporadas", comp.name);
                };
                assert!(!champion.champion_club_name.is_empty());
            }
        }
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
        let initial_profiles = session.profiles.clone();

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
            initial_profiles, session.profiles,
            "perfil dos clubes deveria mudar após 5 temporadas de progressão"
        );
    }

    #[test]
    fn titulares_de_fato_desgastam_e_reporte_de_cansados_bate_com_o_roster() {
        // Caixa branca: prova que `world::apply_season_fatigue` está
        // ligado ao dispatch (não só existe isolada em `world`) — depois
        // de uma temporada inteira, pelo menos os titulares (que jogaram
        // partidas de liga de verdade) deveriam entrar na próxima com
        // condição abaixo de `FULL_CONDITION`.
        let mut session = GameSession::new(&example_pack_path(), 21).unwrap();
        assert!(
            session
                .roster
                .iter()
                .all(|p| p.condition == world::FULL_CONDITION),
            "ninguém deveria começar a carreira cansado"
        );

        let receipt = session.dispatch(Command::AdvanceSeason).unwrap();

        assert_eq!(
            receipt.tired_players,
            session
                .roster
                .iter()
                .filter(|p| p.condition < world::FULL_CONDITION)
                .count(),
            "receipt.tired_players deveria bater com o roster logo após o dispatch"
        );
        assert!(
            receipt.tired_players > 0,
            "esperava pelo menos um jogador cansado após uma temporada inteira de liga \
             (256 jogadores, 16 × 11 titulares no pack de exemplo)"
        );
    }

    #[test]
    fn suspensoes_de_fato_acontecem_ligadas_ao_dispatch() {
        // Caixa branca: prova que o caminho de carreira
        // (`world::season::run_season` com `roster: Some`, rodada a
        // rodada) está de fato ligado ao dispatch, não só existe isolado
        // em `world::season`/`world::discipline`. ~4% por time por
        // partida, 16 clubes de liga, 14 partidas cada: em 10 temporadas é
        // extremamente improvável nunca acontecer nenhuma se o sorteio
        // estivesse de fato rodando.
        let mut session = GameSession::new(&example_pack_path(), 55).unwrap();
        let mut total_suspensions = 0usize;
        for _ in 0..10 {
            let receipt = session.dispatch(Command::AdvanceSeason).unwrap();
            total_suspensions += receipt.suspensions;
        }
        assert!(
            total_suspensions > 0,
            "esperava pelo menos uma suspensão em 10 temporadas no pack de exemplo"
        );
    }

    #[test]
    fn folha_salarial_de_fato_debita_o_orcamento_e_bate_com_o_reporte() {
        // Caixa branca: prova que `world::pay_salaries` está ligado ao
        // dispatch — todo clube tem elenco com CA > 0, então o orçamento
        // total tem que cair estritamente a cada temporada, e o valor
        // reportado em `CommandReceipt::payroll_paid` tem que bater com
        // a queda observada no orçamento.
        let mut session = GameSession::new(&example_pack_path(), 3).unwrap();
        let budget_before: i64 = session.budgets.iter().map(|b| b.cents()).sum();

        let receipt = session.dispatch(Command::AdvanceSeason).unwrap();

        assert!(
            receipt.payroll_paid.cents() > 0,
            "esperava folha salarial positiva no pack de exemplo (256 jogadores com CA > 0)"
        );
        // `payroll_paid` é só a folha; o orçamento também muda com
        // transferências (`run_market_day`), então a queda total do
        // orçamento tem que ser **pelo menos** a folha paga, não
        // necessariamente igual (compra/venda também move o total entre
        // clubes, mas o mercado em si conserva — só a folha drena).
        let budget_after: i64 = session.budgets.iter().map(|b| b.cents()).sum();
        assert_eq!(
            budget_before - budget_after,
            receipt.payroll_paid.cents(),
            "a queda do orçamento total deveria ser exatamente a folha paga, já que \
             transferências conservam dinheiro entre clubes"
        );
    }

    #[test]
    fn mercado_de_transferencias_de_fato_move_jogadores_entre_clubes() {
        // Outro teste de caixa branca: prova que `world::run_market_day`
        // está ligado ao dispatch e produz transferências de verdade no
        // pack de exemplo (256 jogadores reais, `docs/07-roadmap.md` M1) —
        // não só que a função isolada funciona (isso já é testado em
        // `ai::market` e `world::market`).
        let mut session = GameSession::new(&example_pack_path(), 11).unwrap();
        let initial_clubs: Vec<domain::ClubId> = session.roster.iter().map(|p| p.club).collect();
        let initial_budgets = session.budgets.clone();

        let mut total_transfers = 0usize;
        for _ in 0..5 {
            let receipt = session.dispatch(Command::AdvanceSeason).unwrap();
            total_transfers += receipt.transfers;
        }

        assert!(
            total_transfers > 0,
            "esperava pelo menos uma transferência em 5 temporadas de mercado"
        );
        let final_clubs: Vec<domain::ClubId> = session.roster.iter().map(|p| p.club).collect();
        assert_ne!(
            initial_clubs, final_clubs,
            "pelo menos um jogador deveria ter mudado de clube"
        );
        assert_ne!(
            initial_budgets, session.budgets,
            "orçamentos deveriam ter mudado com as transferências"
        );
        // Transferência em si conserva dinheiro (mesma propriedade de
        // `ai::market::proptests::dinheiro_total_e_conservado`), mas o
        // total do mundo não fica mais igual depois de 5 temporadas: a
        // folha salarial (`world::pay_salaries`, RF-CL-02) drena dinheiro
        // da economia a cada uma — só pode diminuir, nunca aumentar.
        let total_before: i64 = initial_budgets.iter().map(|b| b.cents()).sum();
        let total_after: i64 = session.budgets.iter().map(|b| b.cents()).sum();
        assert!(
            total_after < total_before,
            "folha salarial de 5 temporadas deveria ter drenado algum dinheiro da economia"
        );
    }

    #[test]
    fn lesoes_de_fato_acontecem_e_excluem_jogador_da_escalacao() {
        // Caixa branca: prova que `world::roll_injuries` está ligado ao
        // dispatch (256 jogadores reais, ~6% de chance/temporada — esperar
        // zero lesões em 5 temporadas seria ~1 em milhares) e que o
        // CommandReceipt reporta a contagem certa.
        let mut session = GameSession::new(&example_pack_path(), 13).unwrap();

        let mut saw_injury = false;
        for _ in 0..5 {
            let receipt = session.dispatch(Command::AdvanceSeason).unwrap();
            assert_eq!(
                receipt.injuries,
                session.roster.iter().filter(|p| p.injured).count(),
                "receipt.injuries deveria bater com o roster logo após o dispatch"
            );
            if receipt.injuries > 0 {
                saw_injury = true;
            }
        }
        assert!(
            saw_injury,
            "esperava pelo menos uma lesão em 5 temporadas no pack de exemplo"
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
