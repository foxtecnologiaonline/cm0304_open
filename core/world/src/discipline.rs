//! Suspensões por expulsão — fatia mínima de RF-JG-09 (`docs/01-requisitos.md
//! §2.3`: "Cartões, suspensões automáticas por competição").
//!
//! Ao contrário de condição (`crate::condition`) e lesão
//! (`crate::injuries`), que cabem numa granularidade de temporada sem
//! distorcer o que representam, suspensão só faz sentido por partida: é
//! por isso que `crate::season::run_season` agora simula round-robin
//! rodada a rodada para o caminho de carreira (perfil recalculado a cada
//! rodada, não mais uma vez por temporada) — ver o doc de lá. Sem essa
//! granularidade, "suspender por uma rodada" não teria como ter efeito
//! algum na simulação.
//!
//! Depois de cada rodada, cada time que jogou sofre um sorteio de
//! expulsão (determinístico, ~4% por time por partida — ordem de grandeza
//! real de cartão vermelho numa liga de primeira divisão); se acontece, um
//! dos titulares daquele time **naquela rodada** (sorteado entre eles,
//! sem favorecer nenhuma posição) fica suspenso só na rodada seguinte da
//! mesma competição, na mesma temporada — a suspensão nunca atravessa
//! temporadas (mesma disciplina de `crate::condition`/`crate::injuries`:
//! sem calendário diário, não há como saber quantas partidas reais de
//! intervalo existem entre o fim de uma temporada e o início da próxima).
//! Só partidas de **liga** geram ou cumprem suspensão — copa é isolada
//! (`crate::cup`), mesma exclusão de `crate::condition`.
//!
//! Sem prorrogação de pena por reincidência, sem cartão amarelo acumulado,
//! sem diferença de rigor por árbitro (RF-MU-11) — um sorteio só, uma
//! consequência só. O resto de RF-JG-09 fica para quando houver jogador em
//! campo de verdade no motor (v1/v2).

use std::collections::HashSet;

use domain::{ClubId, DeterministicRng, PlayerId};

/// Chance de uma expulsão por time por partida, em permilagem — ~4%.
const EXPULSION_CHANCE_PERMILLE: u32 = 40;

/// Combina temporada e rodada num único `u64` — o `tick` de
/// `DeterministicRng::seeded` para o sorteio de disciplina (`domain`
/// "world.discipline", `entity` = `club.index()`). Não precisa ser
/// reversível nem evitar colisão com outro domínio (a string do domínio já
/// separa os espaços de RNG, `docs/02 §5`) — só precisa ser estável e
/// distinto por `(temporada, rodada)`. `pub(crate)` porque
/// `crate::finance::match_day_revenue` reaproveita a mesma combinação para
/// o sorteio de público — não é um dado sensível a colisão como
/// `crate::fixture_key` (cada domínio de RNG já é uma string distinta,
/// `docs/02 §5`), só uma conveniência para não repetir o mesmo `<<`/`|`
/// em dois lugares.
pub(crate) fn season_round_tick(season_index: u32, round: u32) -> u64 {
    (u64::from(season_index) << 32) | u64::from(round)
}

