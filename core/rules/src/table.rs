//! Tabela de classificação — aplica os `tiebreakers` de um
//! `pack::ResolvedCompetition` (`docs/03 §7`) a um conjunto de resultados.

use std::cmp::Ordering;
use std::collections::HashMap;

use domain::ClubId;
use pack::Tiebreaker;

use crate::fixture::Fixture;

/// Placar final de uma partida já disputada.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Score {
    pub home_goals: u32,
    pub away_goals: u32,
}

/// Uma linha da tabela — as contagens brutas, antes de qualquer ordenação.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TableRow {
    pub club: ClubId,
    pub played: u32,
    pub wins: u32,
    pub draws: u32,
    pub losses: u32,
    pub goals_for: u32,
    pub goals_against: u32,
}

impl TableRow {
    fn empty(club: ClubId) -> Self {
        Self {
            club,
            played: 0,
            wins: 0,
            draws: 0,
            losses: 0,
            goals_for: 0,
            goals_against: 0,
        }
    }

    /// 3 pontos por vitória, 1 por empate — a única convenção de pontuação
    /// suportada por ora (`docs/03 §7` não expõe pontuação customizada
    /// ainda; quando expuser, é aqui que essa constante vira parâmetro).
    #[must_use]
    pub fn points(&self) -> u32 {
        self.wins * 3 + self.draws
    }

    #[must_use]
    pub fn goal_difference(&self) -> i64 {
        i64::from(self.goals_for) - i64::from(self.goals_against)
    }
}

/// Calcula a tabela final a partir do calendário e dos placares, ordenada
/// pelos critérios de desempate na ordem em que aparecem em `tiebreakers`.
///
/// `HeadToHead` é resolvido **apenas par a par** (pontos ganhos de um clube
/// contra o outro especificamente) — suficiente para o caso comum de dois
/// clubes empatados, mas não implementa a mini-liga completa que a regra
/// oficial pede para três ou mais clubes empatados entre si (caso raro, e
/// que pode inclusive ser não-transitivo: A > B > C > A já aconteceu em
/// competições de verdade). Documentado como simplificação conhecida do v0,
/// não um bug escondido.
///
/// O desempate final, **sempre**, é a ordem do `ClubId` — nunca existe
/// "empate de verdade" na tabela devolvida, porque uma ordem indefinida
/// quebraria o determinismo entre plataformas (`ADR 0002`).
#[must_use]
pub fn compute_table(
    clubs: &[ClubId],
    results: &[(Fixture, Score)],
    tiebreakers: &[Tiebreaker],
) -> Vec<TableRow> {
    let mut rows: HashMap<ClubId, TableRow> =
        clubs.iter().map(|&c| (c, TableRow::empty(c))).collect();
    let mut head_to_head_points: HashMap<(ClubId, ClubId), u32> = HashMap::new();

    for (fixture, score) in results {
        apply_result(
            &mut rows,
            fixture.home,
            fixture.away,
            score.home_goals,
            score.away_goals,
        );
        record_head_to_head(&mut head_to_head_points, fixture, score);
    }

    let mut table: Vec<TableRow> = rows.into_values().collect();
    table.sort_by(|a, b| compare_rows(a, b, tiebreakers, &head_to_head_points));
    table
}

fn apply_result(
    rows: &mut HashMap<ClubId, TableRow>,
    home: ClubId,
    away: ClubId,
    home_goals: u32,
    away_goals: u32,
) {
    let home_row = rows
        .get_mut(&home)
        .expect("clube do fixture não está na lista de clubes");
    home_row.played += 1;
    home_row.goals_for += home_goals;
    home_row.goals_against += away_goals;
    match home_goals.cmp(&away_goals) {
        Ordering::Greater => home_row.wins += 1,
        Ordering::Equal => home_row.draws += 1,
        Ordering::Less => home_row.losses += 1,
    }

    let away_row = rows
        .get_mut(&away)
        .expect("clube do fixture não está na lista de clubes");
    away_row.played += 1;
    away_row.goals_for += away_goals;
    away_row.goals_against += home_goals;
    match away_goals.cmp(&home_goals) {
        Ordering::Greater => away_row.wins += 1,
        Ordering::Equal => away_row.draws += 1,
        Ordering::Less => away_row.losses += 1,
    }
}

fn record_head_to_head(map: &mut HashMap<(ClubId, ClubId), u32>, fixture: &Fixture, score: &Score) {
    let (home_points, away_points) = match score.home_goals.cmp(&score.away_goals) {
        Ordering::Greater => (3, 0),
        Ordering::Equal => (1, 1),
        Ordering::Less => (0, 3),
    };
    *map.entry((fixture.home, fixture.away)).or_insert(0) += home_points;
    *map.entry((fixture.away, fixture.home)).or_insert(0) += away_points;
}

