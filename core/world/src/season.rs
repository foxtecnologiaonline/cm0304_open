//! Loop de temporada — gera o calendário de cada competição
//! ([`rules::round_robin`]), simula cada confronto ([`engine::simulate`]) e
//! calcula a tabela final ([`rules::compute_table`]), aplicando promoção e
//! rebaixamento para a temporada seguinte. É o que fecha o portão do M1:
//! "um mundo que gira sozinho, sem uma única tela"
//! (`docs/07-roadmap.md#m1--kick-off-headless-10-semanas`).

use std::cmp::Ordering;

use domain::{ClubId, CompetitionId};
use pack::{Format, LoadedPack};
use rules::{Fixture, Score, TableRow};

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
/// já cuida disso). `strengths` vem de
/// [`crate::strength::generate_strengths`].
#[must_use]
pub fn run_season(
    pack: &LoadedPack,
    membership: &[CompetitionId],
    strengths: &[engine::TeamStrength],
    world_seed: u64,
    season_index: u32,
) -> SeasonResult {
    let mut tables = Vec::with_capacity(pack.competitions.len());
    let mut movements = Vec::new();
    let mut matches_played = 0u32;
    let mut home_wins = 0u32;
    let mut away_wins = 0u32;
    let mut draws = 0u32;
    let mut total_goals = 0u32;

    for competition in &pack.competitions {
        // Clubes atualmente na competição, em ordem de id denso — nunca via
        // iteração de um `HashMap` (`ADR 0002`: calendário e tabela não
        // podem depender de uma ordem não-determinística).
        let clubs: Vec<ClubId> = membership
            .iter()
            .enumerate()
            .filter(|&(_, &comp)| comp == competition.id)
            .map(|(idx, _)| ClubId::new(idx as u32))
            .collect();

        let Format::RoundRobin { legs, .. } = competition.format;
        let Ok(fixtures) = rules::round_robin(&clubs, legs) else {
            // Número de clubes ímpar ou < 2 na competição — não deveria
            // acontecer com um pack validado (`docs/03 §7`), mas também
            // pode surgir se `promotion.slots`/`relegation.slots` de duas
            // competições emparelhadas não baterem entre si (validação que
            // o `pack` ainda não faz — dívida técnica conhecida, não um
            // `panic!` escondido). Pular a competição nesta temporada é
            // mais seguro que travar o mundo inteiro por causa de uma liga.
            continue;
        };

        let mut results = Vec::with_capacity(fixtures.len());
        for fixture in fixtures {
            let home = crate::strength::strength_of(strengths, fixture.home);
            let away = crate::strength::strength_of(strengths, fixture.away);
            let ctx = engine::MatchContext {
                world_seed,
                fixture: fixture_key(season_index, &fixture),
            };
            let events = engine::simulate(home, away, ctx);
            let (home_goals, away_goals) = engine::score(&events);

            matches_played += 1;
            total_goals += home_goals + away_goals;
            match home_goals.cmp(&away_goals) {
                Ordering::Greater => home_wins += 1,
                Ordering::Less => away_wins += 1,
                Ordering::Equal => draws += 1,
            }
            results.push((
                fixture,
                Score {
                    home_goals,
                    away_goals,
                },
            ));
        }

        let table = rules::compute_table(&clubs, &results, &competition.tiebreakers);

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

    SeasonResult {
        season_index,
        tables,
        movements,
        matches_played,
        home_wins,
        away_wins,
        draws,
        total_goals,
    }
}

/// Simula `seasons` temporadas em sequência, aplicando promoção e
/// rebaixamento entre elas. A força de cada clube é sorteada uma vez
/// (`crate::strength::generate_strengths`) e permanece constante — ver o
/// doc daquele módulo para o porquê.
#[must_use]
pub fn run_seasons(pack: &LoadedPack, world_seed: u64, seasons: u32) -> Vec<SeasonResult> {
    let strengths = crate::strength::generate_strengths(pack, world_seed);
    let mut membership: Vec<CompetitionId> = pack.clubs.iter().map(|c| c.competition).collect();
    let mut reports = Vec::with_capacity(seasons as usize);

    for season_index in 0..seasons {
        let result = run_season(pack, &membership, &strengths, world_seed, season_index);
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
/// total, e nenhuma calibração roda milhões de temporadas).
fn fixture_key(season_index: u32, fixture: &Fixture) -> u64 {
    const BITS: u32 = 20; // 2^20 ≈ 1 milhão — folga generosa sobre docs/03 §9
    debug_assert!(fixture.home.index() < (1 << BITS));
    debug_assert!(fixture.away.index() < (1 << BITS));
    debug_assert!(season_index < (1 << BITS));
    (u64::from(season_index) << (2 * BITS))
        | (u64::from(fixture.home.index()) << BITS)
        | u64::from(fixture.away.index())
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

    #[test]
    fn roda_uma_temporada_no_pack_de_exemplo_sem_travar() {
        let p = example_pack();
        let strengths = crate::strength::generate_strengths(&p, 42);
        let membership = initial_membership(&p);
        let result = run_season(&p, &membership, &strengths, 42, 0);

        assert_eq!(result.tables.len(), 2); // tier1 e tier2
        // 8 clubes, turno+returno = 8*7 = 56 jogos por competição.
        assert_eq!(result.matches_played, 56 * 2);
        assert_eq!(
            result.home_wins + result.away_wins + result.draws,
            result.matches_played
        );
    }

    #[test]
    fn soma_de_pontos_bate_com_i5_no_pack_de_exemplo() {
        let p = example_pack();
        let strengths = crate::strength::generate_strengths(&p, 7);
        let membership = initial_membership(&p);
        let result = run_season(&p, &membership, &strengths, 7, 0);

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
        let tier1_id = p.competitions[0].id;

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
            season in 0u32..(1 << 20),
            home in 0u32..(1 << 20),
            away in 0u32..(1 << 20),
        ) {
            let f = Fixture { round: 0, home: ClubId::new(home), away: ClubId::new(away) };
            let key = fixture_key(season, &f);

            const BITS: u32 = 20;
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
