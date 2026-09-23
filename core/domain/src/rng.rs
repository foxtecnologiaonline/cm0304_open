//! RNG determinístico, por subsistema e por entidade — nunca um gerador global.
//!
//! `docs/02-arquitetura.md §5` exige que simular a partida A não possa mudar
//! o resultado da partida B, e que o mesmo `(seed_mundo, comandos)` produza
//! sempre o mesmo mundo, em qualquer arquitetura. A forma de garantir isso é
//! **nunca compartilhar um gerador**: cada chamada que precisa de
//! aleatoriedade deriva o seu próprio `DeterministicRng` a partir de
//! `(seed_mundo, domínio, entidade, tick)`.
//!
//! A mistura de seed usa `splitmix64` (algoritmo público, um passo, sem
//! estado) para produzir os 128 bits que alimentam um `Pcg64`
//! (`rand_pcg`) — gerador rápido, com boas propriedades estatísticas e
//! comportamento idêntico em qualquer plataforma que implemente `u64`
//! corretamente (ou seja: todas as nossas plataformas-alvo).

use rand_core::{RngCore, SeedableRng};
use rand_pcg::Pcg64;

/// Gerador determinístico derivado de uma seed composta. Nunca compartilhado
/// entre entidades ou subsistemas — ver módulo para o porquê.
pub struct DeterministicRng {
    inner: Pcg64,
}

impl DeterministicRng {
    /// Deriva um gerador a partir de `(seed_mundo, domínio, entidade, tick)`.
    ///
    /// `domain` identifica o subsistema (ex.: `"match.shot"`, `"transfer.ai"`,
    /// `"regen.generate"`) — subsistemas diferentes com a mesma entidade e o
    /// mesmo tick produzem sequências independentes.
    #[must_use]
    pub fn seeded(world_seed: u64, domain: &str, entity: u64, tick: u64) -> Self {
        let s0 = splitmix64(world_seed);
        let s1 = splitmix64(s0 ^ fnv1a_64(domain.as_bytes()));
        let s2 = splitmix64(s1 ^ entity);
        let s3 = splitmix64(s2 ^ tick);
        // `Pcg64::Seed` são 32 bytes (estado de 128 bits + seleção de stream
        // de 128 bits) — completamos a cadeia de mistura até preencher tudo.
        let s4 = splitmix64(s3);
        let s5 = splitmix64(s4);

        let mut seed = [0u8; 32];
        seed[0..8].copy_from_slice(&s2.to_le_bytes());
        seed[8..16].copy_from_slice(&s3.to_le_bytes());
        seed[16..24].copy_from_slice(&s4.to_le_bytes());
        seed[24..32].copy_from_slice(&s5.to_le_bytes());

        Self {
            inner: Pcg64::from_seed(seed),
        }
    }

    #[must_use]
    pub fn next_u32(&mut self) -> u32 {
        self.inner.next_u32()
    }

    #[must_use]
    pub fn next_u64(&mut self) -> u64 {
        self.inner.next_u64()
    }

    /// Inteiro uniforme em `0..bound`, sem viés de módulo (algoritmo de
    /// Lemire, inteiro puro). `bound` deve ser maior que zero.
    #[must_use]
    pub fn below(&mut self, bound: u32) -> u32 {
        debug_assert!(bound > 0, "DeterministicRng::below: bound deve ser > 0");
        let bound = u64::from(bound);
        loop {
            let r = u64::from(self.next_u32());
            let m = r * bound;
            let low = m & 0xFFFF_FFFF;
            if low < bound {
                let threshold = bound.wrapping_neg() % bound;
                if low < threshold {
                    continue;
                }
            }
            return (m >> 32) as u32;
        }
    }

    /// `true` com probabilidade `per_mille`/1000 (0 nunca, 1000 sempre).
    /// A mesma escala de milésimos usada por `Fixed` (`docs/03 §4`).
    #[must_use]
    pub fn chance_per_mille(&mut self, per_mille: u32) -> bool {
        if per_mille == 0 {
            return false;
        }
        if per_mille >= 1000 {
            return true;
        }
        self.below(1000) < per_mille
    }

