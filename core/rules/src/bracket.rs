//! Pareamento de mata-mata de fase única (`docs/03-modelo-de-dados.md §7`,
//! `pack::Format::Knockout`).
//!
//! Diferente de [`crate::fixture::round_robin`], o calendário inteiro de um
//! mata-mata **não pode** ser gerado de uma vez: quem joga a 2ª rodada
//! depende de quem venceu a 1ª. Por isso esta função só resolve **um**
//! pareamento por chamada — `world::cup` chama de novo a cada rodada,
//! passando os vencedores da anterior como `contenders` da próxima. Isso
//! mantém este módulo tão "puro" quanto `fixture`: nenhuma noção de partida
//! jogada, nenhum RNG, só "dada esta lista ordenada, quem joga contra quem".
//!
//! O pareamento em si é a posição no vetor de entrada: `contenders[0]` joga
//! contra `contenders[1]`, `contenders[2]` contra `contenders[3]`, e assim
//! por diante — sem sorteio de chave (simplificação deliberada: a "ordem do
//! bracket" vem de `ResolvedClub::id`, dense e determinístico, nunca de
//! `DeterministicRng`). Documentado aqui, não escondido: um mata-mata de
//! verdade tem cerimônia de sorteio; este não.

use std::fmt;

use domain::ClubId;

/// Erro ao parear uma rodada de mata-mata — hoje só um jeito de dar errado,
/// espelhando [`crate::fixture::FixtureError`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BracketError {
    /// Mata-mata sem bye (`docs/03 §7`) exige potência de 2, `>= 2`,
    /// participantes em toda rodada — inclusive nas rodadas depois da
    /// primeira, onde `contenders` já é a lista de vencedores.
    NotPowerOfTwo { count: usize },
}

impl fmt::Display for BracketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            BracketError::NotPowerOfTwo { count } => {
                write!(
                    f,
                    "número de participantes deve ser potência de 2, >= 2 (veio {count})"
                )
            }
        }
    }
}

impl std::error::Error for BracketError {}

/// Pareia uma rodada: `(contenders[0], contenders[1])`,
/// `(contenders[2], contenders[3])`, ... — a ordem de `contenders` **é** o
/// chaveamento (ver doc do módulo). O primeiro de cada par manda a partida
/// (`world::cup` decide se isso muda a cada rodada ou não).
pub fn pair_round(contenders: &[ClubId]) -> Result<Vec<(ClubId, ClubId)>, BracketError> {
    let n = contenders.len();
    if n < 2 || !n.is_power_of_two() {
        return Err(BracketError::NotPowerOfTwo { count: n });
    }
    Ok(contenders
        .chunks_exact(2)
        .map(|pair| (pair[0], pair[1]))
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn clubs(n: u32) -> Vec<ClubId> {
        (0..n).map(ClubId::new).collect()
    }

    #[test]
    fn rejeita_numero_que_nao_e_potencia_de_dois() {
        assert_eq!(
            pair_round(&clubs(6)),
            Err(BracketError::NotPowerOfTwo { count: 6 })
        );
    }

    #[test]
    fn rejeita_menos_de_dois_participantes() {
        assert_eq!(
            pair_round(&clubs(1)),
            Err(BracketError::NotPowerOfTwo { count: 1 })
        );
        assert_eq!(
            pair_round(&[]),
            Err(BracketError::NotPowerOfTwo { count: 0 })
        );
    }

    #[test]
    fn pareia_consecutivos_na_ordem_de_entrada() {
        let pairs = pair_round(&clubs(8)).unwrap();
        assert_eq!(
            pairs,
            vec![
                (ClubId::new(0), ClubId::new(1)),
                (ClubId::new(2), ClubId::new(3)),
                (ClubId::new(4), ClubId::new(5)),
                (ClubId::new(6), ClubId::new(7)),
            ]
        );
    }

    #[test]
    fn aceita_potencias_de_dois_maiores() {
        assert!(pair_round(&clubs(2)).is_ok());
        assert!(pair_round(&clubs(4)).is_ok());
        assert!(pair_round(&clubs(16)).is_ok());
    }

    #[test]
    fn todo_participante_aparece_em_exatamente_um_par() {
        let cs = clubs(16);
        let pairs = pair_round(&cs).unwrap();
        assert_eq!(pairs.len(), 8);
        let mut seen = std::collections::HashSet::new();
        for (a, b) in pairs {
            assert!(seen.insert(a));
            assert!(seen.insert(b));
        }
        assert_eq!(seen.len(), 16);
    }
}

/// Testes de propriedade (`docs/08 §1`).
#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;
    use std::collections::HashSet;

    proptest! {
        #[test]
        fn pair_round_respeita_os_invariantes_para_qualquer_potencia_de_dois(
            power in 1u32..8, // n = 2^power, 2..=128
        ) {
            let n = 1u32 << power;
            let cs: Vec<ClubId> = (0..n).map(ClubId::new).collect();
            let pairs = pair_round(&cs).unwrap();

            prop_assert_eq!(pairs.len(), (n / 2) as usize);

            let mut seen = HashSet::new();
            for (a, b) in pairs {
                prop_assert_ne!(a, b, "clube pareado contra si mesmo");
                prop_assert!(seen.insert(a));
                prop_assert!(seen.insert(b));
            }
            prop_assert_eq!(seen.len(), n as usize);
        }

        #[test]
        fn pair_round_e_deterministico(power in 1u32..8) {
            let n = 1u32 << power;
            let cs: Vec<ClubId> = (0..n).map(ClubId::new).collect();
            prop_assert_eq!(pair_round(&cs), pair_round(&cs));
        }

        #[test]
        fn pair_round_rejeita_qualquer_n_que_nao_seja_potencia_de_dois(
            n in 1usize..200,
        ) {
            prop_assume!(!n.is_power_of_two() || n < 2);
            let cs: Vec<ClubId> = (0..n as u32).map(ClubId::new).collect();
            prop_assert!(pair_round(&cs).is_err());
        }
    }
}
