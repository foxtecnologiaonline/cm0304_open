//! Geração de calendário — round-robin (`docs/03-modelo-de-dados.md §7`,
//! `docs/01-requisitos.md` RF-MU-02).
//!
//! Usa o "método do círculo" (*circle method*), o algoritmo clássico e
//! determinístico de agendamento de todos-contra-todos: fixa um clube,
//! roda os demais a cada rodada. Produz exatamente `n-1` rodadas para `n`
//! clubes (par), cada uma com `n/2` jogos, cobrindo todo par exatamente uma
//! vez por turno — sem sorteio, sem aleatoriedade, e portanto sem depender
//! de `DeterministicRng` (o calendário é uma função pura do conjunto de
//! clubes, não do RNG do mundo).

use std::fmt;

use domain::ClubId;

/// Uma partida agendada: quem manda, quem visita, em que rodada.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Fixture {
    pub round: u32,
    pub home: ClubId,
    pub away: ClubId,
}

/// Erro ao gerar o calendário — hoje só um jeito de dar errado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FixtureError {
    /// O método do círculo exige número par de clubes ≥ 2. Um número ímpar
    /// pediria uma rodada de "folga" por clube — não implementado (nenhuma
    /// competição do MVP usa número ímpar de times, `docs/00 §7`).
    OddOrTooFewClubs { count: usize },
}

impl fmt::Display for FixtureError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FixtureError::OddOrTooFewClubs { count } => {
                write!(f, "número de clubes deve ser par e >= 2 (veio {count})")
            }
        }
    }
}

impl std::error::Error for FixtureError {}

/// Gera o calendário de um turno (cada clube joga contra cada outro
/// exatamente uma vez) para `clubs`, alternando mando de forma equilibrada
/// entre as rodadas.
pub fn single_round_robin(clubs: &[ClubId]) -> Result<Vec<Fixture>, FixtureError> {
    let n = clubs.len();
    if n < 2 || !n.is_multiple_of(2) {
        return Err(FixtureError::OddOrTooFewClubs { count: n });
    }

    let mut arrangement = clubs.to_vec();
    let mut fixtures = Vec::with_capacity(n * (n - 1) / 2);

    for round in 0..(n - 1) as u32 {
        for i in 0..n / 2 {
            let (left, right) = (arrangement[i], arrangement[n - 1 - i]);
            // Alterna quem manda a cada rodada — sem isso, a posição 0 do
            // array mandaria em toda rodada em que aparece, o que dá mando
            // de campo desigual entre os clubes ao longo do turno.
            let (home, away) = if round % 2 == 0 {
                (left, right)
            } else {
                (right, left)
            };
            fixtures.push(Fixture { round, home, away });
        }
        // Método do círculo: mantém `arrangement[0]` fixo, roda o resto.
        let last = arrangement
            .pop()
            .expect("n >= 2, então arrangement nunca fica vazio aqui");
        arrangement.insert(1, last);
    }

    Ok(fixtures)
}

