//! Loop de temporada — gera o calendário de cada competição
//! ([`rules::round_robin`]), simula cada confronto ([`engine::simulate`]) e
//! calcula a tabela final ([`rules::compute_table`]), aplicando promoção e
//! rebaixamento para a temporada seguinte. É o que fecha o portão do M1:
//! "um mundo que gira sozinho, sem uma única tela"
//! (`docs/07-roadmap.md#m1--kick-off-headless-10-semanas`).
//!
//! Duas formas de simular uma liga, escolhidas por `run_season` conforme
//! [`CareerRound`] é passado ou não: **estática** (`career: None` — perfil
//! fixo a temporada inteira, o caminho de `managerfc-cli calibrate`/`bench`,
//! sem mudança de comportamento desde antes desta fatia) e **de carreira**
//! (`career: Some` — `app::GameSession`, perfil recalculado **a cada
//! rodada** a partir do roster atual, excluindo quem está suspenso naquela
//! rodada, e bilheteria de cada partida em casa creditada no orçamento do
//! mandante — `crate::finance::match_day_revenue`, RF-CL-03). A
//! granularidade de rodada existe só por causa de `crate::discipline`:
//! suspensão (`RF-JG-09`) só faz sentido em granularidade de partida, e
//! até essa fatia toda liga era simulada de uma vez, sem nenhum ponto no
//! meio da temporada onde "o jogador X está fora desta rodada" pudesse ter
//! efeito; bilheteria por rodada aproveitou o mesmo ponto de simulação, por
//! já existir. Consistência (`crate::consistency`) e adaptação
//! (`crate::adaptation`) também aproveitam esse mesmo ponto: cada perfil de
//! rodada recebe um balanço/malus de força conforme os titulares daquela
//! rodada, antes de simular qualquer partida dela. Moral (`crate::morale`)
//! também — mas é a única fatia por rodada que não olha pra um atributo
//! de jogador, e sim pro saldo de resultados do próprio clube já simulados
//! nesta temporada (sem RNG nenhum: função pura de `RoundRobinOutcome::results`
//! acumulado até aqui). Cup (`crate::cup`) não ganha essa granularidade —
//! continua usando o perfil estático do início da temporada em qualquer um
//! dos dois casos, e não gera bilheteria nem é afetada por suspensão,
//! consistência, adaptação ou moral (mesma exclusão de `crate::condition`).

use std::cmp::Ordering;
use std::collections::{BTreeMap, HashMap, HashSet};

use domain::{ClubId, CompetitionId, Money, PlayerId};
use pack::{Format, LoadedPack};
use rules::{Fixture, Score, TableRow};

use crate::progression::PlayerState;

/// Entradas do caminho de carreira agrupadas numa só estrutura — em vez de
/// `roster`/`budgets`/`stadium_capacities` como três parâmetros `Option`
/// independentes de [`run_season`], o que deixaria passar uma combinação
/// incoerente (ex.: `budgets` sem `roster`) um erro de compilação em vez de
/// um bug silencioso em tempo de execução. `budgets` é mutável: cada
/// partida em casa credita bilheteria nele (`crate::finance::match_day_revenue`,
/// RF-CL-03) durante a simulação rodada a rodada.
pub struct CareerRound<'a> {
    pub roster: &'a [PlayerState],
    pub budgets: &'a mut [Money],
    pub stadium_capacities: &'a [u32],
    /// Reputação de cada clube, 0-100 (`crate::reputation::generate_club_reputation`)
    /// — influencia o piso de público em `crate::finance::match_day_revenue`
    /// (RF-MU-08, "receita").
    pub club_reputation: &'a [u8],
    /// Sujeira de cada jogador, 0-100 (`crate::discipline::generate_dirtiness`)
    /// — pesa quem é escolhido quando o time sofre uma expulsão
    /// (`crate::discipline::roll_suspensions`), mesmo índice denso do
    /// roster inteiro (não só os titulares da rodada).
    pub dirtiness: &'a [u8],
    /// Mentalidade de "jogo importante" de cada jogador, 0-100
    /// (`crate::cup::generate_big_game_mentality`) — escala a força de cada
    /// clube só na copa (`crate::cup::apply_big_game_bonus`), mesmo índice
    /// denso do roster inteiro.
    pub big_game_mentality: &'a [u8],
    /// Consistência de cada jogador, 0-100
    /// (`crate::consistency::generate_consistency`) — balança a força de
    /// cada clube a cada rodada de liga
    /// (`crate::consistency::apply_consistency_swing`), mesmo índice denso
    /// do roster inteiro.
    pub consistency: &'a [u8],
    /// Velocidade de adaptação de cada jogador, 0-100
    /// (`crate::adaptation::generate_adaptation`) — reduz a força de um
    /// recém-chegado por até duas temporadas
    /// (`crate::adaptation::apply_adaptation_penalty`), mesmo índice denso
    /// do roster inteiro.
    pub adaptation: &'a [u8],
    /// Quantas temporadas desde a última transferência de cada jogador
    /// (`crate::adaptation::initial_seasons_since_transfer`/
    /// `mark_transferred`/`advance_seasons_since_transfer`, estado mutável
    /// de carreira — `app::GameSession` cuida da mutação, este campo é só
    /// leitura durante o loop por rodada), mesmo índice denso do roster
    /// inteiro.
    pub seasons_since_transfer: &'a [u8],
    /// Escalação e instruções do técnico humano (`crate::setup`) — vazio
    /// numa sessão só observadora, uma entrada quando há um clube sob
    /// comando. Clube sem entrada aqui segue o caminho da IA.
    pub setups: &'a [crate::setup::ClubSetup],
}

/// Resultado de simular uma temporada inteira, competição por competição.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeasonResult {
    pub season_index: u32,
    /// Uma entrada por competição do pack, na mesma ordem
    /// (`pack.competitions`, ordem de id denso — determinística).
    pub tables: Vec<(CompetitionId, Vec<TableRow>)>,
    /// Clubes que mudam de competição na temporada seguinte, como
    /// consequência da tabela desta: `(clube, de, para)`.
    pub movements: Vec<(ClubId, CompetitionId, CompetitionId)>,
    pub matches_played: u32,
    pub home_wins: u32,
    pub away_wins: u32,
    pub draws: u32,
    pub total_goals: u32,
    /// Total de finalizações (gol ou não — `engine::MatchEvent::Shot`),
    /// somando os dois lados de toda partida da temporada. Alimenta a
    /// calibração de "finalizações por time"/"conversão" (`docs/04 §4.1`),
    /// que o motor v0 (sem `Shot`) não tinha como medir.
    pub total_shots: u32,
    /// Resultado de cada competição em formato `Format::Knockout` desta
    /// temporada (`crate::cup`) — **não** inclui partidas de copa em
    /// nenhuma das contagens acima (`matches_played`/`total_goals`/etc):
    /// essas são as contagens de calibração de liga (`docs/04 §4.1`), e
    /// misturar copa ali enviesaria sem avisar. Uma entrada por competição
    /// de mata-mata do pack, na mesma ordem de `pack.competitions`.
    pub cups: Vec<crate::cup::CupResult>,
    /// Quantas suspensões de verdade aconteceram esta temporada
    /// (`crate::discipline::roll_suspensions`, uma por rodada em que uma
    /// expulsão foi sorteada) — sempre `0` quando `run_season` é chamado
    /// sem roster (caminho estático: sem granularidade de rodada, sem
    /// como simular suspensão, ver o doc do módulo).
    pub suspensions: usize,
    /// Total de bilheteria creditada nesta temporada
    /// (`crate::finance::match_day_revenue`, RF-CL-03, fatia mínima) — só
    /// partidas de **liga** em casa geram receita aqui (copa não, mesma
    /// exclusão de suspensão/condição); sempre `Money::ZERO` quando
    /// `run_season` é chamado sem [`CareerRound`] (caminho estático: sem
    /// orçamento nenhum para creditar, ver o doc do módulo).
    pub match_day_revenue: Money,
}

impl SeasonResult {
    /// Média de gols por partida, em milésimos (mesma escala de
    /// `docs/03 §4`) — conveniência para relatórios de calibração
    /// (`docs/08 §4`) sem forçar quem chama a fazer a conta.
    #[must_use]
    pub fn average_goals_permille(&self) -> u64 {
        if self.matches_played == 0 {
            return 0;
        }
        u64::from(self.total_goals) * 1000 / u64::from(self.matches_played)
    }
}

/// Simula uma temporada. `membership[club.as_usize()]` é a competição atual
/// do clube — normalmente vem de `pack` na primeira temporada e da
/// [`SeasonResult::movements`] da anterior daí em diante ([`run_seasons`]
/// já cuida disso). `profiles` vem de
/// [`crate::quality::generate_match_profiles`] (ou da versão com roster,
/// `crate::quality::generate_match_profiles_from_roster`) — é sempre usado
/// para copa, e para liga **apenas** quando `career` é `None`.
///
/// `career`, quando `Some`, muda como toda competição `Format::RoundRobin`
/// é simulada: rodada a rodada, recalculando o perfil de cada clube que
/// joga a partir do roster atual (excluindo quem está suspenso — ver o doc
/// do módulo) em vez de usar `profiles` a temporada inteira, e creditando
/// bilheteria de cada partida em casa no orçamento do mandante
/// (`crate::finance::match_day_revenue`). `None` é exatamente o
/// comportamento de antes desta fatia (`managerfc-cli calibrate`/`bench`,
/// via [`run_seasons`]). O caminho de carreira é uma [`SeasonRun`] jogada
/// até o fim de uma vez — quem precisa parar entre rodadas
/// (`app::GameSession`, `Command::AdvanceRound`) usa [`SeasonRun`]
/// diretamente; o resultado é idêntico, byte a byte.
#[must_use]
pub fn run_season(
    pack: &LoadedPack,
    membership: &[CompetitionId],
    profiles: &[engine::TeamMatchProfile],
    career: Option<CareerRound<'_>>,
    world_seed: u64,
    season_index: u32,
) -> SeasonResult {
    match career {
        Some(mut career) => {
            let mut run = SeasonRun::start(pack, membership, season_index);
            while !run.is_finished() {
                let _ = run.play_matchday(pack, &mut career, world_seed);
            }
            run.finish(pack, profiles, Some(&career), world_seed)
        }
        None => {
            let mut simulated = Vec::new();
            for competition in &pack.competitions {
                let Format::RoundRobin { legs, .. } = competition.format else {
                    continue;
                };
                let clubs = league_clubs(membership, competition.id);
                let Ok(fixtures) = rules::round_robin(&clubs, legs) else {
                    // Ver o doc de `SeasonRun::start` para o porquê de pular.
                    continue;
                };
                let outcome =
                    simulate_round_robin_static(profiles, fixtures, world_seed, season_index);
                simulated.push((competition.id, clubs, outcome));
            }
            let leagues: Vec<LeagueOutcome<'_>> = simulated
                .iter()
                .map(|(competition, clubs, outcome)| LeagueOutcome {
                    competition: *competition,
                    clubs,
                    outcome,
                })
                .collect();
            assemble_season(pack, profiles, None, &leagues, world_seed, season_index)
        }
    }
}

