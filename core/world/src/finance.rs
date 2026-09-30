//! Orçamento de clube para o mercado (`crate::market`) — hoje sempre
//! sintético: o pack ainda não declara `finance`/orçamento (`docs/03 §1`
//! lista `CLUB ||--|| FINANCE` no modelo de domínio, mas nenhum campo
//! disso existe em `pack::ResolvedClub` ainda). Mesma filosofia de
//! `crate::strength`'s força sintética: um substituto sorteado
//! deterministicamente, documentado, não uma simulação de finanças de
//! verdade (receita de bilheteria, patrocínio, folha salarial — nenhuma
//! existe, `docs/01 §2.8`).

use domain::{ClubId, DeterministicRng, Money};
use pack::LoadedPack;

/// Piso e teto do orçamento sintético, em centavos — magnitude escolhida
/// pra ficar na mesma ordem de grandeza de alguns jogadores de
/// `ai::market_value` (CA médio ~100 → ~1.000.000,00), não uma calibração
/// econômica de verdade.
const MIN_SYNTHETIC_BUDGET_CENTS: i64 = 10_000_000; // 100.000,00
const MAX_SYNTHETIC_BUDGET_CENTS: i64 = 1_000_000_000; // 10.000.000,00

/// Gera um orçamento por clube, indexado pelo id denso do clube
/// (`ClubId::as_usize()`) — mesmo layout de `crate::strength::generate_strengths`.
#[must_use]
pub fn generate_budgets(pack: &LoadedPack, world_seed: u64) -> Vec<Money> {
    pack.clubs
        .iter()
        .map(|club| synthetic_budget(club.id, world_seed))
        .collect()
}

fn synthetic_budget(club: ClubId, world_seed: u64) -> Money {
    let mut rng =
        DeterministicRng::seeded(world_seed, "world.club_budget", u64::from(club.index()), 0);
    let span = (MAX_SYNTHETIC_BUDGET_CENTS - MIN_SYNTHETIC_BUDGET_CENTS) as u32;
    let cents = MIN_SYNTHETIC_BUDGET_CENTS + i64::from(rng.below(span));
    Money::from_cents(cents)
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
    fn gera_um_orcamento_por_clube_dentro_dos_limites() {
        let p = example_pack();
        let budgets = generate_budgets(&p, 42);
        assert_eq!(budgets.len(), p.clubs.len());
        for b in &budgets {
            assert!(b.cents() >= MIN_SYNTHETIC_BUDGET_CENTS);
            assert!(b.cents() <= MAX_SYNTHETIC_BUDGET_CENTS);
        }
    }

    #[test]
    fn e_deterministico_para_a_mesma_seed() {
        let p = example_pack();
        assert_eq!(generate_budgets(&p, 7), generate_budgets(&p, 7));
    }

    #[test]
    fn seeds_diferentes_dao_orcamentos_diferentes() {
        let p = example_pack();
        assert_ne!(generate_budgets(&p, 1), generate_budgets(&p, 2));
    }
}