/// Gera o returno inteiro (`legs` turnos — `docs/03 §7`, `format.legs`),
/// invertendo mando a cada turno adicional. `legs == 1` é só
/// [`single_round_robin`]; `legs == 2` é o dobro de turno e returno.
pub fn round_robin(clubs: &[ClubId], legs: u8) -> Result<Vec<Fixture>, FixtureError> {
    let first_leg = single_round_robin(clubs)?;
    let rounds_per_leg = first_leg.len() / (clubs.len() / 2);

    let mut all = Vec::with_capacity(first_leg.len() * usize::from(legs.max(1)));
    for leg in 0..legs.max(1) {
        let round_offset = u32::from(leg) * rounds_per_leg as u32;
        all.extend(first_leg.iter().map(|f| {
            if leg % 2 == 0 {
                Fixture {
                    round: f.round + round_offset,
                    home: f.home,
                    away: f.away,
                }
            } else {
                // turnos ímpares invertem mando — o "returno" clássico.
                Fixture {
                    round: f.round + round_offset,
                    home: f.away,
                    away: f.home,
                }
            }
        }));
    }
    Ok(all)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn clubs(n: u32) -> Vec<ClubId> {
        (0..n).map(ClubId::new).collect()
    }

    #[test]
    fn rejeita_numero_impar_de_clubes() {
        assert_eq!(
            single_round_robin(&clubs(3)),
            Err(FixtureError::OddOrTooFewClubs { count: 3 })
        );
    }

    #[test]
    fn rejeita_menos_de_dois_clubes() {
        assert!(single_round_robin(&[]).is_err());
    }

    #[test]
    fn turno_unico_cobre_cada_par_exatamente_uma_vez() {
        let cs = clubs(8);
        let fixtures = single_round_robin(&cs).unwrap();

        assert_eq!(fixtures.len(), 8 * 7 / 2); // C(8,2) = 28
        assert_eq!(fixtures.iter().map(|f| f.round).max(), Some(6)); // n-1 = 7 rodadas, 0..=6

        // Cada rodada tem exatamente n/2 jogos.
        for round in 0..7 {
            assert_eq!(fixtures.iter().filter(|f| f.round == round).count(), 4);
        }

        // Todo par não-ordenado de clubes aparece exatamente uma vez.
        let mut pairs = HashSet::new();
        for f in &fixtures {
            let key = (f.home.min(f.away), f.home.max(f.away));
            assert!(pairs.insert(key), "par {key:?} apareceu mais de uma vez");
        }
        assert_eq!(pairs.len(), 28);
    }

    #[test]
    fn nenhum_clube_joga_duas_vezes_na_mesma_rodada() {
        let cs = clubs(8);
        let fixtures = single_round_robin(&cs).unwrap();
        for round in 0..7 {
            let mut seen = HashSet::new();
            for f in fixtures.iter().filter(|f| f.round == round) {
                assert!(
                    seen.insert(f.home),
                    "clube {:?} joga 2x na rodada {round}",
                    f.home
                );
                assert!(
                    seen.insert(f.away),
                    "clube {:?} joga 2x na rodada {round}",
                    f.away
                );
            }
        }
    }

    #[test]
    fn turno_e_returno_dobram_os_jogos_e_invertem_mando() {
        let cs = clubs(8);
        let fixtures = round_robin(&cs, 2).unwrap();
        assert_eq!(fixtures.len(), 8 * 7); // cada par joga 2x (ida e volta)

        // Cada par ordenado (home, away) aparece exatamente uma vez — ou
        // seja, A manda contra B exatamente uma vez, e B manda contra A
        // exatamente uma vez, nunca A manda duas vezes contra B.
        let mut ordered_pairs = HashSet::new();
        for f in &fixtures {
            assert!(
                ordered_pairs.insert((f.home, f.away)),
                "confronto ordenado ({:?}, {:?}) repetido",
                f.home,
                f.away
            );
        }
        assert_eq!(ordered_pairs.len(), 56);
    }

    #[test]
    fn cada_clube_joga_o_numero_certo_de_partidas() {
        let cs = clubs(8);
        let fixtures = round_robin(&cs, 2).unwrap();
        for &club in &cs {
            let played = fixtures
                .iter()
                .filter(|f| f.home == club || f.away == club)
                .count();
            assert_eq!(played, 14); // (n-1) * legs = 7 * 2
        }
    }
}

/// Testes de propriedade (`docs/08 §1`) — generalizam os exemplos fixos
/// acima (sempre com 8 clubes) para qualquer quantidade par entre 2 e 24 e
/// qualquer número de turnos, gerando o calendário e conferindo os mesmos
/// invariantes: contagem total, ninguém joga duas vezes na mesma rodada,
/// todo clube joga o número certo de partidas.
#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;
    use std::collections::HashSet;

    proptest! {
        #[test]
        fn round_robin_respeita_os_invariantes_para_qualquer_n_par(
            half_n in 1u32..12, // n = 2*half_n, sempre par, 2..=22
            legs in 1u8..=2,
        ) {
            let n = half_n * 2;
            let cs: Vec<domain::ClubId> = (0..n).map(domain::ClubId::new).collect();
            let fixtures = round_robin(&cs, legs).unwrap();

            let expected_total = (n as usize) * (n as usize - 1) / 2 * usize::from(legs);
            prop_assert_eq!(fixtures.len(), expected_total);

            let rounds_per_leg = n as usize - 1;
            for round in 0..(rounds_per_leg * usize::from(legs)) as u32 {
                let mut seen = HashSet::new();
                for f in fixtures.iter().filter(|f| f.round == round) {
                    prop_assert_ne!(f.home, f.away, "clube contra si mesmo na rodada {}", round);
                    prop_assert!(seen.insert(f.home), "clube {:?} joga 2x na rodada {}", f.home, round);
                    prop_assert!(seen.insert(f.away), "clube {:?} joga 2x na rodada {}", f.away, round);
                }
            }

            for &club in &cs {
                let played = fixtures.iter().filter(|f| f.home == club || f.away == club).count();
                prop_assert_eq!(played, rounds_per_leg * usize::from(legs));
            }
        }

        #[test]
        fn round_robin_e_deterministico(half_n in 1u32..12, legs in 1u8..=2) {
            let n = half_n * 2;
            let cs: Vec<domain::ClubId> = (0..n).map(domain::ClubId::new).collect();
            let a = round_robin(&cs, legs).unwrap();
            let b = round_robin(&cs, legs).unwrap();
            prop_assert_eq!(a, b);
        }
    }
}