fn compare_rows(
    a: &TableRow,
    b: &TableRow,
    tiebreakers: &[Tiebreaker],
    head_to_head: &HashMap<(ClubId, ClubId), u32>,
) -> Ordering {
    for &criterion in tiebreakers {
        let ordering = match criterion {
            Tiebreaker::Points => b.points().cmp(&a.points()),
            Tiebreaker::Wins => b.wins.cmp(&a.wins),
            Tiebreaker::Draws => b.draws.cmp(&a.draws),
            Tiebreaker::GoalDifference => b.goal_difference().cmp(&a.goal_difference()),
            Tiebreaker::GoalsFor => b.goals_for.cmp(&a.goals_for),
            // Gols sofridos: MENOS é melhor — é o único critério "invertido".
            Tiebreaker::GoalsAgainst => a.goals_against.cmp(&b.goals_against),
            Tiebreaker::HeadToHead => {
                let for_a = head_to_head.get(&(a.club, b.club)).copied().unwrap_or(0);
                let for_b = head_to_head.get(&(b.club, a.club)).copied().unwrap_or(0);
                for_b.cmp(&for_a)
            }
        };
        if ordering != Ordering::Equal {
            return ordering;
        }
    }
    // Nunca deixa a tabela com ordem indefinida entre dois clubes — ver
    // doc da função.
    a.club.cmp(&b.club)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn c(i: u32) -> ClubId {
        ClubId::new(i)
    }

    fn fixture(home: ClubId, away: ClubId) -> Fixture {
        Fixture {
            round: 0,
            home,
            away,
        }
    }

    fn score(h: u32, a: u32) -> Score {
        Score {
            home_goals: h,
            away_goals: a,
        }
    }

    #[test]
    fn soma_de_pontos_bate_com_3_vitorias_mais_empates() {
        // Invariante I5 de docs/08-qualidade-e-testes.md §2, aplicado aqui.
        let clubs = [c(0), c(1), c(2), c(3)];
        let results = [
            (fixture(c(0), c(1)), score(2, 1)),
            (fixture(c(2), c(3)), score(0, 0)),
            (fixture(c(0), c(2)), score(1, 1)),
            (fixture(c(1), c(3)), score(3, 0)),
        ];
        let table = compute_table(&clubs, &results, &[Tiebreaker::Points]);

        let total_wins: u32 = table.iter().map(|r| r.wins).sum();
        let total_draws: u32 = table.iter().map(|r| r.draws).sum();
        let total_points: u32 = table.iter().map(TableRow::points).sum();
        assert_eq!(total_points, total_wins * 3 + total_draws);
    }

    #[test]
    fn ordena_por_pontos_desc() {
        let clubs = [c(0), c(1), c(2)];
        let results = [
            (fixture(c(0), c(1)), score(3, 0)), // 0: 3 pts
            (fixture(c(1), c(2)), score(1, 1)), // 1: 1 pt, 2: 1 pt
        ];
        let table = compute_table(&clubs, &results, &[Tiebreaker::Points]);
        assert_eq!(table[0].club, c(0));
    }

    #[test]
    fn desempate_por_saldo_de_gols() {
        let clubs = [c(0), c(1), c(2), c(3)];
        // 0 e 1 ficam com os mesmos pontos (3), mas saldos diferentes.
        let results = [
            (fixture(c(0), c(2)), score(5, 0)), // saldo +5 pra 0
            (fixture(c(1), c(3)), score(1, 0)), // saldo +1 pra 1
        ];
        let table = compute_table(
            &clubs,
            &results,
            &[Tiebreaker::Points, Tiebreaker::GoalDifference],
        );
        let pos_0 = table.iter().position(|r| r.club == c(0)).unwrap();
        let pos_1 = table.iter().position(|r| r.club == c(1)).unwrap();
        assert!(pos_0 < pos_1, "clube com saldo maior deveria vir antes");
    }

    #[test]
    fn menos_gols_sofridos_e_melhor_no_criterio_goals_against() {
        let clubs = [c(0), c(1), c(2), c(3)];
        // Ambos empatam em pontos (0) e em gols marcados (0), mas 0 sofreu
        // menos que 1.
        let results = [
            (fixture(c(2), c(0)), score(1, 0)), // 0 perde, sofre 1
            (fixture(c(3), c(1)), score(3, 0)), // 1 perde, sofre 3
        ];
        let table = compute_table(
            &clubs,
            &results,
            &[
                Tiebreaker::Points,
                Tiebreaker::GoalsFor,
                Tiebreaker::GoalsAgainst,
            ],
        );
        let pos_0 = table.iter().position(|r| r.club == c(0)).unwrap();
        let pos_1 = table.iter().position(|r| r.club == c(1)).unwrap();
        assert!(pos_0 < pos_1, "quem sofreu menos gol deveria vir antes");
    }

    #[test]
    fn head_to_head_desempata_par_a_par() {
        let clubs = [c(0), c(1), c(2), c(3)];
        // 0 e 1 empatam em pontos (3) e em saldo (+1), mas 0 venceu o
        // confronto direto contra 1. club0: GF2 GA1 (2-0 e 0-1) = saldo +1.
        // club1: GF3 GA2 (0-2 e 3-0) = saldo +1. Mesmos pontos (3, de 1
        // vitória e 1 derrota cada), saldo idêntico — só o desempate direto
        // separa os dois. O 4º jogo existe só para afastar o saldo de 3 de
        // +1 (sem ele, 3 empataria com 0 e 1 depois de só 1 jogo, criando
        // um ciclo de 3 vias de desempate direto — não-transitivo por
        // natureza, exatamente a limitação documentada em `compute_table` —
        // que tornaria a posição relativa de 0 e 1 dependente da ordem
        // interna do sort, não do resultado do confronto direto entre eles).
        let results = [
            (fixture(c(0), c(1)), score(2, 0)), // 0 vence 1 no confronto direto
            (fixture(c(1), c(2)), score(3, 0)), // 1 vence 2
            (fixture(c(0), c(3)), score(0, 1)), // 0 perde por 1 — iguala o saldo com 1
            (fixture(c(3), c(2)), score(0, 5)), // afasta o saldo de 3 (agora -4, não +1)
        ];
        let table = compute_table(
            &clubs,
            &results,
            &[
                Tiebreaker::Points,
                Tiebreaker::GoalDifference,
                Tiebreaker::HeadToHead,
            ],
        );
        let pos_0 = table.iter().position(|r| r.club == c(0)).unwrap();
        let pos_1 = table.iter().position(|r| r.club == c(1)).unwrap();
        assert!(
            pos_0 < pos_1,
            "vencedor do confronto direto deveria vir antes"
        );
    }

    #[test]
    fn desempate_final_e_sempre_por_club_id_nunca_indefinido() {
        // Dois clubes idênticos em tudo (nenhum jogo disputado) — sem um
        // desempate final determinístico, a ordem entre eles dependeria da
        // implementação do sort, o que violaria o ADR 0002.
        let clubs = [c(5), c(2)];
        let table = compute_table(&clubs, &[], &[Tiebreaker::Points]);
        assert_eq!(table[0].club, c(2));
        assert_eq!(table[1].club, c(5));
    }
}