    /// Sorteia um índice em `0..len` — conveniência para escolher entre
    /// alternativas de igual peso (ex.: qual defensor comete a falta).
    #[must_use]
    pub fn pick_index(&mut self, len: usize) -> usize {
        debug_assert!(len > 0, "DeterministicRng::pick_index: len deve ser > 0");
        self.below(len as u32) as usize
    }
}

/// `splitmix64` — gerador de um passo usado só para *misturar* seeds, não
/// para gerar a sequência de jogo em si (isso é papel do `Pcg64`).
const fn splitmix64(mut x: u64) -> u64 {
    x = x.wrapping_add(0x9E37_79B9_7F4A_7C15);
    let mut z = x;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^ (z >> 31)
}

/// FNV-1a de 64 bits — hash estável e documentado para transformar o nome do
/// subsistema (`&str`) em `u64`, sem depender do `Hasher` padrão da stdlib
/// (que não garante estabilidade entre versões do compilador).
const fn fnv1a_64(bytes: &[u8]) -> u64 {
    const OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01B3;
    let mut hash = OFFSET;
    let mut i = 0;
    while i < bytes.len() {
        hash ^= bytes[i] as u64;
        hash = hash.wrapping_mul(PRIME);
        i += 1;
    }
    hash
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mesma_seed_produz_a_mesma_sequencia() {
        let mut a = DeterministicRng::seeded(42, "match.shot", 7, 100);
        let mut b = DeterministicRng::seeded(42, "match.shot", 7, 100);
        let seq_a: Vec<u32> = (0..50).map(|_| a.next_u32()).collect();
        let seq_b: Vec<u32> = (0..50).map(|_| b.next_u32()).collect();
        assert_eq!(seq_a, seq_b);
    }

    #[test]
    fn entidades_diferentes_nao_compartilham_sequencia() {
        let mut a = DeterministicRng::seeded(42, "match.shot", 7, 100);
        let mut b = DeterministicRng::seeded(42, "match.shot", 8, 100);
        assert_ne!(a.next_u64(), b.next_u64());
    }

    #[test]
    fn dominios_diferentes_nao_compartilham_sequencia() {
        // A mesma "partida" (mesmo entity/tick) em dois subsistemas distintos
        // não pode vazar correlação — é o que impede que simular a partida A
        // influencie a partida B por acidente de implementação.
        let mut a = DeterministicRng::seeded(42, "match.shot", 7, 100);
        let mut b = DeterministicRng::seeded(42, "match.foul", 7, 100);
        assert_ne!(a.next_u64(), b.next_u64());
    }

    #[test]
    fn ticks_diferentes_nao_compartilham_sequencia() {
        let mut a = DeterministicRng::seeded(42, "match.shot", 7, 1);
        let mut b = DeterministicRng::seeded(42, "match.shot", 7, 2);
        assert_ne!(a.next_u64(), b.next_u64());
    }

    #[test]
    fn below_nunca_estoura_o_limite() {
        let mut rng = DeterministicRng::seeded(1, "test.below", 0, 0);
        for _ in 0..10_000 {
            assert!(rng.below(7) < 7);
        }
    }

    #[test]
    fn chance_per_mille_extremos_sao_deterministicos() {
        let mut rng = DeterministicRng::seeded(1, "test.chance", 0, 0);
        assert!(!rng.chance_per_mille(0));
        assert!(rng.chance_per_mille(1000));
    }

    #[test]
    fn chance_per_mille_converge_para_a_probabilidade_pedida() {
        let mut rng = DeterministicRng::seeded(9, "test.chance.converge", 0, 0);
        let trials = 200_000;
        let hits = (0..trials).filter(|_| rng.chance_per_mille(100)).count();
        let ratio_permille = hits * 1000 / trials;
        // 10% esperado; com 200k tentativas a folga de ±10‰ absoluto é generosa
        // o bastante para não ser um teste flaky, e apertada o bastante para
        // pegar um bug real de viés.
        assert!(
            (90..=110).contains(&ratio_permille),
            "taxa observada {ratio_permille}‰, esperado ~100‰"
        );
    }
}