/// Clubes atualmente na competição, em ordem de id denso — nunca via
/// iteração de um `HashMap` (`ADR 0002`: calendário e tabela não podem
/// depender de ordem não-determinística).
fn league_clubs(membership: &[CompetitionId], competition: CompetitionId) -> Vec<ClubId> {
    membership
        .iter()
        .enumerate()
        .filter(|&(_, &comp)| comp == competition)
        .map(|(idx, _)| ClubId::new(idx as u32))
        .collect()
}

/// Uma liga já simulada por inteiro — entrada de [`assemble_season`].
struct LeagueOutcome<'a> {
    competition: CompetitionId,
    clubs: &'a [ClubId],
    outcome: &'a RoundRobinOutcome,
}

/// Fecha uma temporada: tabela, acesso/queda e totais de cada liga já
/// simulada (`leagues`), mais a copa, na ordem de `pack.competitions` —
/// comum ao caminho estático e ao de carreira ([`SeasonRun::finish`]), pra
/// os dois nunca divergirem na montagem do [`SeasonResult`].
fn assemble_season(
    pack: &LoadedPack,
    profiles: &[engine::TeamMatchProfile],
    career: Option<&CareerRound<'_>>,
    leagues: &[LeagueOutcome<'_>],
    world_seed: u64,
    season_index: u32,
) -> SeasonResult {
    let mut tables = Vec::with_capacity(pack.competitions.len());
    let mut movements = Vec::new();
    let mut cups = Vec::new();
    let mut matches_played = 0u32;
    let mut home_wins = 0u32;
    let mut away_wins = 0u32;
    let mut draws = 0u32;
    let mut total_goals = 0u32;
    let mut total_shots = 0u32;
    let mut suspensions = 0usize;
    let mut match_day_revenue = Money::ZERO;

    for competition in &pack.competitions {
        match competition.format {
            Format::RoundRobin { .. } => {
                let Some(pos) = leagues.iter().position(|l| l.competition == competition.id) else {
                    continue;
                };
                let LeagueOutcome { clubs, outcome, .. } = &leagues[pos];
                matches_played += outcome.matches_played;
                home_wins += outcome.home_wins;
                away_wins += outcome.away_wins;
                draws += outcome.draws;
                total_goals += outcome.total_goals;
                total_shots += outcome.total_shots;
                suspensions += outcome.suspensions;
                match_day_revenue = match_day_revenue + outcome.match_day_revenue;

                let table = rules::compute_table(clubs, &outcome.results, &competition.tiebreakers);

                if let Some(target) = competition.promotion.to {
                    for row in table.iter().take(competition.promotion.slots as usize) {
                        movements.push((row.club, competition.id, target));
                    }
                }
                if let Some(target) = competition.relegation.to {
                    for row in table
                        .iter()
                        .rev()
                        .take(competition.relegation.slots as usize)
                    {
                        movements.push((row.club, competition.id, target));
                    }
                }

                tables.push((competition.id, table));
            }
            Format::Knockout { .. } => {
                // Participantes = todos os clubes do país, não `membership`
                // (nenhum clube aponta `competition` pra uma copa —
                // `crate::cup`). Partidas de copa **não** entram em
                // `matches_played`/`total_goals`/etc — ver doc de
                // `SeasonResult::cups`. Sempre usa o perfil estático do
                // início da temporada, mesmo no caminho de carreira —
                // suspensão não afeta copa (doc do módulo). "Jogo
                // importante" (`crate::cup::apply_big_game_bonus`) escala
                // essa força só para a copa, e só no caminho de carreira
                // (`None` no caminho estático devolve `profiles` sem
                // mudar nada, ver o doc de `crate::cup`).
                let participants = crate::cup::cup_participants(pack, competition);
                let cup_profiles = crate::cup::apply_big_game_bonus(
                    pack,
                    career.map(|c| (c.roster, c.big_game_mentality)),
                    profiles,
                );
                if let Some(result) = crate::cup::run_cup(
                    competition.id,
                    &participants,
                    &cup_profiles,
                    world_seed,
                    season_index,
                ) {
                    cups.push(result);
                }
            }
        }
    }

    SeasonResult {
        season_index,
        tables,
        movements,
        cups,
        matches_played,
        home_wins,
        away_wins,
        draws,
        total_goals,
        total_shots,
        suspensions,
        match_day_revenue,
    }
}

/// Acumulador de uma liga simulada — comum aos dois caminhos
/// ([`simulate_round_robin_static`]/[`simulate_round_robin_for_career`]),
/// pra `run_season` somar nos totais da temporada sem repetir seis
/// `+=` por caminho.
#[derive(Debug, Default)]
struct RoundRobinOutcome {
    results: Vec<(Fixture, Score)>,
    matches_played: u32,
    home_wins: u32,
    away_wins: u32,
    draws: u32,
    total_goals: u32,
    total_shots: u32,
    suspensions: usize,
    match_day_revenue: Money,
}

impl RoundRobinOutcome {
    fn record_match(&mut self, fixture: Fixture, score: Score, shots: (u32, u32)) {
        self.matches_played += 1;
        self.total_goals += score.home_goals + score.away_goals;
        self.total_shots += shots.0 + shots.1;
        match score.home_goals.cmp(&score.away_goals) {
            Ordering::Greater => self.home_wins += 1,
            Ordering::Less => self.away_wins += 1,
            Ordering::Equal => self.draws += 1,
        }
        self.results.push((fixture, score));
    }
}

/// Simula um turno/returno inteiro com o perfil estático de `profiles`,
/// igual a antes desta fatia — nenhuma mudança de comportamento para quem
/// chama sem roster (`managerfc-cli calibrate`/`bench`, via
/// [`run_seasons`]).
fn simulate_round_robin_static(
    profiles: &[engine::TeamMatchProfile],
    fixtures: Vec<Fixture>,
    world_seed: u64,
    season_index: u32,
) -> RoundRobinOutcome {
    let mut outcome = RoundRobinOutcome::default();
    for fixture in fixtures {
        let home = profiles[fixture.home.as_usize()];
        let away = profiles[fixture.away.as_usize()];
        let (score, shots) = simulate_fixture(
            home,
            away,
            (engine::Tactics::NEUTRAL, engine::Tactics::NEUTRAL),
            world_seed,
            season_index,
            &fixture,
        );
        outcome.record_match(fixture, score, shots);
    }
    outcome
}

/// O que uma rodada (dia de jogos) da temporada produziu — devolvido por
/// [`SeasonRun::play_matchday`] pra quem avança rodada a rodada
/// (`app::GameSession`, `Command::AdvanceRound`) reportar sem esperar o fim
/// da temporada. A soma de todos os `MatchdayReport` de uma temporada bate
/// com os totais de [`SeasonResult`] (testado).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct MatchdayReport {
    /// Índice (0-based) da rodada que acabou de ser jogada.
    pub matchday: u32,
    /// Partidas de **liga** disputadas nesta rodada (copa só no fim da
    /// temporada, ver [`SeasonRun::finish`]).
    pub matches_played: u32,
    pub suspensions: usize,
    pub match_day_revenue: Money,
    /// Placar de cada partida desta rodada, competição por competição.
    pub results: Vec<(CompetitionId, Fixture, Score)>,
    /// Titulares escalados pelo técnico humano que não puderam jogar nesta
    /// rodada, com quem entrou no lugar: `(saiu, entrou)`
    /// (`crate::setup::resolve_xi`).
    pub lineup_adjustments: Vec<(PlayerId, PlayerId)>,
}

/// Estado de uma liga no meio da temporada (ver [`SeasonRun`]).
#[derive(Debug)]
struct LeagueRun {
    competition: CompetitionId,
    clubs: Vec<ClubId>,
    /// Calendário agrupado por rodada, em ordem crescente de rodada.
    rounds: Vec<(u32, Vec<Fixture>)>,
    outcome: RoundRobinOutcome,
    /// Suspensos **na rodada seguinte** — nunca atravessa competição nem
    /// temporada (começa vazio a cada [`SeasonRun::start`]).
    suspended: HashSet<PlayerId>,
}

