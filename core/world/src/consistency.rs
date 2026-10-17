//! Consistência — sexta fatia real de RF-JG-02 (atributos ocultos, `docs/01
//! §2.3`): o quanto o rendimento de um jogador varia de rodada pra rodada,
//! não sua qualidade média (já capturada pelo CA, `crate::quality`). Mora
//! aqui, não em `crate::quality`, porque o único consumidor é o loop por
//! rodada de `crate::season` — mesmo motivo de `crate::market::generate_loyalty`
//! morar lá, não em `crate::progression` (co-localização pelo consumidor
//! principal).
//!
//! [`generate_consistency`] sorteia uma consistência sintética (0-100) por
//! jogador, uma vez por carreira. [`apply_consistency_swing`] usa a média
//! dos titulares de cada clube numa rodada pra sortear um "balanço" de
//! força **zero-médio**: consistência mínima (0) balança até ±20% pra cima
//! ou pra baixo a cada rodada; consistência máxima (100) nunca balança (o
//! time sempre rende exatamente o perfil calculado, como antes desta
//! fatia). Ao contrário de sujeira (`crate::discipline`) ou jogo importante
//! (`crate::cup`) — que **direcionam** um resultado —, isto é ruído:
//! aumenta a variância observada rodada a rodada, mas não desloca a força
//! esperada ao longo de muitas rodadas (sorteio simétrico ao redor de 1x).
//!
//! Só existe no caminho de carreira (loop por rodada de
//! `crate::season::simulate_round_robin_for_career`); o caminho estático
//! (`managerfc-cli calibrate`/`bench`) usa a força bruta, sem balanço —
//! mesma disciplina de todo outro atributo oculto já implementado. Copa
//! (`crate::cup`) também não ganha balanço: ela já não tem granularidade de
//! rodada (usa o perfil estático do início da temporada em qualquer
//! caminho, ver o doc de `crate::cup`), então não há "rodada" nenhuma aqui
//! pra sortear um balanço contra.

use domain::{ClubId, DeterministicRng, Fixed, PlayerId};
use engine::TeamMatchProfile;
use pack::LoadedPack;

/// Piso e teto da consistência sintética, em escala 0-100 — mesmo layout de
/// `crate::discipline`/`crate::cup`.
const MIN_SYNTHETIC_CONSISTENCY: u8 = 0;
const MAX_SYNTHETIC_CONSISTENCY: u8 = 100;

/// Gera a consistência de cada jogador do pack, na mesma ordem (e portanto
/// no mesmo índice denso) que `crate::progression::initial_roster`. Chamada
/// uma vez por carreira (`app::GameSession::new`): sem atributos ocultos de
/// verdade ainda (RF-JG-02 completo), a consistência de um jogador nunca
/// muda depois de gerada.
#[must_use]
pub fn generate_consistency(pack: &LoadedPack, world_seed: u64) -> Vec<u8> {
    pack.players
        .iter()
        .map(|p| synthetic_consistency(p.id, world_seed))
        .collect()
}

fn synthetic_consistency(player: PlayerId, world_seed: u64) -> u8 {
    let mut rng = DeterministicRng::seeded(
        world_seed,
        "world.consistency",
        u64::from(player.index()),
        0,
    );
    let span = u32::from(MAX_SYNTHETIC_CONSISTENCY - MIN_SYNTHETIC_CONSISTENCY) + 1;
    MIN_SYNTHETIC_CONSISTENCY + u8::try_from(rng.below(span)).expect("span cabe em u8 (0..=100)")
}

/// Balanço máximo de força, em permilagem, para consistência mínima (0) —
/// ±20%. Consistência 100 nunca balança (intervalo de largura 0). Não
/// calibrado, só a direção certa.
const MAX_SWING_PERMILLE: u32 = 200;

