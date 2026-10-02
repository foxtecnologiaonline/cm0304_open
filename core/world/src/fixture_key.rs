//! Empacotamento de `(temporada, mandante, visitante)` num único `u64` para
//! `engine::MatchContext.fixture` — a chave de RNG de uma partida
//! (`docs/02 §5`).
//!
//! Usado por `crate::season::fixture_key` (liga) e `crate::cup::cup_fixture_key`
//! (copa), que antes reimplementavam o mesmo empacotamento de bits cada um
//! com sua própria constante `BITS` local. Dois lugares com a mesma
//! constante só ficam sincronizados por comentário — se um dia alguém
//! precisasse aumentar `BITS` (mais clubes ou mais temporadas que hoje) e
//! só atualizasse um dos dois, os dois espaços de chave divergiriam
//! silenciosamente, e em build `--release` (onde `debug_assert!` não roda —
//! é assim que `cli-gate` builda, `.github/workflows/rust-ci.yml`) um
//! índice fora da faixa daria *wraparound* em vez de pane, produzindo uma
//! colisão de chave entre duas partidas diferentes sem nenhum aviso. Esta
//! função compartilhada existe só para fechar esse risco: `BITS` mora num
//! lugar só, então não tem como divergir entre liga e copa.

use domain::ClubId;

/// Bits por campo — 2^20 ≈ 1 milhão, folga generosa sobre o volume alvo de
/// `docs/03 §9`.
pub(crate) const BITS: u32 = 20;

/// Bit mais alto, reservado para separar o espaço de chaves da copa do de
/// liga — sem isso, uma partida de copa entre dois clubes que também se
/// enfrentam na liga na mesma temporada reproduziria byte a byte o mesmo
/// sorteio da partida de liga (`crate::cup`, doc do módulo).
const CUP_FLAG: u64 = 1 << 63;

/// Empacota a chave. `is_cup` seta o bit reservado — `false` reproduz
/// exatamente o esquema histórico de `season::fixture_key`.
#[must_use]
pub(crate) fn pack(season_index: u32, home: ClubId, away: ClubId, is_cup: bool) -> u64 {
    debug_assert!(home.index() < (1 << BITS));
    debug_assert!(away.index() < (1 << BITS));
    debug_assert!(season_index < (1 << BITS));
    let base = (u64::from(season_index) << (2 * BITS))
        | (u64::from(home.index()) << BITS)
        | u64::from(away.index());
    if is_cup { base | CUP_FLAG } else { base }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn liga_e_copa_nunca_colidem_para_a_mesma_entrada() {
        let league = pack(0, ClubId::new(1), ClubId::new(2), false);
        let cup = pack(0, ClubId::new(1), ClubId::new(2), true);
        assert_ne!(league, cup);
        assert_eq!(league & CUP_FLAG, 0);
        assert_eq!(cup & CUP_FLAG, CUP_FLAG);
    }

    #[test]
    fn par_invertido_produz_chave_diferente() {
        let a = pack(0, ClubId::new(1), ClubId::new(2), false);
        let b = pack(0, ClubId::new(2), ClubId::new(1), false);
        assert_ne!(a, b);
    }
}

/// Testes de propriedade (`docs/08 §1`).
#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Empacota exatamente (sem perda) dentro dos limites documentados —
        /// decompor a chave devolve os três valores originais, pro lado de
        /// liga e pro lado de copa igualmente.
        #[test]
        fn pack_e_reversivel_dentro_dos_limites(
            season in 0u32..(1 << BITS),
            home in 0u32..(1 << BITS),
            away in 0u32..(1 << BITS),
            is_cup: bool,
        ) {
            let key = pack(season, ClubId::new(home), ClubId::new(away), is_cup);

            const MASK: u64 = (1 << BITS) - 1;
            let decoded_away = (key & MASK) as u32;
            let decoded_home = ((key >> BITS) & MASK) as u32;
            let decoded_season = ((key >> (2 * BITS)) & MASK) as u32;

            prop_assert_eq!(decoded_away, away);
            prop_assert_eq!(decoded_home, home);
            prop_assert_eq!(decoded_season, season);
            prop_assert_eq!(key & CUP_FLAG != 0, is_cup);
        }

        #[test]
        fn pack_e_deterministico(season: u32, home: u32, away: u32, is_cup: bool) {
            let season = season % (1 << BITS);
            let home = home % (1 << BITS);
            let away = away % (1 << BITS);
            prop_assert_eq!(
                pack(season, ClubId::new(home), ClubId::new(away), is_cup),
                pack(season, ClubId::new(home), ClubId::new(away), is_cup)
            );
        }
    }
}
