//! Força sintética por clube — um substituto temporário para "calcular a
//! força a partir do elenco de verdade" (`docs/04 §2.1`), que só existe
//! quando `ai`/jogadores chegarem (M2/M3, `docs/07-roadmap.md`).
//!
//! Sem isso, `world` não teria nada para passar pro `engine::simulate` —
//! mas também não faz sentido inventar uma fórmula sofisticada aqui só pra
//! jogar fora depois. A força de cada clube é sorteada uma vez, de forma
//! determinística, e permanece constante durante toda a sequência de
//! temporadas simulada (o "elenco" sintético não evolui) — suficiente para
//! exercitar calendário, tabela e promoção/rebaixamento de ponta a ponta.

use domain::{ClubId, DeterministicRng, Fixed};
use engine::TeamStrength;
use pack::LoadedPack;

/// Piso e teto da força sintética — mesma ordem de grandeza usada nos
/// testes do `engine` (`Fixed::from_int(100)` como referência "média").
const MIN_SYNTHETIC_STRENGTH: i32 = 50;
const MAX_SYNTHETIC_STRENGTH: i32 = 150;

/// Gera uma força por clube, indexada pelo id denso do clube
/// (`ClubId::as_usize()`) — o mesmo layout SoA-friendly de `docs/02 §6.1`.
#[must_use]
pub fn generate_strengths(pack: &LoadedPack, world_seed: u64) -> Vec<TeamStrength> {
    pack.clubs
        .iter()
        .map(|club| {
            let mut rng = DeterministicRng::seeded(
                world_seed,
                "world.club_strength",
                u64::from(club.id.index()),
                0,
            );
            let span = (MAX_SYNTHETIC_STRENGTH - MIN_SYNTHETIC_STRENGTH + 1) as u32;
            let value = MIN_SYNTHETIC_STRENGTH + rng.below(span) as i32;
            TeamStrength::new(Fixed::from_int(value))
        })
        .collect()
}

/// Busca a força de um clube pelo seu id denso — painel de acesso indexado,
/// igual ao resto do domínio (`docs/02 §6.1`).
#[must_use]
pub fn strength_of(strengths: &[TeamStrength], club: ClubId) -> TeamStrength {
    strengths[club.as_usize()]
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
    fn gera_uma_forca_por_clube_dentro_dos_limites() {
        let p = example_pack();
        let strengths = generate_strengths(&p, 42);
        assert_eq!(strengths.len(), p.clubs.len());
        for s in &strengths {
            assert!(s.value() >= Fixed::from_int(MIN_SYNTHETIC_STRENGTH));
            assert!(s.value() <= Fixed::from_int(MAX_SYNTHETIC_STRENGTH));
        }
    }

    #[test]
    fn e_deterministico_para_a_mesma_seed() {
        let p = example_pack();
        let a = generate_strengths(&p, 7);
        let b = generate_strengths(&p, 7);
        assert_eq!(a, b);
    }

    #[test]
    fn seeds_diferentes_dao_forcas_diferentes() {
        let p = example_pack();
        let a = generate_strengths(&p, 1);
        let b = generate_strengths(&p, 2);
        assert_ne!(a, b);
    }
}