/// Escala a força de `profile` por um balanço zero-médio, sorteado a partir
/// da consistência média de `starters` naquela rodada — `starters` tem que
/// já refletir qualquer exclusão da rodada (lesão/suspensão, ver
/// `crate::season`); `consistency` tem que estar no mesmo índice denso que
/// o roster inteiro (`generate_consistency`), não só `starters`. `tick`
/// combina temporada+rodada (`crate::discipline::season_round_tick`) — o
/// mesmo balanço nunca se repete entre rodadas nem temporadas diferentes.
/// Time sem titular elegível (`starters` vazio) devolve `profile` sem
/// balanço — nada pra tirar média.
#[must_use]
pub fn apply_consistency_swing(
    profile: TeamMatchProfile,
    starters: &[PlayerId],
    consistency: &[u8],
    world_seed: u64,
    club: ClubId,
    tick: u64,
) -> TeamMatchProfile {
    if starters.is_empty() {
        return profile;
    }
    let total: u32 = starters
        .iter()
        .map(|&p| u32::from(consistency[p.as_usize()]))
        .sum();
    let average = total / starters.len() as u32;
    let range = MAX_SWING_PERMILLE * (100 - average) / 100;

    let mut rng = DeterministicRng::seeded(
        world_seed,
        "world.consistency_swing",
        u64::from(club.index()),
        tick,
    );
    let swing_permille: i32 = if range == 0 {
        0
    } else {
        rng.below(2 * range + 1) as i32 - range as i32
    };

    let multiplier = Fixed::from_ratio(1000 + i64::from(swing_permille), 1000);
    TeamMatchProfile {
        strength: engine::TeamStrength::new(profile.strength.value() * multiplier),
        ..profile
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{Ability, CompetitionId, GameDate, NationId, Position};
    use engine::{FinishingQuality, GoalkeepingQuality, TeamStrength};
    use pack::{ResolvedClub, ResolvedNation, ResolvedPlayer};

    fn example_pack() -> LoadedPack {
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
                ability: Ability::new(100, 150),
            })
            .collect();
        LoadedPack {
            reference_year: 2026,
            nations: vec![nation],
            competitions: vec![],
            clubs: vec![club],
            players,
        }
    }

    fn profile(strength: i32) -> TeamMatchProfile {
        TeamMatchProfile {
            strength: TeamStrength::new(Fixed::from_int(strength)),
            finishing: FinishingQuality::new(Fixed::from_int(10)),
            goalkeeping: GoalkeepingQuality::new(Fixed::from_int(10)),
        }
    }

    #[test]
    fn generate_consistency_e_deterministica_para_a_mesma_seed() {
        let p = example_pack();
        assert_eq!(generate_consistency(&p, 7), generate_consistency(&p, 7));
    }

    #[test]
    fn sem_titular_devolve_o_perfil_sem_mudanca() {
        let base = profile(300);
        let result = apply_consistency_swing(base, &[], &[], 1, ClubId::new(0), 0);
        assert_eq!(result, base);
    }

    #[test]
    fn consistencia_maxima_nunca_balanca() {
        let starters = vec![PlayerId::new(0), PlayerId::new(1)];
        let consistency = vec![100u8, 100u8];
        let base = profile(300);
        for tick in 0..100u64 {
            let result =
                apply_consistency_swing(base, &starters, &consistency, 1, ClubId::new(0), tick);
            assert_eq!(
                result.strength.value(),
                base.strength.value(),
                "consistência 100 deveria nunca balançar a força"
            );
        }
    }

    #[test]
    fn e_deterministico_para_a_mesma_entrada() {
        let starters = vec![PlayerId::new(0), PlayerId::new(1)];
        let consistency = vec![20u8, 30u8];
        let base = profile(300);
        assert_eq!(
            apply_consistency_swing(base, &starters, &consistency, 42, ClubId::new(0), 5),
            apply_consistency_swing(base, &starters, &consistency, 42, ClubId::new(0), 5)
        );
    }

    #[test]
    fn consistencia_baixa_de_fato_balanca_a_forca_ao_longo_de_varias_rodadas() {
        // Sanidade estatística: com consistência mínima, pelo menos algumas
        // das 200 rodadas sorteadas deveriam produzir uma força diferente
        // do perfil base — "nunca balança" seria bug, não sorte.
        let starters = vec![PlayerId::new(0), PlayerId::new(1)];
        let consistency = vec![0u8, 0u8];
        let base = profile(300);
        let mut saw_swing = false;
        for tick in 0..200u64 {
            let result =
                apply_consistency_swing(base, &starters, &consistency, 1, ClubId::new(0), tick);
            if result.strength.value() != base.strength.value() {
                saw_swing = true;
                break;
            }
        }
        assert!(
            saw_swing,
            "consistência 0 deveria balançar a força em pelo menos uma de 200 rodadas"
        );
    }

    #[test]
    fn times_diferentes_balancam_de_forma_independente() {
        let starters = vec![PlayerId::new(0)];
        let consistency = vec![10u8];
        let base = profile(300);
        let a = apply_consistency_swing(base, &starters, &consistency, 3, ClubId::new(0), 7);
        let b = apply_consistency_swing(base, &starters, &consistency, 3, ClubId::new(1), 7);
        // Mesma entrada exceto o clube — RNG derivado por `club.index()`
        // deveria (quase sempre) produzir balanços diferentes.
        assert_ne!(a.strength.value(), b.strength.value());
    }
}
