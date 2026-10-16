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
//!
//! Quem é expulso, dentro dos titulares daquela rodada, não é mais
//! sorteado uniformemente: [`generate_dirtiness`] sorteia uma "sujeira"
//! sintética (0-100) por jogador, uma vez por carreira — fatia de
//! RF-JG-02 (atributos ocultos, `docs/01 §2.3`) — e [`roll_suspensions`]
//! pesa o sorteio a favor de quem é mais sujo (peso `1 + sujeira/10`:
//! de 1x a 11x mais chance que o jogador mais limpo possível). A chance
//! de o **time** ser expulso em cada partida continua fixa em ~4%; só
//! "quem, dentro do time" muda.

use std::collections::HashSet;

use domain::{ClubId, DeterministicRng, PlayerId};
use pack::LoadedPack;

/// Chance de uma expulsão por time por partida, em permilagem — ~4%.
const EXPULSION_CHANCE_PERMILLE: u32 = 40;

/// Piso e teto da sujeira sintética, em escala 0-100 — mesmo layout de
/// `crate::reputation`/`crate::injuries`.
const MIN_SYNTHETIC_DIRTINESS: u8 = 0;
const MAX_SYNTHETIC_DIRTINESS: u8 = 100;

/// Gera a sujeira de cada jogador do pack, na mesma ordem (e portanto no
/// mesmo índice denso) que `crate::progression::initial_roster` — mesmo
/// invariante de indexação de `crate::contract::ContractState`. Chamada
/// uma vez por carreira (`app::GameSession::new`): sem atributos ocultos
/// de verdade ainda (RF-JG-02 completo), a sujeira de um jogador nunca
/// muda depois de gerada.
#[must_use]
pub fn generate_dirtiness(pack: &LoadedPack, world_seed: u64) -> Vec<u8> {
    pack.players
        .iter()
        .map(|p| synthetic_dirtiness(p.id, world_seed))
        .collect()
}

fn synthetic_dirtiness(player: PlayerId, world_seed: u64) -> u8 {
    let mut rng =
        DeterministicRng::seeded(world_seed, "world.dirtiness", u64::from(player.index()), 0);
    let span = u32::from(MAX_SYNTHETIC_DIRTINESS - MIN_SYNTHETIC_DIRTINESS) + 1;
    MIN_SYNTHETIC_DIRTINESS + u8::try_from(rng.below(span)).expect("span cabe em u8 (0..=100)")
}

/// Peso de um jogador no sorteio de quem é expulso, conforme sua sujeira
/// — `1 + sujeira/10`: nunca zero (todo titular pode ser expulso), de 1x
/// (sujeira 0) a 11x (sujeira 100) mais chance que o mínimo. Não
/// calibrado, só a direção certa.
const DIRTINESS_WEIGHT_DIVISOR: u32 = 10;

fn suspension_weight(dirtiness: u8) -> u32 {
    1 + u32::from(dirtiness) / DIRTINESS_WEIGHT_DIVISOR
}

