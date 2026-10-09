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
//! já existir. Cup (`crate::cup`) não ganha essa granularidade — continua
//! usando o perfil estático do início da temporada em qualquer um dos dois
//! casos, e não gera bilheteria nem é afetada por suspensão (mesma exclusão
//! de `crate::condition`).

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
/// para copa, e para liga **apenas** quando `roster` é `None`.
///
/// `career`, quando `Some`, muda como toda competição `Format::RoundRobin`
/// é simulada: rodada a rodada, recalculando o perfil de cada clube que
/// joga a partir do roster atual (excluindo quem está suspenso — ver o doc
/// do módulo) em vez de usar `profiles` a temporada inteira, e creditando
/// bilheteria de cada partida em casa no orçamento do mandante
/// (`crate::finance::match_day_revenue`). `None` é exatamente o
/// comportamento de antes desta fatia (`managerfc-cli calibrate`/`bench`,
/// via [`run_seasons`]).
#[must_use]
pub fn run_season(
    pack: &LoadedPack,
    membership: &[CompetitionId],
    profiles: &[engine::TeamMatchProfile],
    mut career: Option<CareerRound<'_>>,
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
            Format::RoundRobin { legs, .. } => {
                // Clubes atualmente na competição, em ordem de id denso —
                // nunca via iteração de um `HashMap` (`ADR 0002`: calendário
                // e tabela não podem depender de ordem não-determinística).
                let clubs: Vec<ClubId> = membership
                    .iter()
                    .enumerate()
                    .filter(|&(_, &comp)| comp == competition.id)
                    .map(|(idx, _)| ClubId::new(idx as u32))
                    .collect();

                let Ok(fixtures) = rules::round_robin(&clubs, legs) else {
                    // Número de clubes ímpar ou < 2 na competição — não
                    // deveria acontecer com um pack validado (`docs/03 §7`),
                    // mas também pode surgir se `promotion.slots`/
                    // `relegation.slots` de duas competições emparelhadas
                    // não baterem entre si (validação que o `pack` ainda não
                    // faz — dívida técnica conhecida, não um `panic!`
                    // escondido). Pular a competição nesta temporada é mais
                    // seguro que travar o mundo inteiro por causa de uma liga.
                    continue;
                };

                let outcome = match career.as_mut() {
                    None => {
                        simulate_round_robin_static(profiles, fixtures, world_seed, season_index)
                    }
                    Some(career) => simulate_round_robin_for_career(
                        pack,
                        career,
                        fixtures,
                        world_seed,
                        season_index,
                    ),
                };
                matches_played += outcome.matches_played;
                home_wins += outcome.home_wins;
                away_wins += outcome.away_wins;
                draws += outcome.draws;
                total_goals += outcome.total_goals;
                total_shots += outcome.total_shots;
                suspensions += outcome.suspensions;
                match_day_revenue = match_day_revenue + outcome.match_day_revenue;

                let table =
                    rules::compute_table(&clubs, &outcome.results, &competition.tiebreakers);

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
                // suspensão não afeta copa (doc do módulo).
                let participants = crate::cup::cup_participants(pack, competition);
                if let Some(result) = crate::cup::run_cup(
                    competition.id,
                    &participants,
                    profiles,
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
#[derive(Default)]
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
        let (score, shots) = simulate_fixture(home, away, world_seed, season_index, &fixture);
        outcome.record_match(fixture, score, shots);
    }
    outcome
}

/// Simula um turno/returno inteiro **rodada a rodada**: o perfil de cada
/// clube que joga é recalculado a cada rodada a partir de `career.roster`,
/// excluindo quem está suspenso (ver o doc do módulo) — é o que torna
/// `crate::discipline::roll_suspensions` capaz de ter efeito de verdade na
/// simulação, não só existir isolado. Suspensão nunca atravessa rodada de
/// competições diferentes nem temporada (reinicia vazia a cada chamada).
/// Cada partida credita bilheteria no orçamento do mandante
/// (`crate::finance::match_day_revenue`, RF-CL-03).
fn simulate_round_robin_for_career(
    pack: &LoadedPack,
    career: &mut CareerRound<'_>,
    fixtures: Vec<Fixture>,
    world_seed: u64,
    season_index: u32,
) -> RoundRobinOutcome {
    let mut outcome = RoundRobinOutcome::default();
    let mut fixtures_by_round: BTreeMap<u32, Vec<Fixture>> = BTreeMap::new();
    for fixture in fixtures {
        fixtures_by_round
            .entry(fixture.round)
            .or_default()
            .push(fixture);
    }

    let mut suspended: HashSet<PlayerId> = HashSet::new();
    for (round, round_fixtures) in fixtures_by_round {
        let playing_clubs: HashSet<ClubId> = round_fixtures
            .iter()
            .flat_map(|f| [f.home, f.away])
            .collect();

        let mut round_profiles: HashMap<ClubId, engine::TeamMatchProfile> = HashMap::new();
        let mut starters_by_club: Vec<(ClubId, Vec<PlayerId>)> =
            Vec::with_capacity(playing_clubs.len());
        for &club in &playing_clubs {
            let profile = crate::quality::profile_from_roster_excluding_or_synthetic(
                pack,
                career.roster,
                club,
                &suspended,
                world_seed,
            );
            round_profiles.insert(club, profile);
            let starters =
                crate::quality::starters_from_roster_excluding(career.roster, club, &suspended);
            starters_by_club.push((club, starters));
        }

        for fixture in round_fixtures {
            let home = round_profiles[&fixture.home];
            let away = round_profiles[&fixture.away];
            let (score, shots) = simulate_fixture(home, away, world_seed, season_index, &fixture);

            let revenue = crate::finance::match_day_revenue(
                fixture.home,
                career.stadium_capacities,
                world_seed,
                season_index,
                round,
            );
            let idx = fixture.home.as_usize();
            career.budgets[idx] = career.budgets[idx] + revenue;
            outcome.match_day_revenue = outcome.match_day_revenue + revenue;

            outcome.record_match(fixture, score, shots);
        }

        suspended =
            crate::discipline::roll_suspensions(world_seed, season_index, round, &starters_by_club);
        outcome.suspensions += suspended.len();
    }
    outcome
}

/// Simula uma partida e devolve o placar e as finalizações de cada lado —
/// o núcleo sensível a RNG compartilhado pelos dois caminhos acima
/// (`engine::simulate`, `ADR 0002`): só existe aqui, nunca duplicado.
fn simulate_fixture(
    home_profile: engine::TeamMatchProfile,
    away_profile: engine::TeamMatchProfile,
    world_seed: u64,
    season_index: u32,
    fixture: &Fixture,
) -> (Score, (u32, u32)) {
    let ctx = engine::MatchContext {
        world_seed,
        fixture: fixture_key(season_index, fixture),
    };
    let events = engine::simulate(home_profile, away_profile, ctx);
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
        let career = CareerRound {
            roster: &roster,
            budgets: &mut budgets,
            stadium_capacities: &capacities,
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
    fn caminho_de_carreira_e_deterministico_para_a_mesma_entrada() {
        let p = example_pack();
        let profiles = crate::quality::generate_match_profiles(&p, 7);
        let roster = crate::progression::initial_roster(&p);
        let membership = initial_membership(&p);
        let capacities = example_capacities(&p);

        let mut budgets_a = vec![Money::ZERO; p.clubs.len()];
        let career_a = CareerRound {
            roster: &roster,
            budgets: &mut budgets_a,
            stadium_capacities: &capacities,
        };
        let a = run_season(&p, &membership, &profiles, Some(career_a), 7, 0);

        let mut budgets_b = vec![Money::ZERO; p.clubs.len()];
        let career_b = CareerRound {
            roster: &roster,
            budgets: &mut budgets_b,
            stadium_capacities: &capacities,
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

        let total_suspensions: usize = (0..10)
            .map(|season_index| {
                let mut budgets = vec![Money::ZERO; p.clubs.len()];
                let career = CareerRound {
                    roster: &roster,
                    budgets: &mut budgets,
                    stadium_capacities: &capacities,
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
        let mut budgets = vec![Money::ZERO; p.clubs.len()];

        let career = CareerRound {
            roster: &roster,
            budgets: &mut budgets,
            stadium_capacities: &capacities,
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