/// Uma temporada de carreira jogada **uma rodada por vez**: é o que deixa a
/// UI parar entre dois dias de jogo (escalar, mexer na tática, negociar) em
/// vez de só avançar a temporada inteira. As ligas rodam em paralelo — a
/// rodada `k` de toda liga que tem `k`-ésima rodada é jogada junta —, o que
/// é seguro pra determinismo porque as ligas não compartilham estado
/// (clubes pertencem a uma liga por vez, e cada partida/sorteio é derivado
/// de `(seed, clube, rodada)`, nunca de uma sequência global), então jogar
/// liga por liga (como antes) ou rodada por rodada dá exatamente o mesmo
/// resultado — teste de regressão e golden master provam isso.
///
/// Fluxo: [`Self::start`] → [`Self::play_matchday`] até
/// [`Self::is_finished`] → [`Self::finish`] (tabelas, acesso/queda e copa).
#[derive(Debug)]
pub struct SeasonRun {
    season_index: u32,
    leagues: Vec<LeagueRun>,
    matchday: u32,
    total_matchdays: u32,
    /// Rodada preparada e ainda não concluída (ver [`SeasonRun::begin_matchday`]).
    prepared: Option<Vec<LeagueRoundPrep>>,
}

impl SeasonRun {
    /// Monta o calendário de toda liga a partir de `membership` (a
    /// competição atual de cada clube). Uma liga cujo calendário não pode
    /// ser gerado (número de clubes ímpar ou < 2) é pulada nesta temporada
    /// — não deveria acontecer com um pack validado (`docs/03 §7`), mas
    /// também pode surgir se `promotion.slots`/`relegation.slots` de duas
    /// competições emparelhadas não baterem entre si (validação que o
    /// `pack` ainda não faz — dívida técnica conhecida, não um `panic!`
    /// escondido): pular uma liga é mais seguro que travar o mundo inteiro.
    #[must_use]
    pub fn start(pack: &LoadedPack, membership: &[CompetitionId], season_index: u32) -> Self {
        let mut leagues = Vec::new();
        for competition in &pack.competitions {
            let Format::RoundRobin { legs, .. } = competition.format else {
                continue;
            };
            let clubs = league_clubs(membership, competition.id);
            let Ok(fixtures) = rules::round_robin(&clubs, legs) else {
                continue;
            };
            let mut by_round: BTreeMap<u32, Vec<Fixture>> = BTreeMap::new();
            for fixture in fixtures {
                by_round.entry(fixture.round).or_default().push(fixture);
            }
            leagues.push(LeagueRun {
                competition: competition.id,
                clubs,
                rounds: by_round.into_iter().collect(),
                outcome: RoundRobinOutcome::default(),
                suspended: HashSet::new(),
            });
        }
        let total_matchdays = leagues
            .iter()
            .map(|l| l.rounds.len() as u32)
            .max()
            .unwrap_or(0);
        Self {
            season_index,
            leagues,
            matchday: 0,
            total_matchdays,
            prepared: None,
        }
    }

    #[must_use]
    pub fn season_index(&self) -> u32 {
        self.season_index
    }

    /// Quantas rodadas a temporada tem (a da liga mais longa).
    #[must_use]
    pub fn total_matchdays(&self) -> u32 {
        self.total_matchdays
    }

    /// Próxima rodada a jogar (0-based); igual a [`Self::total_matchdays`]
    /// quando a temporada acabou.
    #[must_use]
    pub fn next_matchday(&self) -> u32 {
        self.matchday
    }

    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.matchday >= self.total_matchdays
    }

    /// Prepara a próxima rodada de toda liga que ainda tem uma (perfis,
    /// táticas e titulares de cada clube) **sem jogar nada**. É o ponto em
    /// que a UI pode jogar uma das partidas ao vivo
    /// ([`Self::prepared_fixture_of`]) e depois concluir a rodada com o
    /// resultado dela ([`Self::commit_matchday`]). No-op se a rodada já está
    /// preparada ou a temporada acabou.
    pub fn begin_matchday(&mut self, pack: &LoadedPack, career: &CareerRound<'_>, world_seed: u64) {
        if self.is_finished() || self.prepared.is_some() {
            return;
        }
        let mut preps = Vec::with_capacity(self.leagues.len());
        for (index, league) in self.leagues.iter().enumerate() {
            let Some((round, fixtures)) = league.rounds.get(self.matchday as usize) else {
                continue;
            };
            preps.push(prepare_league_round(
                pack,
                career,
                index,
                *round,
                fixtures,
                &league.outcome.results,
                &league.suspended,
                world_seed,
                self.season_index,
            ));
        }
        self.prepared = Some(preps);
    }

    /// `true` entre [`Self::begin_matchday`] e [`Self::commit_matchday`].
    #[must_use]
    pub fn is_matchday_prepared(&self) -> bool {
        self.prepared.is_some()
    }

    /// A partida de `club` na rodada preparada, pronta para o motor.
    #[must_use]
    pub fn prepared_fixture_of(&self, club: ClubId) -> Option<PreparedFixture> {
        let prepared = self.prepared.as_ref()?;
        let prep = prepared.iter().find(|p| p.sides.contains_key(&club))?;
        let league = &self.leagues[prep.league_index];
        let (_, fixtures) = league.rounds.get(self.matchday as usize)?;
        let fixture = *fixtures.iter().find(|f| f.home == club || f.away == club)?;
        Some(PreparedFixture {
            competition: league.competition,
            fixture,
            season_index: self.season_index,
            home: prep.sides[&fixture.home].clone(),
            away: prep.sides[&fixture.away].clone(),
        })
    }

    /// Joga a próxima rodada de toda liga que ainda tem uma. No-op (relatório
    /// vazio) se a temporada já acabou.
    pub fn play_matchday(
        &mut self,
        pack: &LoadedPack,
        career: &mut CareerRound<'_>,
        world_seed: u64,
    ) -> MatchdayReport {
        self.commit_matchday(pack, career, world_seed, None)
    }

    /// Conclui a rodada: prepara (se [`Self::begin_matchday`] ainda não
    /// foi chamado), joga as partidas, credita bilheteria, sorteia as
    /// suspensões e avança. `external` substitui a simulação instantânea de
    /// uma partida pelo resultado jogado ao vivo — com a mesma tática e
    /// sem intervenção, o resultado ao vivo é idêntico ao instantâneo, então
    /// `external: None` e `Some(resultado ao vivo sem mexer em nada)`
    /// produzem a mesma temporada.
    pub fn commit_matchday(
        &mut self,
        pack: &LoadedPack,
        career: &mut CareerRound<'_>,
        world_seed: u64,
        external: Option<ExternalResult>,
    ) -> MatchdayReport {
        let mut report = MatchdayReport {
            matchday: self.matchday,
            ..MatchdayReport::default()
        };
        if self.is_finished() {
            return report;
        }
        self.begin_matchday(pack, career, world_seed);
        let preps = self.prepared.take().unwrap_or_default();
        for prep in preps {
            let league_index = prep.league_index;
            complete_league_round(
                career,
                &mut self.leagues[league_index],
                self.matchday as usize,
                prep,
                external,
                world_seed,
                self.season_index,
                &mut report,
            );
        }
        self.matchday += 1;
        report
    }

    /// Fecha a temporada: tabelas, acesso/queda e copa. `career` só é lido
    /// aqui (roster e mentalidade de jogo importante, pra copa — ver o doc
    /// do módulo). Chamar antes de [`Self::is_finished`] fecha com o que já
    /// foi jogado (tabela parcial), sem pânico. Não consome a execução: o
    /// calendário e os placares continuam consultáveis depois
    /// ([`Self::fixtures_of`], [`Self::round_results`]).
    #[must_use]
    pub fn finish(
        &self,
        pack: &LoadedPack,
        profiles: &[engine::TeamMatchProfile],
        career: Option<&CareerRound<'_>>,
        world_seed: u64,
    ) -> SeasonResult {
        let leagues: Vec<LeagueOutcome<'_>> = self
            .leagues
            .iter()
            .map(|l| LeagueOutcome {
                competition: l.competition,
                clubs: &l.clubs,
                outcome: &l.outcome,
            })
            .collect();
        assemble_season(
            pack,
            profiles,
            career,
            &leagues,
            world_seed,
            self.season_index,
        )
    }

    /// Calendário de `club` na temporada, rodada a rodada, com o placar das
    /// já jogadas. Vazio se o clube não está em nenhuma liga desta
    /// temporada. Copa não entra (sem rodadas, ver o doc do módulo).
    #[must_use]
    pub fn fixtures_of(&self, club: ClubId) -> Vec<FixtureEntry> {
        let Some(league) = self.leagues.iter().find(|l| l.clubs.contains(&club)) else {
            return Vec::new();
        };
        let mut entries = Vec::with_capacity(league.rounds.len());
        // Os placares estão em `outcome.results` na ordem em que as partidas
        // foram jogadas — rodada a rodada, na ordem do calendário —, então o
        // índice de um placar é o deslocamento acumulado de rodadas + a
        // posição da partida na rodada.
        let mut offset = 0usize;
        for (matchday, (_round, fixtures)) in league.rounds.iter().enumerate() {
            for (j, fixture) in fixtures.iter().enumerate() {
                if fixture.home == club || fixture.away == club {
                    let score = if (matchday as u32) < self.matchday {
                        league.outcome.results.get(offset + j).map(|(_, s)| *s)
                    } else {
                        None
                    };
                    entries.push(FixtureEntry {
                        matchday: matchday as u32,
                        competition: league.competition,
                        fixture: *fixture,
                        score,
                    });
                }
            }
            offset += fixtures.len();
        }
        entries
    }

    /// Placares de toda liga na rodada `matchday` (0-based) — só se já foi
    /// jogada, senão vazio.
    #[must_use]
    pub fn round_results(&self, matchday: u32) -> Vec<(CompetitionId, Fixture, Score)> {
        if matchday >= self.matchday {
            return Vec::new();
        }
        let mut out = Vec::new();
        for league in &self.leagues {
            let Some((_, fixtures)) = league.rounds.get(matchday as usize) else {
                continue;
            };
            let offset: usize = league.rounds[..matchday as usize]
                .iter()
                .map(|(_, f)| f.len())
                .sum();
            for (j, fixture) in fixtures.iter().enumerate() {
                if let Some((_, score)) = league.outcome.results.get(offset + j) {
                    out.push((league.competition, *fixture, *score));
                }
            }
        }
        out
    }

    /// `true` se `player` cumpre suspensão na **próxima** rodada (sorteada
    /// ao fim da anterior, ver `crate::discipline`).
    #[must_use]
    pub fn is_suspended(&self, player: PlayerId) -> bool {
        self.leagues.iter().any(|l| l.suspended.contains(&player))
    }
}