/// Sorteia, entre `starters`, quem é expulso — ponderado por
/// [`suspension_weight`] em vez de uniforme. `dirtiness` tem que estar no
/// mesmo índice denso que o roster inteiro (`generate_dirtiness`), não só
/// `starters`: cada `PlayerId` em `starters` indexa `dirtiness` pelo seu
/// próprio id denso (`PlayerId::as_usize()`).
fn weighted_starter_index(
    starters: &[PlayerId],
    dirtiness: &[u8],
    rng: &mut DeterministicRng,
) -> usize {
    let weights: Vec<u32> = starters
        .iter()
        .map(|p| suspension_weight(dirtiness[p.as_usize()]))
        .collect();
    let total: u32 = weights.iter().sum();
    let mut roll = rng.below(total);
    for (idx, &weight) in weights.iter().enumerate() {
        if roll < weight {
            return idx;
        }
        roll -= weight;
    }
    starters.len() - 1 // nunca deveria chegar aqui: pesos somam `total` exatamente
}

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
/// `dirtiness` tem que estar no mesmo índice denso que o roster inteiro
/// (`generate_dirtiness`, ver o doc do módulo): quem é expulso é
/// ponderado por sujeira ([`weighted_starter_index`]), não uniforme.
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
    dirtiness: &[u8],
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
            let idx = weighted_starter_index(starters, dirtiness, &mut rng);
            suspended.insert(starters[idx]);
        }
    }
    suspended
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Sujeira uniforme (50 pra todo mundo) — reduz o sorteio ponderado
    /// de volta a uniforme, pros testes que não são sobre o efeito da
    /// sujeira em si. Tamanho generoso (100) cobre qualquer `PlayerId`
    /// usado nestes testes.
    fn uniform_dirtiness() -> Vec<u8> {
        vec![50; 100]
    }

    #[test]
    fn time_sem_titular_nunca_e_sorteado() {
        let starters = vec![(ClubId::new(0), vec![])];
        let suspended = roll_suspensions(1, 0, 0, &starters, &uniform_dirtiness());
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
            let suspended = roll_suspensions(seed, 0, 0, &starters, &uniform_dirtiness());
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
            let suspended = roll_suspensions(seed, 0, 0, &starters, &uniform_dirtiness());
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
        let dirtiness = uniform_dirtiness();
        let a = roll_suspensions(3, 0, 0, &starters[..1], &dirtiness);
        let b = roll_suspensions(3, 0, 0, &starters, &dirtiness);
        // O resultado do clube 0 isolado é igual ao resultado do clube 0
        // dentro da lista com o clube 1 — a presença do clube 1 não altera
        // o sorteio do clube 0.
        assert_eq!(a.contains(&PlayerId::new(1)), b.contains(&PlayerId::new(1)));
    }

    #[test]
    fn e_deterministico_para_a_mesma_entrada() {
        let starters = vec![(ClubId::new(0), vec![PlayerId::new(1), PlayerId::new(2)])];
        let dirtiness = uniform_dirtiness();
        assert_eq!(
            roll_suspensions(42, 3, 5, &starters, &dirtiness),
            roll_suspensions(42, 3, 5, &starters, &dirtiness)
        );
    }

    #[test]
    fn rodada_diferente_pode_sortear_diferente() {
        let starters = vec![(ClubId::new(0), vec![PlayerId::new(1), PlayerId::new(2)])];
        let dirtiness = uniform_dirtiness();
        let results: Vec<HashSet<PlayerId>> = (0..50)
            .map(|round| roll_suspensions(99, 0, round, &starters, &dirtiness))
            .collect();
        assert!(
            results.windows(2).any(|w| w[0] != w[1]),
            "50 rodadas produziram sempre o mesmo resultado — RNG não está variando com a rodada"
        );
    }

    #[test]
    fn jogador_mais_sujo_e_suspenso_com_mais_frequencia() {
        // Sanidade estatística: dois titulares do mesmo time, um com
        // sujeira máxima (peso 11x) e um com sujeira mínima (peso 1x) —
        // ao longo de muitas rodadas independentes, o sujo deveria ser o
        // expulso com bem mais frequência que o limpo.
        let starters = vec![(
            ClubId::new(0),
            vec![PlayerId::new(0), PlayerId::new(1)], // 0: limpo, 1: sujo
        )];
        let mut dirtiness = vec![0u8; 2];
        dirtiness[1] = 100;

        let mut dirty_suspensions = 0usize;
        let mut clean_suspensions = 0usize;
        for round in 0..2000u32 {
            let suspended = roll_suspensions(1, 0, round, &starters, &dirtiness);
            if suspended.contains(&PlayerId::new(1)) {
                dirty_suspensions += 1;
            }
            if suspended.contains(&PlayerId::new(0)) {
                clean_suspensions += 1;
            }
        }
        assert!(
            dirty_suspensions > clean_suspensions * 3,
            "jogador sujo deveria ser suspenso com bem mais frequência (sujo: \
             {dirty_suspensions}, limpo: {clean_suspensions})"
        );
    }

    #[test]
    fn generate_dirtiness_e_deterministica_para_a_mesma_seed() {
        use domain::{CompetitionId, GameDate, NationId, Position};
        use pack::{LoadedPack, ResolvedClub, ResolvedNation, ResolvedPlayer};

        let nation = ResolvedNation {
            id: NationId::new(0),
            external_id: "ex".to_string(),
            name: "Ex".to_string(),
        };
        let club = ResolvedClub {
            id: ClubId::new(0),
            external_id: "ex.a".to_string(),
            name: "A".to_string(),
            nation: nation.id,
            competition: CompetitionId::new(0),
            founded: None,
            stadium: None,
        };
        let players = (0..10)
            .map(|i| ResolvedPlayer {
                id: PlayerId::new(i),
                external_id: format!("ex.a.p{i:02}"),
                first_name: "N".to_string(),
                last_name: "S".to_string(),
                birth: GameDate::from_ymd(2000, 1, 1),
                nation: nation.id,
                club: club.id,
                position: Position::Midfielder,
                attributes: domain::PlayerAttributes::default(),
                ability: domain::Ability::new(100, 150),
            })
            .collect();
        let pack = LoadedPack {
            reference_year: 2026,
            nations: vec![nation],
            competitions: vec![],
            clubs: vec![club],
            players,
        };

        assert_eq!(generate_dirtiness(&pack, 7), generate_dirtiness(&pack, 7));
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
            dirtiness_value: u8,
        ) {
            let starters = vec![(ClubId::new(club), vec![PlayerId::new(player)])];
            let mut dirtiness = vec![0u8; 20];
            dirtiness[player as usize] = dirtiness_value;
            prop_assert_eq!(
                roll_suspensions(world_seed, season_index, round, &starters, &dirtiness),
                roll_suspensions(world_seed, season_index, round, &starters, &dirtiness)
            );
        }

        #[test]
        fn roll_suspensions_nunca_suspende_quem_nao_titularizou(
            world_seed: u64,
            season_index: u32,
            round: u32,
            dirtiness_a: u8,
            dirtiness_b: u8,
        ) {
            let starters = vec![(ClubId::new(0), vec![PlayerId::new(1), PlayerId::new(2)])];
            let mut dirtiness = vec![0u8; 3];
            dirtiness[1] = dirtiness_a;
            dirtiness[2] = dirtiness_b;
            let suspended = roll_suspensions(world_seed, season_index, round, &starters, &dirtiness);
            for player in &suspended {
                prop_assert!(*player == PlayerId::new(1) || *player == PlayerId::new(2));
            }
        }
    }
}