/// Testes de propriedade (`docs/08 §1`) — o invariante I5 de
/// `docs/08-qualidade-e-testes.md §2` ("soma de pontos = 3×vitórias +
/// empates, para toda competição, toda rodada"), verificado para placares
/// arbitrários em vez de só os exemplos escolhidos à mão acima.
#[cfg(test)]
mod proptests {
    use super::*;
    use crate::fixture::round_robin;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn soma_de_pontos_bate_com_i5_para_placares_arbitrarios(
            scores in proptest::collection::vec((0u32..6, 0u32..6), 28),
        ) {
            // 8 clubes, turno único = 28 jogos (C(8,2)) — bate exatamente
            // com o tamanho do vetor de placares gerado.
            let clubs: Vec<ClubId> = (0..8).map(ClubId::new).collect();
            let fixtures = round_robin(&clubs, 1).unwrap();
            let results: Vec<(Fixture, Score)> = fixtures
                .into_iter()
                .zip(scores)
                .map(|(f, (h, a))| (f, Score { home_goals: h, away_goals: a }))
                .collect();

            let table = compute_table(&clubs, &results, &[Tiebreaker::Points]);

            let total_wins: u32 = table.iter().map(|r| r.wins).sum();
            let total_draws: u32 = table.iter().map(|r| r.draws).sum();
            let total_points: u32 = table.iter().map(TableRow::points).sum();
            prop_assert_eq!(total_points, total_wins * 3 + total_draws);

            // Cada clube: jogos = vitórias + empates + derrotas, sempre.
            for row in &table {
                prop_assert_eq!(row.played, row.wins + row.draws + row.losses);
            }

            // 28 jogos, cada um soma exatamente 2 "played" (mandante +
            // visitante) — então a soma de `played` na tabela é sempre 56.
            let total_played: u32 = table.iter().map(|r| r.played).sum();
            prop_assert_eq!(total_played, 28 * 2);
        }

        /// A ordem da tabela nunca deixa dois clubes "empatados de vez" —
        /// para qualquer conjunto de placares, `compute_table` produz uma
        /// permutação total e determinística dos clubes (generaliza
        /// `desempate_final_e_sempre_por_club_id_nunca_indefinido`).
        #[test]
        fn tabela_e_sempre_uma_permutacao_completa_e_deterministica(
            scores in proptest::collection::vec((0u32..4, 0u32..4), 28),
        ) {
            let clubs: Vec<ClubId> = (0..8).map(ClubId::new).collect();
            let fixtures = round_robin(&clubs, 1).unwrap();
            let results: Vec<(Fixture, Score)> = fixtures
                .into_iter()
                .zip(scores)
                .map(|(f, (h, a))| (f, Score { home_goals: h, away_goals: a }))
                .collect();

            let tiebreakers = [Tiebreaker::Points, Tiebreaker::GoalDifference, Tiebreaker::GoalsFor];
            let a = compute_table(&clubs, &results, &tiebreakers);
            let b = compute_table(&clubs, &results, &tiebreakers);
            prop_assert_eq!(&a, &b, "mesma entrada produziu tabelas diferentes");

            let mut club_ids: Vec<ClubId> = a.iter().map(|r| r.club).collect();
            club_ids.sort();
            let mut expected = clubs.clone();
            expected.sort();
            prop_assert_eq!(club_ids, expected, "tabela não contém exatamente os clubes de entrada");
        }
    }
}