/// Uma partida do calendário de um clube (ver [`SeasonRun::fixtures_of`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FixtureEntry {
    /// Rodada (0-based) — índice na ordem de calendário, não o número
    /// interno de `rules::Fixture::round`.
    pub matchday: u32,
    pub competition: CompetitionId,
    pub fixture: Fixture,
    /// `None` se ainda não foi jogada.
    pub score: Option<Score>,
}

/// Um lado de uma partida já preparada: tudo que o motor precisa dele.
#[derive(Debug, Clone)]
pub struct PreparedSide {
    pub club: ClubId,
    /// Perfil final da rodada (moral, adaptação e consistência já aplicadas).
    pub profile: engine::TeamMatchProfile,
    pub tactics: engine::Tactics,
    /// Titulares que entram em campo, já sem lesionados/suspensos e, no
    /// clube do técnico humano, na ordem dos espaços da formação.
    pub starters: Vec<PlayerId>,
}

/// Uma partida da rodada, pronta para ser jogada (ver
/// [`SeasonRun::prepared_fixture_of`]).
#[derive(Debug, Clone)]
pub struct PreparedFixture {
    pub competition: CompetitionId,
    pub fixture: Fixture,
    pub season_index: u32,
    pub home: PreparedSide,
    pub away: PreparedSide,
}

/// Resultado de uma partida decidida **fora** do `SeasonRun` (jogada ao
/// vivo, minuto a minuto): substitui a simulação instantânea daquela
/// partida em [`SeasonRun::commit_matchday`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExternalResult {
    pub fixture: Fixture,
    pub score: Score,
    /// Finalizações `(mandante, visitante)`.
    pub shots: (u32, u32),
}

/// A preparação de uma rodada de uma liga (ver [`prepare_league_round`]).
#[derive(Debug)]
struct LeagueRoundPrep {
    league_index: usize,
    round: u32,
    /// Clubes que jogam, em ordem de calendário.
    order: Vec<ClubId>,
    sides: HashMap<ClubId, PreparedSide>,
    adjustments: Vec<(PlayerId, PlayerId)>,
}

/// Prepara uma rodada de uma liga: perfil de cada clube recalculado a
/// partir de `career.roster` (excluindo quem está suspenso — ver o doc do
/// módulo), com moral/adaptação/consistência aplicadas nessa ordem. Só lê o
/// estado: nada é jogado nem creditado aqui.
#[allow(clippy::too_many_arguments)]
fn prepare_league_round(
    pack: &LoadedPack,
    career: &CareerRound<'_>,
    league_index: usize,
    round: u32,
    fixtures: &[Fixture],
    results_so_far: &[(Fixture, Score)],
    suspended: &HashSet<PlayerId>,
    world_seed: u64,
    season_index: u32,
) -> LeagueRoundPrep {
    // Clubes que jogam nesta rodada, em ordem de calendário (determinística;
    // um clube joga uma vez por rodada).
    let order: Vec<ClubId> = fixtures.iter().flat_map(|f| [f.home, f.away]).collect();
    let mut sides: HashMap<ClubId, PreparedSide> = HashMap::with_capacity(order.len());
    let mut adjustments = Vec::new();
    for &club in &order {
        // Clube com técnico humano: onze e táticas vêm do `ClubSetup`
        // (`crate::setup`); todos os outros seguem o caminho da IA.
        let (profile, tactics, starters) = match career.setups.iter().find(|s| s.club == club) {
            Some(setup) => {
                let xi = crate::setup::resolve_xi(career.roster, setup, suspended);
                adjustments.extend(xi.adjustments.iter().copied());
                let (profile, tactics) = crate::setup::profile_for(pack, career.roster, &xi, setup)
                    .unwrap_or_else(|| {
                        (
                            crate::quality::synthetic_profile(club, world_seed),
                            engine::Tactics::NEUTRAL,
                        )
                    });
                (profile, tactics, xi.players())
            }
            None => (
                crate::quality::profile_from_roster_excluding_or_synthetic(
                    pack,
                    career.roster,
                    club,
                    suspended,
                    world_seed,
                ),
                engine::Tactics::NEUTRAL,
                crate::quality::starters_from_roster_excluding(career.roster, club, suspended),
            ),
        };
        // Moral (RF-JG-07, `crate::morale`): saldo de vitórias/derrotas
        // nas últimas partidas de liga já disputadas nesta temporada
        // (`results_so_far`, só o que já foi simulado até aqui — a
        // rodada atual ainda não entrou). Primeiro, antes dos malus/
        // balanços por jogador abaixo.
        let profile = crate::morale::apply_morale(profile, club, results_so_far);
        // Malus de adaptação (RF-JG-02, `crate::adaptation`): recém-
        // chegado de adaptação baixa rende um pouco menos por até duas
        // temporadas — antes do balanço de consistência, abaixo, pra
        // os dois se combinarem na mesma força final.
        let profile = crate::adaptation::apply_adaptation_penalty(
            profile,
            &starters,
            career.adaptation,
            career.seasons_since_transfer,
        );
        // Balanço de consistência (RF-JG-02, `crate::consistency`):
        // ruído zero-médio por rodada, não um direcionamento como
        // sujeira/jogo importante — ver o doc do módulo.
        let profile = crate::consistency::apply_consistency_swing(
            profile,
            &starters,
            career.consistency,
            world_seed,
            club,
            crate::discipline::season_round_tick(season_index, round),
        );
        sides.insert(
            club,
            PreparedSide {
                club,
                profile,
                tactics,
                starters,
            },
        );
    }
    LeagueRoundPrep {
        league_index,
        round,
        order,
        sides,
        adjustments,
    }
}

/// Conclui uma rodada de uma liga já preparada: simula as partidas (menos a
/// de `external`, cujo resultado vem de fora), credita a bilheteria de cada
/// partida em casa no orçamento do mandante
/// (`crate::finance::match_day_revenue`, RF-CL-03) e sorteia as suspensões
/// da próxima rodada.
#[allow(clippy::too_many_arguments)]
fn complete_league_round(
    career: &mut CareerRound<'_>,
    league: &mut LeagueRun,
    matchday: usize,
    prep: LeagueRoundPrep,
    external: Option<ExternalResult>,
    world_seed: u64,
    season_index: u32,
    report: &mut MatchdayReport,
) {
    let LeagueRun {
        competition,
        rounds,
        outcome,
        suspended,
        ..
    } = league;
    let (round, fixtures) = &rounds[matchday];
    let round = *round;
    debug_assert_eq!(round, prep.round);

    for &fixture in fixtures {
        let (score, shots) = match external {
            Some(ext) if ext.fixture == fixture => (ext.score, ext.shots),
            _ => {
                let home = &prep.sides[&fixture.home];
                let away = &prep.sides[&fixture.away];
                simulate_fixture(
                    home.profile,
                    away.profile,
                    (home.tactics, away.tactics),
                    world_seed,
                    season_index,
                    &fixture,
                )
            }
        };

        let revenue = crate::finance::match_day_revenue(
            fixture.home,
            career.stadium_capacities,
            career.club_reputation,
            world_seed,
            season_index,
            round,
        );
        let idx = fixture.home.as_usize();
        career.budgets[idx] = career.budgets[idx] + revenue;
        outcome.match_day_revenue = outcome.match_day_revenue + revenue;
        report.match_day_revenue = report.match_day_revenue + revenue;

        outcome.record_match(fixture, score, shots);
        report.matches_played += 1;
        report.results.push((*competition, fixture, score));
    }

    let starters_by_club: Vec<(ClubId, Vec<PlayerId>)> = prep
        .order
        .iter()
        .map(|club| (*club, prep.sides[club].starters.clone()))
        .collect();
    *suspended = crate::discipline::roll_suspensions(
        world_seed,
        season_index,
        round,
        &starters_by_club,
        career.dirtiness,
    );
    outcome.suspensions += suspended.len();
    report.suspensions += suspended.len();
    report.lineup_adjustments.extend(prep.adjustments);
}

/// Simula uma partida e devolve o placar e as finalizações de cada lado —
/// o núcleo sensível a RNG compartilhado pelos dois caminhos acima
/// (`engine::simulate`, `ADR 0002`): só existe aqui, nunca duplicado.
fn simulate_fixture(
    home_profile: engine::TeamMatchProfile,
    away_profile: engine::TeamMatchProfile,
    tactics: (engine::Tactics, engine::Tactics),
    world_seed: u64,
    season_index: u32,
    fixture: &Fixture,
) -> (Score, (u32, u32)) {
    let ctx = engine::MatchContext {
        world_seed,
        fixture: fixture_key(season_index, fixture),
    };
    let events = engine::simulate_tactical(home_profile, away_profile, tactics.0, tactics.1, ctx);
    let (home_goals, away_goals) = engine::score(&events);
    let shots = engine::shots(&events);
    (
        Score {
            home_goals,
            away_goals,
        },
        shots,
    )
}