/// Depois de simular uma rodada, sorteia quem fica suspenso na rodada
/// seguinte — um `PlayerId` por time em que a expulsão aconteceu (nunca
/// mais de um por time nesta fatia mínima). `starters_by_club` é a lista de
/// titulares de cada time que jogou a rodada (vazia = time sem titular
/// elegível, ex.: todos lesionados/suspensos — não sorteável, pulado).
///
/// Determinístico por `(world_seed, club, season_index, round)`: a ordem
/// em que os times aparecem em `starters_by_club` não influencia o
/// resultado de nenhum outro time (`docs/02 §5`, um `Rng` por
/// entidade-raiz).
#[must_use]
pub fn roll_suspensions(
    world_seed: u64,
    season_index: u32,
    round: u32,
    starters_by_club: &[(ClubId, Vec<PlayerId>)],
) -> HashSet<PlayerId> {
    let mut suspended = HashSet::new();
    for (club, starters) in starters_by_club {
        if starters.is_empty() {
            continue;
        }
        let mut rng = DeterministicRng::seeded(
            world_seed,
            "world.discipline",
            u64::from(club.index()),
            season_round_tick(season_index, round),
        );
        if rng.chance_per_mille(EXPULSION_CHANCE_PERMILLE) {
            let idx = rng.pick_index(starters.len());
            suspended.insert(starters[idx]);
        }
    }
    suspended
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn time_sem_titular_nunca_e_sorteado() {
        let starters = vec![(ClubId::new(0), vec![])];
        let suspended = roll_suspensions(1, 0, 0, &starters);
        assert!(suspended.is_empty());
    }

    #[test]
    fn suspenso_e_sempre_um_dos_titulares_daquele_time() {
        let starters = vec![(
            ClubId::new(0),
            vec![PlayerId::new(10), PlayerId::new(11), PlayerId::new(12)],
        )];
        // Várias seeds — ao menos uma deveria gerar suspensão (4% por
        // chamada, 200 tentativas torna "nenhuma suspensão" improvável a
        // ponto de indicar bug se acontecer).
        let mut saw_suspension = false;
        for seed in 0..200u64 {
            let suspended = roll_suspensions(seed, 0, 0, &starters);
            if let Some(&player) = suspended.iter().next() {
                saw_suspension = true;
                assert!(
                    [PlayerId::new(10), PlayerId::new(11), PlayerId::new(12)].contains(&player),
                    "suspenso {player:?} não é titular daquele time"
                );
                assert_eq!(suspended.len(), 1);
            }
        }
        assert!(
            saw_suspension,
            "esperava pelo menos uma suspensão em 200 seeds a ~4%"
        );
    }

    #[test]
    fn nunca_suspende_mais_de_um_por_time_na_mesma_rodada() {
        let starters = vec![(ClubId::new(0), vec![PlayerId::new(1), PlayerId::new(2)])];
        for seed in 0..200u64 {
            let suspended = roll_suspensions(seed, 0, 0, &starters);
            assert!(suspended.len() <= 1);
        }
    }

    #[test]
    fn times_diferentes_sorteiam_de_forma_independente() {
        // Clube 0 expulsa, clube 1 não deveria ser afetado por isso — cada
        // um tem sua própria seed derivada (`entity = club.index()`).
        let starters = vec![
            (ClubId::new(0), vec![PlayerId::new(1)]),
            (ClubId::new(1), vec![PlayerId::new(2)]),
        ];
        let a = roll_suspensions(3, 0, 0, &starters[..1]);
        let b = roll_suspensions(3, 0, 0, &starters);
        // O resultado do clube 0 isolado é igual ao resultado do clube 0
        // dentro da lista com o clube 1 — a presença do clube 1 não altera
        // o sorteio do clube 0.
        assert_eq!(a.contains(&PlayerId::new(1)), b.contains(&PlayerId::new(1)));
    }

    #[test]
    fn e_deterministico_para_a_mesma_entrada() {
        let starters = vec![(ClubId::new(0), vec![PlayerId::new(1), PlayerId::new(2)])];
        assert_eq!(
            roll_suspensions(42, 3, 5, &starters),
            roll_suspensions(42, 3, 5, &starters)
        );
    }

    #[test]
    fn rodada_diferente_pode_sortear_diferente() {
        let starters = vec![(ClubId::new(0), vec![PlayerId::new(1), PlayerId::new(2)])];
        let results: Vec<HashSet<PlayerId>> = (0..50)
            .map(|round| roll_suspensions(99, 0, round, &starters))
            .collect();
        assert!(
            results.windows(2).any(|w| w[0] != w[1]),
            "50 rodadas produziram sempre o mesmo resultado — RNG não está variando com a rodada"
        );
    }
}

/// Testes de propriedade (`docs/08 §1`).
#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn roll_suspensions_e_deterministico(
            world_seed: u64,
            season_index: u32,
            round: u32,
            club in 0u32..8,
            player in 0u32..20,
        ) {
            let starters = vec![(ClubId::new(club), vec![PlayerId::new(player)])];
            prop_assert_eq!(
                roll_suspensions(world_seed, season_index, round, &starters),
                roll_suspensions(world_seed, season_index, round, &starters)
            );
        }

        #[test]
        fn roll_suspensions_nunca_suspende_quem_nao_titularizou(
            world_seed: u64,
            season_index: u32,
            round: u32,
        ) {
            let starters = vec![(ClubId::new(0), vec![PlayerId::new(1), PlayerId::new(2)])];
            let suspended = roll_suspensions(world_seed, season_index, round, &starters);
            for player in &suspended {
                prop_assert!(*player == PlayerId::new(1) || *player == PlayerId::new(2));
            }
        }
    }
}
