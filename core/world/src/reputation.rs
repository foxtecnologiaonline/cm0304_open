//! Reputação de clube — fatia mínima de RF-MU-08 (`docs/01-requisitos.md
//! §2.4`: "Reputação de país/clube influenciando atratividade, receita e
//! IA"). Hoje só a parte de **receita**: `crate::finance::match_day_revenue`
//! usa a reputação do mandante pra puxar o piso de público pra cima —
//! clube mais reputado atrai casa mais cheia em média, nunca joga pra
//! arquibancada vazia. "Atratividade" (influenciar quem um jogador prefere
//! jogar) e "IA" (clubes reputados competindo por reforços melhores) ainda
//! faltam — pedem decisão/preferência do lado do jogador ou da IA de
//! mercado, que hoje não considera reputação nenhuma (`ai::market_value`).
//!
//! Só reputação de **clube**; reputação de **país** (a outra metade de
//! RF-MU-08) não existe ainda — o pack não declara nada parecido com força
//! relativa de liga nacional.
//!
//! Sintética, mesma filosofia de `crate::finance`'s orçamento/capacidade: o
//! pack ainda não declara reputação de verdade (nenhum histórico de
//! títulos, tradição ou mídia existe pra calcular isso de verdade — RF-CL-08,
//! M5). Escala 0-100 (percentual, não CA): mais perto de "nota" que de
//! atributo de jogador.

use domain::{ClubId, DeterministicRng};
use pack::LoadedPack;

const MIN_SYNTHETIC_REPUTATION: u8 = 0;
const MAX_SYNTHETIC_REPUTATION: u8 = 100;

/// Gera a reputação de cada clube, indexada pelo id denso do clube —
/// mesmo layout de `crate::finance::generate_budgets`. Chamada uma vez por
/// carreira (`app::GameSession::new`): sem histórico de títulos ainda
/// (RF-CL-08, M5), não há como a reputação de um clube mudar depois de
/// gerada.
#[must_use]
pub fn generate_club_reputation(pack: &LoadedPack, world_seed: u64) -> Vec<u8> {
    pack.clubs
        .iter()
        .map(|club| synthetic_reputation(club.id, world_seed))
        .collect()
}

fn synthetic_reputation(club: ClubId, world_seed: u64) -> u8 {
    let mut rng =
        DeterministicRng::seeded(world_seed, "world.reputation", u64::from(club.index()), 0);
    let span = u32::from(MAX_SYNTHETIC_REPUTATION - MIN_SYNTHETIC_REPUTATION) + 1;
    MIN_SYNTHETIC_REPUTATION + u8::try_from(rng.below(span)).expect("span cabe em u8 (0..=100)")
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

    #[test]
    fn gera_uma_reputacao_por_clube_dentro_dos_limites() {
        let p = example_pack();
        let reputation = generate_club_reputation(&p, 42);
        assert_eq!(reputation.len(), p.clubs.len());
        for &r in &reputation {
            assert!(r <= MAX_SYNTHETIC_REPUTATION);
        }
    }

    #[test]
    fn e_deterministica_para_a_mesma_seed() {
        let p = example_pack();
        assert_eq!(
            generate_club_reputation(&p, 7),
            generate_club_reputation(&p, 7)
        );
    }

    #[test]
    fn seeds_diferentes_dao_reputacoes_diferentes() {
        let p = example_pack();
        assert_ne!(
            generate_club_reputation(&p, 1),
            generate_club_reputation(&p, 2)
        );
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
        #[test]
        fn generate_club_reputation_e_deterministico_para_qualquer_seed(world_seed: u64) {
            let p = example_pack();
            prop_assert_eq!(
                generate_club_reputation(&p, world_seed),
                generate_club_reputation(&p, world_seed)
            );
        }
    }
}