/// Simula `seasons` temporadas em sequência, aplicando promoção e
/// rebaixamento entre elas. O perfil de cada clube é calculado uma vez
/// (`crate::quality::generate_match_profiles`, instantâneo do pack) e
/// permanece constante — nenhuma progressão nem mercado aqui, só
/// `app::GameSession` acompanha estado de carreira entre temporadas; isto
/// é o caminho simples que `managerfc-cli calibrate` usa.
#[must_use]
pub fn run_seasons(pack: &LoadedPack, world_seed: u64, seasons: u32) -> Vec<SeasonResult> {
    let profiles = crate::quality::generate_match_profiles(pack, world_seed);
    let mut membership: Vec<CompetitionId> = pack.clubs.iter().map(|c| c.competition).collect();
    let mut reports = Vec::with_capacity(seasons as usize);

    for season_index in 0..seasons {
        let result = run_season(pack, &membership, &profiles, None, world_seed, season_index);
        for &(club, _from, to) in &result.movements {
            membership[club.as_usize()] = to;
        }
        reports.push(result);
    }
    reports
}

/// Combina temporada + o par (mandante, visitante) num único `u64` para
/// alimentar `MatchContext.fixture` — o único gancho de entropia que
/// `engine::simulate` expõe pra quem chama (ele mesmo deriva sua seed de
/// `(world_seed, "engine.v0.match", fixture, 0)` por dentro).
///
/// `ClubId` já é único **em todo o pack**, não só dentro de uma competição
/// (um clube pertence a exatamente uma competição por vez), então o par
/// `(home, away)` sozinho já distingue qualquer partida de qualquer outra
/// dentro da mesma temporada — inclusive entre competições diferentes.
/// Falta só distinguir a mesma partida repetida em temporadas diferentes
/// (o reencontro anual dos mesmos dois clubes, se nenhum subir/descer);
/// por isso `season_index` entra na mistura. O empacotamento é exato (sem
/// perda, sem colisão) dentro dos limites verificados pelos `debug_assert`
/// — bem acima de qualquer pack real (`docs/03 §9`: ~25 mil clubes no
/// total, e nenhuma calibração roda milhões de temporadas). O empacotamento
/// em si mora em `crate::fixture_key`, compartilhado com `crate::cup`
/// (`cup::cup_fixture_key`) — ver lá o porquê de não duplicar `BITS`.
fn fixture_key(season_index: u32, fixture: &Fixture) -> u64 {
    crate::fixture_key::pack(season_index, fixture.home, fixture.away, false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn example_pack() -> LoadedPack {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/example-two-tier");
        pack::load_and_validate(&path).unwrap().pack
    }

    fn initial_membership(pack: &LoadedPack) -> Vec<CompetitionId> {
        pack.clubs.iter().map(|c| c.competition).collect()
    }

    /// Capacidades de estádio fixas (não vêm de `crate::finance` pra não
    /// acoplar estes testes à sua faixa sintética) — só precisam ser
    /// grandes o bastante para gerar bilheteria positiva em qualquer rodada.
    fn example_capacities(pack: &LoadedPack) -> Vec<u32> {
        vec![20_000; pack.clubs.len()]
    }

    /// Reputação fixa "de meio de tabela" (não vem de `crate::reputation`
    /// pra não acoplar estes testes à sua faixa sintética) — pros testes
    /// que não são sobre o efeito da reputação em si.
    fn example_reputation(pack: &LoadedPack) -> Vec<u8> {
        vec![50; pack.clubs.len()]
    }

    /// Sujeira fixa "neutra" (não vem de `crate::discipline` pra não
    /// acoplar estes testes à sua faixa sintética) — pros testes que não
    /// são sobre o efeito da sujeira em si.
    fn example_dirtiness(pack: &LoadedPack) -> Vec<u8> {
        vec![50; pack.players.len()]
    }

    /// Mentalidade de "jogo importante" fixa "neutra" — mesmo motivo de
    /// `example_dirtiness`.
    fn example_big_game_mentality(pack: &LoadedPack) -> Vec<u8> {
        vec![50; pack.players.len()]
    }

    /// Consistência máxima pra todo mundo — zera o balanço de
    /// `crate::consistency::apply_consistency_swing` (ver o doc do módulo),
    /// pros testes que não são sobre o efeito da consistência em si.
    fn example_consistency(pack: &LoadedPack) -> Vec<u8> {
        vec![100; pack.players.len()]
    }

    /// Adaptação fixa "irrelevante" — mesmo motivo de `example_consistency`.
    fn example_adaptation(pack: &LoadedPack) -> Vec<u8> {
        vec![50; pack.players.len()]
    }

    /// Todo mundo "já assentado" — zera o malus de
    /// `crate::adaptation::apply_adaptation_penalty` (ver o doc do
    /// módulo), pros testes que não são sobre o efeito da adaptação em si.
    fn example_seasons_since_transfer(pack: &LoadedPack) -> Vec<u8> {
        crate::adaptation::initial_seasons_since_transfer(pack)
    }

    #[test]
    fn roda_uma_temporada_no_pack_de_exemplo_sem_travar() {
        let p = example_pack();
        let profiles = crate::quality::generate_match_profiles(&p, 42);
        let membership = initial_membership(&p);
        let result = run_season(&p, &membership, &profiles, None, 42, 0);

        assert_eq!(result.tables.len(), 2); // tier1 e tier2
        // 8 clubes, turno+returno = 8*7 = 56 jogos por competição.
        assert_eq!(result.matches_played, 56 * 2);
        assert_eq!(
            result.home_wins + result.away_wins + result.draws,
            result.matches_played
        );
        assert_eq!(
            result.suspensions, 0,
            "caminho estático (roster: None) nunca simula suspensão"
        );
    }

    #[test]
    fn caminho_de_carreira_produz_as_mesmas_contagens_agregadas_que_o_estatico() {
        // O loop por rodada (`roster: Some`) tem que continuar simulando o
        // mesmo número de partidas e produzindo tabelas do mesmo tamanho
        // que o caminho estático — só a *origem* do perfil por rodada
        // muda, não a estrutura da competição.
        let p = example_pack();
        let profiles = crate::quality::generate_match_profiles(&p, 42);
        let roster = crate::progression::initial_roster(&p);
        let membership = initial_membership(&p);
        let mut budgets = vec![Money::ZERO; p.clubs.len()];
        let capacities = example_capacities(&p);
        let reputation = example_reputation(&p);
        let dirtiness = example_dirtiness(&p);
        let big_game_mentality = example_big_game_mentality(&p);
        let consistency = example_consistency(&p);
        let adaptation = example_adaptation(&p);
        let seasons_since_transfer = example_seasons_since_transfer(&p);
        let career = CareerRound {
            roster: &roster,
            budgets: &mut budgets,
            stadium_capacities: &capacities,
            club_reputation: &reputation,
            dirtiness: &dirtiness,
            big_game_mentality: &big_game_mentality,
            consistency: &consistency,
            adaptation: &adaptation,
            seasons_since_transfer: &seasons_since_transfer,
            setups: &[],
        };
        let result = run_season(&p, &membership, &profiles, Some(career), 42, 0);

        assert_eq!(result.tables.len(), 2);
        assert_eq!(result.matches_played, 56 * 2);
        assert_eq!(
            result.home_wins + result.away_wins + result.draws,
            result.matches_played
        );
        for (_, table) in &result.tables {
            assert_eq!(table.len(), 8);
        }
    }

    #[test]
    fn jogar_rodada_a_rodada_e_identico_a_run_season_e_os_relatorios_somam_o_total() {
        // Garantia central de `SeasonRun`: parar entre rodadas não muda
        // nada — mesmo `SeasonResult`, mesmos orçamentos — e a soma dos
        // `MatchdayReport` fecha com os totais da temporada.
        let p = example_pack();
        let profiles = crate::quality::generate_match_profiles(&p, 42);
        let roster = crate::progression::initial_roster(&p);
        let membership = initial_membership(&p);
        let capacities = example_capacities(&p);
        let reputation = example_reputation(&p);
        let dirtiness = example_dirtiness(&p);
        let big_game_mentality = example_big_game_mentality(&p);
        let consistency = example_consistency(&p);
        let adaptation = example_adaptation(&p);
        let seasons_since_transfer = example_seasons_since_transfer(&p);

        let mut budgets_whole = vec![Money::ZERO; p.clubs.len()];
        let whole = run_season(
            &p,
            &membership,
            &profiles,
            Some(CareerRound {
                roster: &roster,
                budgets: &mut budgets_whole,
                stadium_capacities: &capacities,
                club_reputation: &reputation,
                dirtiness: &dirtiness,
                big_game_mentality: &big_game_mentality,
                consistency: &consistency,
                adaptation: &adaptation,
                seasons_since_transfer: &seasons_since_transfer,
                setups: &[],
            }),
            42,
            0,
        );

        let mut budgets_step = vec![Money::ZERO; p.clubs.len()];
        let mut career = CareerRound {
            roster: &roster,
            budgets: &mut budgets_step,
            stadium_capacities: &capacities,
            club_reputation: &reputation,
            dirtiness: &dirtiness,
            big_game_mentality: &big_game_mentality,
            consistency: &consistency,
            adaptation: &adaptation,
            seasons_since_transfer: &seasons_since_transfer,
            setups: &[],
        };
        let mut run = SeasonRun::start(&p, &membership, 0);
        assert_eq!(run.total_matchdays(), 14, "8 clubes, 2 turnos = 14 rodadas");
        let (mut matches, mut suspensions, mut revenue, mut reported_results) =
            (0u32, 0usize, Money::ZERO, 0usize);
        let mut expected_matchday = 0;
        while !run.is_finished() {
            let report = run.play_matchday(&p, &mut career, 42);
            assert_eq!(report.matchday, expected_matchday);
            assert_eq!(report.results.len() as u32, report.matches_played);
            expected_matchday += 1;
            matches += report.matches_played;
            suspensions += report.suspensions;
            revenue = revenue + report.match_day_revenue;
            reported_results += report.results.len();
        }
        let stepped = run.finish(&p, &profiles, Some(&career), 42);

        assert_eq!(stepped, whole);
        assert_eq!(budgets_step, budgets_whole);
        assert_eq!(matches, stepped.matches_played);
        assert_eq!(reported_results as u32, stepped.matches_played);
        assert_eq!(suspensions, stepped.suspensions);
        assert_eq!(revenue, stepped.match_day_revenue);
    }

    /// Joga `rounds` rodadas de uma temporada de carreira no pack de
    /// exemplo (seed 42) e devolve a execução parada ali.
    fn run_played(rounds: u32) -> (SeasonRun, LoadedPack) {
        let p = example_pack();
        let roster = crate::progression::initial_roster(&p);
        let membership = initial_membership(&p);
        let mut budgets = vec![Money::ZERO; p.clubs.len()];
        let capacities = example_capacities(&p);
        let reputation = example_reputation(&p);
        let dirtiness = example_dirtiness(&p);
        let big_game_mentality = example_big_game_mentality(&p);
        let consistency = example_consistency(&p);
        let adaptation = example_adaptation(&p);
        let seasons_since_transfer = example_seasons_since_transfer(&p);
        let mut career = CareerRound {
            roster: &roster,
            budgets: &mut budgets,
            stadium_capacities: &capacities,
            club_reputation: &reputation,
            dirtiness: &dirtiness,
            big_game_mentality: &big_game_mentality,
            consistency: &consistency,
            adaptation: &adaptation,
            seasons_since_transfer: &seasons_since_transfer,
            setups: &[],
        };
        let mut run = SeasonRun::start(&p, &membership, 0);
        for _ in 0..rounds {
            let _ = run.play_matchday(&p, &mut career, 42);
        }
        (run, p)
    }

    /// Temporada de carreira inteira (seed 42) com `setups` e o `roster`
    /// dado; devolve o resultado, os relatórios das rodadas e os orçamentos.
    fn season_with(
        setups: &[crate::setup::ClubSetup],
        roster: &[PlayerState],
    ) -> (SeasonResult, Vec<MatchdayReport>, Vec<Money>) {
        let p = example_pack();
        let profiles = crate::quality::generate_match_profiles(&p, 42);
        let membership = initial_membership(&p);
        let mut budgets = vec![Money::ZERO; p.clubs.len()];
        let capacities = example_capacities(&p);
        let reputation = example_reputation(&p);
        let dirtiness = example_dirtiness(&p);
        let big_game_mentality = example_big_game_mentality(&p);
        let consistency = example_consistency(&p);
        let adaptation = example_adaptation(&p);
        let seasons_since_transfer = example_seasons_since_transfer(&p);
        let mut career = CareerRound {
            roster,
            budgets: &mut budgets,
            stadium_capacities: &capacities,
            club_reputation: &reputation,
            dirtiness: &dirtiness,
            big_game_mentality: &big_game_mentality,
            consistency: &consistency,
            adaptation: &adaptation,
            seasons_since_transfer: &seasons_since_transfer,
            setups,
        };
        let mut run = SeasonRun::start(&p, &membership, 0);
        let mut reports = Vec::new();
        while !run.is_finished() {
            reports.push(run.play_matchday(&p, &mut career, 42));
        }
        let result = run.finish(&p, &profiles, Some(&career), 42);
        (result, reports, budgets)
    }

    /// Roda a temporada de carreira inteira (seed 42, pack de exemplo)
    /// preparando cada rodada antes de concluí-la; `decide` recebe a partida
    /// preparada do clube 0 e devolve o resultado "ao vivo" a injetar.
    fn season_with_live(
        decide: impl Fn(&PreparedFixture) -> Option<ExternalResult>,
    ) -> (SeasonResult, Vec<Money>) {
        let p = example_pack();
        let profiles = crate::quality::generate_match_profiles(&p, 42);
        let roster = crate::progression::initial_roster(&p);
        let membership = initial_membership(&p);
        let mut budgets = vec![Money::ZERO; p.clubs.len()];
        let capacities = example_capacities(&p);
        let reputation = example_reputation(&p);
        let dirtiness = example_dirtiness(&p);
        let big_game_mentality = example_big_game_mentality(&p);
        let consistency = example_consistency(&p);
        let adaptation = example_adaptation(&p);
        let seasons_since_transfer = example_seasons_since_transfer(&p);
        let mut career = CareerRound {
            roster: &roster,
            budgets: &mut budgets,
            stadium_capacities: &capacities,
            club_reputation: &reputation,
            dirtiness: &dirtiness,
            big_game_mentality: &big_game_mentality,
            consistency: &consistency,
            adaptation: &adaptation,
            seasons_since_transfer: &seasons_since_transfer,
            setups: &[],
        };
        let club = p.clubs[0].id;
        let mut run = SeasonRun::start(&p, &membership, 0);
        while !run.is_finished() {
            run.begin_matchday(&p, &career, 42);
            assert!(run.is_matchday_prepared());
            let external = run.prepared_fixture_of(club).and_then(|f| decide(&f));
            let _ = run.commit_matchday(&p, &mut career, 42, external);
            assert!(!run.is_matchday_prepared());
        }
        let result = run.finish(&p, &profiles, Some(&career), 42);
        (result, budgets)
    }

    /// O resultado que o motor daria sozinho para a partida preparada.
    fn engine_result(f: &PreparedFixture) -> ExternalResult {
        let ctx = engine::MatchContext {
            world_seed: 42,
            fixture: fixture_key(f.season_index, &f.fixture),
        };
        let events = engine::simulate_tactical(
            f.home.profile,
            f.away.profile,
            f.home.tactics,
            f.away.tactics,
            ctx,
        );
        let (home_goals, away_goals) = engine::score(&events);
        ExternalResult {
            fixture: f.fixture,
            score: Score {
                home_goals,
                away_goals,
            },
            shots: engine::shots(&events),
        }
    }

    #[test]
    fn preparar_antes_e_concluir_e_identico_a_jogar_direto() {
        let p = example_pack();
        let roster = crate::progression::initial_roster(&p);
        let direct = season_with(&[], &roster);
        let prepared = season_with_live(|_| None);
        assert_eq!(direct.0, prepared.0);
        assert_eq!(direct.2, prepared.1);
    }

    #[test]
    fn resultado_ao_vivo_igual_ao_do_motor_nao_muda_a_temporada() {
        let direct = season_with_live(|_| None);
        let live = season_with_live(|f| Some(engine_result(f)));
        assert_eq!(direct, live);
    }

    #[test]
    fn resultado_ao_vivo_diferente_substitui_so_aquela_partida() {
        let base = season_with_live(|_| None).0;
        let rigged = season_with_live(|f| {
            (f.season_index == 0 && f.fixture.round == 0).then_some(ExternalResult {
                fixture: f.fixture,
                score: Score {
                    home_goals: 9,
                    away_goals: 0,
                },
                shots: (20, 1),
            })
        })
        .0;
        assert_ne!(base, rigged);
        assert_eq!(base.matches_played, rigged.matches_played);
        assert!(rigged.total_goals > base.total_goals || rigged.total_shots != base.total_shots);
    }

    #[test]
    fn partida_preparada_traz_perfis_e_onzes_dos_dois_lados() {
        let (mut run, p) = run_played(2);
        let roster = crate::progression::initial_roster(&p);
        let membership = initial_membership(&p);
        let _ = membership;
        let budgets_len = p.clubs.len();
        let mut budgets = vec![Money::ZERO; budgets_len];
        let caps = example_capacities(&p);
        let rep = example_reputation(&p);
        let dirt = example_dirtiness(&p);
        let big = example_big_game_mentality(&p);
        let cons = example_consistency(&p);
        let adapt = example_adaptation(&p);
        let sst = example_seasons_since_transfer(&p);
        let career = CareerRound {
            roster: &roster,
            budgets: &mut budgets,
            stadium_capacities: &caps,
            club_reputation: &rep,
            dirtiness: &dirt,
            big_game_mentality: &big,
            consistency: &cons,
            adaptation: &adapt,
            seasons_since_transfer: &sst,
            setups: &[],
        };
        assert!(
            run.prepared_fixture_of(p.clubs[0].id).is_none(),
            "antes de preparar"
        );
        run.begin_matchday(&p, &career, 42);
        run.begin_matchday(&p, &career, 42); // idempotente
        let f = run.prepared_fixture_of(p.clubs[0].id).unwrap();
        assert_eq!(f.home.starters.len(), 11);
        assert_eq!(f.away.starters.len(), 11);
        assert!(f.home.club != f.away.club);
        assert!(f.home.club == p.clubs[0].id || f.away.club == p.clubs[0].id);
        assert_eq!(f.season_index, 0);
    }

    #[test]
    fn setup_padrao_do_tecnico_nao_muda_nada_na_temporada() {
        let p = example_pack();
        let roster = crate::progression::initial_roster(&p);
        let club = p.clubs[0].id;
        let none = season_with(&[], &roster);
        let default = season_with(&[crate::setup::ClubSetup::new(club)], &roster);
        assert_eq!(none.0, default.0);
        assert_eq!(none.2, default.2);
        assert!(default.1.iter().all(|r| r.lineup_adjustments.is_empty()));
    }

    #[test]
    fn mentalidade_ofensiva_do_clube_do_usuario_muda_de_verdade_a_temporada() {
        let p = example_pack();
        let roster = crate::progression::initial_roster(&p);
        let club = p.clubs[0].id;
        let mut attacking = crate::setup::ClubSetup::new(club);
        attacking.mentality = crate::setup::Mentality::Attacking;
        attacking.formation = ai::Formation::F433;
        let base = season_with(&[], &roster).0;
        let tuned = season_with(&[attacking], &roster).0;
        assert_ne!(base, tuned, "tática não pode ser decorativa");
        assert!(
            tuned.total_shots > base.total_shots,
            "ataque total cria e cede mais finalizações: {} vs {}",
            tuned.total_shots,
            base.total_shots
        );
        assert_eq!(base.matches_played, tuned.matches_played);
    }

    #[test]
    fn titular_manual_lesionado_e_substituido_e_a_troca_e_reportada() {
        let p = example_pack();
        let mut roster = crate::progression::initial_roster(&p);
        let club = p.clubs[0].id;
        let mut setup = crate::setup::ClubSetup::new(club);
        let xi = crate::setup::resolve_xi(&roster, &setup, &HashSet::new());
        setup.starters = Some(xi.players());
        let hurt = xi.slots[4].1;
        roster[hurt.as_usize()].injured = true;

        let (_, reports, _) = season_with(&[setup], &roster);
        for report in &reports {
            assert_eq!(report.lineup_adjustments.len(), 1, "uma troca por rodada");
            assert_eq!(report.lineup_adjustments[0].0, hurt);
        }
    }

    #[test]
    fn calendario_do_clube_tem_placar_so_nas_rodadas_ja_jogadas() {
        let (run, p) = run_played(5);
        let club = p.clubs[0].id;
        let fixtures = run.fixtures_of(club);
        assert_eq!(fixtures.len(), 14, "8 clubes, 2 turnos");
        for (i, entry) in fixtures.iter().enumerate() {
            assert_eq!(entry.matchday, i as u32, "uma partida por rodada");
            assert!(entry.fixture.home == club || entry.fixture.away == club);
            assert_eq!(entry.score.is_some(), i < 5, "placar só até a rodada 5");
        }
    }

    #[test]
    fn calendario_completo_fecha_com_a_tabela_final() {
        let (run, p) = run_played(14);
        assert!(run.is_finished());
        let club = p.clubs[0].id;
        let points: u32 = run
            .fixtures_of(club)
            .iter()
            .map(|e| {
                let score = e.score.expect("temporada completa, todo jogo tem placar");
                let (mine, theirs) = if e.fixture.home == club {
                    (score.home_goals, score.away_goals)
                } else {
                    (score.away_goals, score.home_goals)
                };
                match mine.cmp(&theirs) {
                    Ordering::Greater => 3,
                    Ordering::Equal => 1,
                    Ordering::Less => 0,
                }
            })
            .sum();
        let profiles = crate::quality::generate_match_profiles(&p, 42);
        let result = run.finish(&p, &profiles, None, 42);
        let row = result
            .tables
            .iter()
            .flat_map(|(_, t)| t.iter())
            .find(|r| r.club == club)
            .unwrap();
        assert_eq!(points, row.points());
    }

    #[test]
    fn resultados_da_rodada_so_existem_para_rodadas_jogadas() {
        let (run, _p) = run_played(3);
        assert_eq!(run.round_results(0).len(), 8, "4 jogos por liga, 2 ligas");
        assert_eq!(run.round_results(2).len(), 8);
        assert!(run.round_results(3).is_empty(), "rodada 3 ainda não jogada");
        assert!(run.round_results(99).is_empty());
    }

    #[test]
    fn ninguem_esta_suspenso_antes_da_primeira_rodada() {
        let (run, p) = run_played(0);
        assert!(p.players.iter().all(|pl| !run.is_suspended(pl.id)));
    }

    #[test]
    fn rodada_alem_do_fim_da_temporada_e_no_op() {
        let p = example_pack();
        let roster = crate::progression::initial_roster(&p);
        let membership = initial_membership(&p);
        let mut budgets = vec![Money::ZERO; p.clubs.len()];
        let capacities = example_capacities(&p);
        let reputation = example_reputation(&p);
        let dirtiness = example_dirtiness(&p);
        let big_game_mentality = example_big_game_mentality(&p);
        let consistency = example_consistency(&p);
        let adaptation = example_adaptation(&p);
        let seasons_since_transfer = example_seasons_since_transfer(&p);
        let mut career = CareerRound {
            roster: &roster,
            budgets: &mut budgets,
            stadium_capacities: &capacities,
            club_reputation: &reputation,
            dirtiness: &dirtiness,
            big_game_mentality: &big_game_mentality,
            consistency: &consistency,
            adaptation: &adaptation,
            seasons_since_transfer: &seasons_since_transfer,
            setups: &[],
        };
        let mut run = SeasonRun::start(&p, &membership, 0);
        while !run.is_finished() {
            let _ = run.play_matchday(&p, &mut career, 42);
        }
        let extra = run.play_matchday(&p, &mut career, 42);
        assert_eq!(extra.matches_played, 0);
        assert!(extra.results.is_empty());
        assert_eq!(run.next_matchday(), run.total_matchdays());
    }

    #[test]
    fn caminho_de_carreira_e_deterministico_para_a_mesma_entrada() {
        let p = example_pack();
        let profiles = crate::quality::generate_match_profiles(&p, 7);
        let roster = crate::progression::initial_roster(&p);
        let membership = initial_membership(&p);
        let capacities = example_capacities(&p);
        let reputation = example_reputation(&p);
        let dirtiness = example_dirtiness(&p);
        let big_game_mentality = example_big_game_mentality(&p);
        let consistency = example_consistency(&p);
        let adaptation = example_adaptation(&p);
        let seasons_since_transfer = example_seasons_since_transfer(&p);

        let mut budgets_a = vec![Money::ZERO; p.clubs.len()];
        let career_a = CareerRound {
            roster: &roster,
            budgets: &mut budgets_a,
            stadium_capacities: &capacities,
            club_reputation: &reputation,
            dirtiness: &dirtiness,
            big_game_mentality: &big_game_mentality,
            consistency: &consistency,
            adaptation: &adaptation,
            seasons_since_transfer: &seasons_since_transfer,
            setups: &[],
        };
        let a = run_season(&p, &membership, &profiles, Some(career_a), 7, 0);

        let mut budgets_b = vec![Money::ZERO; p.clubs.len()];
        let career_b = CareerRound {
            roster: &roster,
            budgets: &mut budgets_b,
            stadium_capacities: &capacities,
            club_reputation: &reputation,
            dirtiness: &dirtiness,
            big_game_mentality: &big_game_mentality,
            consistency: &consistency,
            adaptation: &adaptation,
            seasons_since_transfer: &seasons_since_transfer,
            setups: &[],
        };
        let b = run_season(&p, &membership, &profiles, Some(career_b), 7, 0);
        assert_eq!(a, b);
        assert_eq!(budgets_a, budgets_b);
    }

    #[test]
    fn suspensoes_de_fato_acontecem_em_varias_temporadas_de_carreira() {
        // Sanidade estatística (como `world::injuries`'s
        // `em_um_elenco_grande_algum_jogador_se_machuca`): ~4% por time por
        // partida, 16 clubes, 14 partidas de liga por clube por temporada —
        // esperar zero suspensões em 10 temporadas seria extremamente
        // improvável se `crate::discipline` estivesse de fato ligado ao
        // loop por rodada.
        let p = example_pack();
        let roster = crate::progression::initial_roster(&p);
        let membership = initial_membership(&p);
        let profiles = crate::quality::generate_match_profiles(&p, 1);
        let capacities = example_capacities(&p);
        let reputation = example_reputation(&p);
        let dirtiness = example_dirtiness(&p);
        let big_game_mentality = example_big_game_mentality(&p);
        let consistency = example_consistency(&p);
        let adaptation = example_adaptation(&p);
        let seasons_since_transfer = example_seasons_since_transfer(&p);

        let total_suspensions: usize = (0..10)
            .map(|season_index| {
                let mut budgets = vec![Money::ZERO; p.clubs.len()];
                let career = CareerRound {
                    roster: &roster,
                    budgets: &mut budgets,
                    stadium_capacities: &capacities,
                    club_reputation: &reputation,
                    dirtiness: &dirtiness,
                    big_game_mentality: &big_game_mentality,
                    consistency: &consistency,
                    adaptation: &adaptation,
                    seasons_since_transfer: &seasons_since_transfer,
                    setups: &[],
                };
                run_season(&p, &membership, &profiles, Some(career), 1, season_index).suspensions
            })
            .sum();
        assert!(
            total_suspensions > 0,
            "esperava pelo menos uma suspensão em 10 temporadas no pack de exemplo"
        );
    }

    #[test]
    fn caminho_de_carreira_credita_bilheteria_no_orcamento_do_mandante() {
        // Caixa branca: prova que `crate::finance::match_day_revenue` está
        // de fato ligado ao loop por rodada, não só existe isolado em
        // `crate::finance` — todo orçamento tem que crescer (todo clube é
        // mandante em algumas partidas do turno+returno).
        let p = example_pack();
        let roster = crate::progression::initial_roster(&p);
        let membership = initial_membership(&p);
        let profiles = crate::quality::generate_match_profiles(&p, 42);
        let capacities = example_capacities(&p);
        let reputation = example_reputation(&p);
        let dirtiness = example_dirtiness(&p);
        let big_game_mentality = example_big_game_mentality(&p);
        let consistency = example_consistency(&p);
        let adaptation = example_adaptation(&p);
        let seasons_since_transfer = example_seasons_since_transfer(&p);
        let mut budgets = vec![Money::ZERO; p.clubs.len()];

        let career = CareerRound {
            roster: &roster,
            budgets: &mut budgets,
            stadium_capacities: &capacities,
            club_reputation: &reputation,
            dirtiness: &dirtiness,
            big_game_mentality: &big_game_mentality,
            consistency: &consistency,
            adaptation: &adaptation,
            seasons_since_transfer: &seasons_since_transfer,
            setups: &[],
        };
        let result = run_season(&p, &membership, &profiles, Some(career), 42, 0);

        assert!(result.match_day_revenue.cents() > 0);
        for &b in &budgets {
            assert!(
                b.cents() > 0,
                "todo clube deveria ter recebido bilheteria em algum turno+returno"
            );
        }
        let total_in_budgets: i64 = budgets.iter().map(|b| b.cents()).sum();
        assert_eq!(total_in_budgets, result.match_day_revenue.cents());
    }

    #[test]
    fn reputacao_de_fato_influencia_a_bilheteria_no_loop_por_rodada() {
        // Caixa branca: prova que `crate::finance::match_day_revenue` está
        // de fato recebendo a reputação do loop por rodada (não só existe
        // isolada em `crate::finance`) — mesmo roster/capacidades, só
        // reputação alta (sempre) vs. baixa (sempre) muda, a bilheteria
        // total tem que ser maior com reputação alta.
        let p = example_pack();
        let roster = crate::progression::initial_roster(&p);
        let membership = initial_membership(&p);
        let profiles = crate::quality::generate_match_profiles(&p, 42);
        let capacities = example_capacities(&p);
        let dirtiness = example_dirtiness(&p);
        let big_game_mentality = example_big_game_mentality(&p);
        let consistency = example_consistency(&p);
        let adaptation = example_adaptation(&p);
        let seasons_since_transfer = example_seasons_since_transfer(&p);

        let high_reputation = vec![100; p.clubs.len()];
        let mut budgets_high = vec![Money::ZERO; p.clubs.len()];
        let career_high = CareerRound {
            roster: &roster,
            budgets: &mut budgets_high,
            stadium_capacities: &capacities,
            club_reputation: &high_reputation,
            dirtiness: &dirtiness,
            big_game_mentality: &big_game_mentality,
            consistency: &consistency,
            adaptation: &adaptation,
            seasons_since_transfer: &seasons_since_transfer,
            setups: &[],
        };
        let high = run_season(&p, &membership, &profiles, Some(career_high), 42, 0);

        let low_reputation = vec![0; p.clubs.len()];
        let mut budgets_low = vec![Money::ZERO; p.clubs.len()];
        let career_low = CareerRound {
            roster: &roster,
            budgets: &mut budgets_low,
            stadium_capacities: &capacities,
            club_reputation: &low_reputation,
            dirtiness: &dirtiness,
            big_game_mentality: &big_game_mentality,
            consistency: &consistency,
            adaptation: &adaptation,
            seasons_since_transfer: &seasons_since_transfer,
            setups: &[],
        };
        let low = run_season(&p, &membership, &profiles, Some(career_low), 42, 0);

        assert!(high.match_day_revenue > low.match_day_revenue);
    }

    #[test]
    fn caminho_estatico_nunca_credita_bilheteria() {
        let p = example_pack();
        let profiles = crate::quality::generate_match_profiles(&p, 42);
        let membership = initial_membership(&p);
        let result = run_season(&p, &membership, &profiles, None, 42, 0);
        assert_eq!(result.match_day_revenue, Money::ZERO);
    }

    #[test]
    fn soma_de_pontos_bate_com_i5_no_pack_de_exemplo() {
        let p = example_pack();
        let profiles = crate::quality::generate_match_profiles(&p, 7);
        let membership = initial_membership(&p);
        let result = run_season(&p, &membership, &profiles, None, 7, 0);

        for (_, table) in &result.tables {
            let total_wins: u32 = table.iter().map(|r| r.wins).sum();
            let total_draws: u32 = table.iter().map(|r| r.draws).sum();
            let total_points: u32 = table.iter().map(TableRow::points).sum();
            assert_eq!(total_points, total_wins * 3 + total_draws);
        }
    }

    #[test]
    fn promocao_e_rebaixamento_movem_clubes_entre_temporadas() {
        let p = example_pack();
        let reports = run_seasons(&p, 42, 2);

        // tier1 rebaixa 2, tier2 promove 2 (docs do pack de exemplo) — a
        // primeira temporada tem que gerar exatamente 4 movimentos.
        assert_eq!(reports[0].movements.len(), 4);

        // A composição de pelo menos uma das competições muda na segunda
        // temporada em relação à primeira (algum clube que era de um lado
        // agora está do outro).
        let tier1_id = p
            .competitions
            .iter()
            .find(|c| c.external_id == "es.tier1")
            .unwrap()
            .id;
        let season0_tier1: Vec<ClubId> = reports[0]
            .tables
            .iter()
            .find(|(id, _)| *id == tier1_id)
            .unwrap()
            .1
            .iter()
            .map(|r| r.club)
            .collect();
        let season1_tier1: Vec<ClubId> = reports[1]
            .tables
            .iter()
            .find(|(id, _)| *id == tier1_id)
            .unwrap()
            .1
            .iter()
            .map(|r| r.club)
            .collect();

        let mut a = season0_tier1.clone();
        let mut b = season1_tier1.clone();
        a.sort();
        b.sort();
        assert_ne!(
            a, b,
            "elenco da 1ª divisão deveria mudar após promoção/rebaixamento"
        );
    }

    #[test]
    fn tamanho_das_competicoes_permanece_equilibrado_apos_promocao_e_rebaixamento() {
        // O pack de exemplo rebaixa e promove exatamente 2 de cada lado —
        // balanceado por construção. Se isso um dia ficar desbalanceado
        // (pack mal configurado), é aqui que apareceria: as duas
        // competições têm que continuar com 8 clubes cada, sempre.
        let p = example_pack();
        let reports = run_seasons(&p, 1, 5);
        for report in &reports {
            for (_, table) in &report.tables {
                assert_eq!(table.len(), 8);
            }
        }
    }

    #[test]
    fn mesma_seed_produz_a_mesma_sequencia_de_temporadas() {
        let p = example_pack();
        let a = run_seasons(&p, 123, 5);
        let b = run_seasons(&p, 123, 5);
        assert_eq!(a, b);
    }

    #[test]
    fn seeds_diferentes_produzem_temporadas_diferentes() {
        let p = example_pack();
        let a = run_seasons(&p, 1, 3);
        let b = run_seasons(&p, 2, 3);
        assert_ne!(a, b);
    }

    #[test]
    fn tabelas_nao_ficam_identicas_ao_longo_das_temporadas() {
        // Sanidade grosseira: nem a composição (promoção/rebaixamento) nem
        // os placares deveriam produzir a mesma tabela 3 temporadas
        // seguidas. Não isola a causa (é `fixture_key_muda_com_a_temporada`,
        // abaixo, que testa isso de forma direta) — só garante que "mundo
        // congelado" não passa despercebido no nível mais alto.
        let p = example_pack();
        let reports = run_seasons(&p, 99, 3);
        let tier1_id = p
            .competitions
            .iter()
            .find(|c| c.external_id == "es.tier1")
            .unwrap()
            .id;

        let tables_by_season: Vec<Vec<TableRow>> = reports
            .iter()
            .map(|r| {
                r.tables
                    .iter()
                    .find(|(id, _)| *id == tier1_id)
                    .unwrap()
                    .1
                    .clone()
            })
            .collect();

        let all_identical = tables_by_season.windows(2).all(|w| w[0] == w[1]);
        assert!(
            !all_identical,
            "tabelas idênticas em todas as temporadas — mundo parado?"
        );
    }

    #[test]
    fn fixture_key_muda_com_a_temporada() {
        // O teste direto do que `tabelas_nao_ficam_identicas...` só observa
        // de longe: sem `season_index` na mistura, o mesmo confronto
        // repetido ano após ano teria a mesma seed — e portanto o mesmo
        // placar sempre, o que tornaria o mundo previsível demais para
        // qualquer competição de mais de 1 temporada.
        let f = Fixture {
            round: 0,
            home: ClubId::new(1),
            away: ClubId::new(2),
        };
        let k0 = fixture_key(0, &f);
        let k1 = fixture_key(1, &f);
        assert_ne!(k0, k1);
    }

    #[test]
    fn fixture_key_depende_do_par_de_clubes_nao_da_rodada() {
        // O par (home, away) já é globalmente único no pack inteiro — a
        // rodada não precisa (e não entra) na chave.
        let f1 = Fixture {
            round: 0,
            home: ClubId::new(1),
            away: ClubId::new(2),
        };
        let f2 = Fixture {
            round: 5,
            home: ClubId::new(1),
            away: ClubId::new(2),
        };
        assert_eq!(fixture_key(0, &f1), fixture_key(0, &f2));

        let f3 = Fixture {
            round: 0,
            home: ClubId::new(2),
            away: ClubId::new(1),
        }; // par invertido
        assert_ne!(fixture_key(0, &f1), fixture_key(0, &f3));
    }
}

/// Testes de propriedade (`docs/08 §1`).
#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;
    use std::path::PathBuf;

    fn example_pack() -> LoadedPack {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/example-two-tier");
        pack::load_and_validate(&path).unwrap().pack
    }

    proptest! {
        /// `fixture_key` empacota exatamente (sem perda) dentro dos limites
        /// documentados — decompor a chave devolve os três valores originais.
        #[test]
        fn fixture_key_e_reversivel_dentro_dos_limites(
            season in 0u32..(1 << crate::fixture_key::BITS),
            home in 0u32..(1 << crate::fixture_key::BITS),
            away in 0u32..(1 << crate::fixture_key::BITS),
        ) {
            let f = Fixture { round: 0, home: ClubId::new(home), away: ClubId::new(away) };
            let key = fixture_key(season, &f);

            const BITS: u32 = crate::fixture_key::BITS;
            const MASK: u64 = (1 << BITS) - 1;
            let decoded_away = (key & MASK) as u32;
            let decoded_home = ((key >> BITS) & MASK) as u32;
            let decoded_season = (key >> (2 * BITS)) as u32;

            prop_assert_eq!(decoded_away, away);
            prop_assert_eq!(decoded_home, home);
            prop_assert_eq!(decoded_season, season);
        }

        /// Generaliza `mesma_seed_produz_a_mesma_sequencia_de_temporadas`
        /// para seeds arbitrárias, não só o exemplo fixo (123).
        #[test]
        fn run_seasons_e_deterministico_para_qualquer_seed(world_seed: u64) {
            let p = example_pack();
            let a = run_seasons(&p, world_seed, 2);
            let b = run_seasons(&p, world_seed, 2);
            prop_assert_eq!(a, b);
        }
    }
}
