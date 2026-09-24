//! Ponto fixo determinístico — a única forma de número fracionário permitida
//! em caminho de estado (ver `ADR 0002` e `docs/03-modelo-de-dados.md §4`).
//!
//! `Fixed` guarda um `i32` em escala 1/1000 (milésimos). Toda operação é
//! inteira, então o resultado é byte-a-byte idêntico em x86-64 e ARM64 — ao
//! contrário de `f32`/`f64`, cujo arredondamento pode divergir entre
//! arquiteturas e unidades de vetorização.
//!
//! `Fixed::to_ratio` existe só para exibição/depuração (logs, relatórios de
//! calibração) — nunca para realimentar o estado do jogo. Não há, de
//! propósito, nenhuma conversão para `f32`/`f64`: o crate nega
//! `clippy::float_arithmetic`, e introduzir uma dependência de `f64` aqui
//! reabriria exatamente o buraco que este tipo existe para fechar.

use std::cmp::Ordering;
use std::fmt;
use std::ops::{Add, AddAssign, Div, Mul, Neg, Sub, SubAssign};

/// Escala interna: 1 unidade de `Fixed` = 1/1000.
pub const SCALE: i32 = 1_000;

/// Número racional determinístico em ponto fixo, escala 1/1000.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Fixed(i32);

impl Fixed {
    /// Zero.
    pub const ZERO: Fixed = Fixed(0);
    /// Um inteiro (1.000).
    pub const ONE: Fixed = Fixed(SCALE);

    /// Constrói a partir do valor bruto em milésimos (uso interno/serialização).
    #[must_use]
    pub const fn from_milli(raw: i32) -> Self {
        Self(raw)
    }

    /// Constrói a partir de um inteiro exato (`Fixed::from_int(3)` = 3.000).
    #[must_use]
    pub const fn from_int(value: i32) -> Self {
        Self(value.saturating_mul(SCALE))
    }

    /// Constrói a partir de uma razão inteira `num/den`, arredondando para o
    /// mais próximo — determinístico, sem passar por ponto flutuante.
    #[must_use]
    pub const fn from_ratio(num: i64, den: i64) -> Self {
        debug_assert!(den != 0, "Fixed::from_ratio: denominador zero");
        Self(round_div_i64(num * SCALE as i64, den))
    }

    /// Valor bruto em milésimos (para serialização no save).
    #[must_use]
    pub const fn raw_milli(self) -> i32 {
        self.0
    }

    /// Parte inteira, truncada em direção a zero.
    #[must_use]
    pub const fn trunc(self) -> i32 {
        self.0 / SCALE
    }

    /// Representação `(inteiro, milésimos)` — única forma aprovada de exibir
    /// o valor sem sair do domínio inteiro.
    #[must_use]
    pub const fn to_ratio(self) -> (i32, i32) {
        (self.0 / SCALE, self.0 % SCALE)
    }

    #[must_use]
    pub const fn abs(self) -> Self {
        Self(self.0.abs())
    }

    #[must_use]
    pub fn clamp(self, lo: Fixed, hi: Fixed) -> Self {
        Self(self.0.clamp(lo.0, hi.0))
    }

    /// Multiplica por um inteiro pequeno sem risco de overflow silencioso —
    /// usa `i64` internamente e satura nos limites de `i32`.
    #[must_use]
    pub fn mul_int(self, factor: i32) -> Self {
        let product = i64::from(self.0) * i64::from(factor);
        Self(product.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32)
    }
}

impl Add for Fixed {
    type Output = Fixed;
    fn add(self, rhs: Self) -> Self::Output {
        Fixed(self.0 + rhs.0)
    }
}

impl Sub for Fixed {
    type Output = Fixed;
    fn sub(self, rhs: Self) -> Self::Output {
        Fixed(self.0 - rhs.0)
    }
}

impl Neg for Fixed {
    type Output = Fixed;
    fn neg(self) -> Self::Output {
        Fixed(-self.0)
    }
}

impl AddAssign for Fixed {
    fn add_assign(&mut self, rhs: Self) {
        self.0 += rhs.0;
    }
}

impl SubAssign for Fixed {
    fn sub_assign(&mut self, rhs: Self) {
        self.0 -= rhs.0;
    }
}

impl Mul for Fixed {
    type Output = Fixed;
    /// Multiplicação de dois `Fixed`: usa `i64` intermediário para não
    /// perder precisão antes de reescalar de volta para milésimos, e a
    /// mesma regra de arredondamento de `from_ratio` (ver `round_div_i64`) —
    /// sem isso, `Fixed::from_ratio(2, 3)` e
    /// `Fixed::from_int(2) / Fixed::from_int(3)` divergiam no último dígito
    /// (667 vs. 666) para o mesmo valor matemático.
    fn mul(self, rhs: Self) -> Self::Output {
        Fixed(round_div_i64(
            i64::from(self.0) * i64::from(rhs.0),
            i64::from(SCALE),
        ))
    }
}

impl Div for Fixed {
    type Output = Fixed;
    fn div(self, rhs: Self) -> Self::Output {
        debug_assert!(rhs.0 != 0, "Fixed::div: divisão por zero");
        Fixed(round_div_i64(
            i64::from(self.0) * i64::from(SCALE),
            i64::from(rhs.0),
        ))
    }
}

/// Divide `num` por `den` arredondando para o mais próximo (metade afastada
/// de zero), em aritmética inteira — a única regra de arredondamento usada
/// em todo o tipo `Fixed` (`from_ratio`, `Mul`, `Div`), para que o mesmo
/// valor matemático produza sempre o mesmo `raw_milli`, não importa por qual
/// operação foi construído.
///
/// Normaliza para denominador positivo **antes** de decidir o sinal do
/// ajuste de arredondamento. Sem isso, `num` e `den` negativos ao mesmo
/// tempo (quociente positivo) caem no ramo errado: por exemplo
/// `round_div_i64(-1_000_000, -1_000)` — que é exatamente `1_000_000/1_000`,
/// uma divisão exata — descontava `half` do lado errado e devolvia `999`
/// em vez de `1000`. Foi um teste de propriedade (`fixed::proptests::div_e_from_ratio_concordam_para_pares_arbitrarios`)
/// que achou esse caso; o par `(-1, -1)` virou teste de regressão fixo.
const fn round_div_i64(num: i64, den: i64) -> i32 {
    let (num, den) = if den < 0 { (-num, -den) } else { (num, den) };
    let half = den / 2;
    let rounded = if num >= 0 {
        (num + half) / den
    } else {
        (num - half) / den
    };
    rounded as i32
}

impl PartialEq<i32> for Fixed {
    fn eq(&self, other: &i32) -> bool {
        self.0 == other.saturating_mul(SCALE)
    }
}

impl PartialOrd<i32> for Fixed {
    fn partial_cmp(&self, other: &i32) -> Option<Ordering> {
        self.0.partial_cmp(&other.saturating_mul(SCALE))
    }
}

impl fmt::Display for Fixed {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (int_part, frac) = self.to_ratio();
        write!(f, "{int_part}.{:03}", frac.abs())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inteiros_sao_exatos() {
        assert_eq!(Fixed::from_int(3).raw_milli(), 3_000);
        assert_eq!(Fixed::from_int(-2).raw_milli(), -2_000);
    }

    #[test]
    fn from_ratio_arredonda_deterministicamente() {
        assert_eq!(Fixed::from_ratio(1, 3).raw_milli(), 333);
        assert_eq!(Fixed::from_ratio(2, 3).raw_milli(), 667);
        assert_eq!(Fixed::from_ratio(-1, 3).raw_milli(), -333);
    }

    #[test]
    fn soma_e_subtracao() {
        let a = Fixed::from_ratio(1, 2); // 0.500
        let b = Fixed::from_ratio(1, 4); // 0.250
        assert_eq!((a + b).raw_milli(), 750);
        assert_eq!((a - b).raw_milli(), 250);
    }

    #[test]
    fn multiplicacao_preserva_escala() {
        let a = Fixed::from_int(2);
        let b = Fixed::from_ratio(1, 2); // 0.5
        assert_eq!((a * b).raw_milli(), Fixed::from_int(1).raw_milli());
    }

    #[test]
    fn div_e_from_ratio_concordam_no_arredondamento() {
        // Regressão: antes de `round_div_i64`, `Div` truncava enquanto
        // `from_ratio` arredondava — o mesmo valor matemático (2/3) tinha
        // dois `raw_milli` diferentes (666 via Div, 667 via from_ratio)
        // dependendo de como foi construído. As duas formas de chegar ao
        // mesmo racional precisam produzir o mesmo `Fixed`.
        let via_div = Fixed::from_int(2) / Fixed::from_int(3);
        let via_ratio = Fixed::from_ratio(2, 3);
        assert_eq!(via_div, via_ratio);
        assert_eq!(via_div.raw_milli(), 667);
    }

    #[test]
    fn divisao_de_negativo_por_negativo_e_exata() {
        // Regressão: `round_div_i64` decidia o sinal do ajuste de
        // arredondamento olhando só para `num`, sem normalizar `den`
        // primeiro. Para numerador E denominador negativos (quociente
        // positivo), isso descontava `half` do lado errado e devolvia 999
        // em vez de 1000 para uma divisão exata (-1/-1 = 1). Achado por
        // `proptests::div_e_from_ratio_concordam_para_pares_arbitrarios`.
        assert_eq!(
            (Fixed::from_int(-1) / Fixed::from_int(-1)).raw_milli(),
            1000
        );
        assert_eq!(Fixed::from_ratio(-1, -1).raw_milli(), 1000);
        assert_eq!(
            (Fixed::from_int(-4) / Fixed::from_int(-3)).raw_milli(),
            Fixed::from_ratio(-4, -3).raw_milli(),
        );
    }

    #[test]
    fn divisao_preserva_escala() {
        let a = Fixed::from_int(1);
        let b = Fixed::from_int(4);
        assert_eq!((a / b).raw_milli(), Fixed::from_ratio(1, 4).raw_milli());
    }

    #[test]
    fn clamp_respeita_limites() {
        let v = Fixed::from_int(150).clamp(Fixed::from_int(0), Fixed::from_int(100));
        assert_eq!(v, 100);
    }

    #[test]
    fn display_formata_como_decimal() {
        assert_eq!(Fixed::from_ratio(1, 4).to_string(), "0.250");
        assert_eq!(Fixed::from_int(-2).to_string(), "-2.000");
    }

    #[test]
    fn mesma_entrada_produz_sempre_o_mesmo_bit_pattern() {
        // O ponto inteiro do determinismo: repetir o mesmo cálculo N vezes
        // (simulando "rodar em outra arquitetura") tem que bater byte a byte.
        let calc = || Fixed::from_ratio(22, 7) * Fixed::from_int(3) - Fixed::from_ratio(1, 6);
        let runs: Vec<i32> = (0..1000).map(|_| calc().raw_milli()).collect();
        assert!(runs.windows(2).all(|w| w[0] == w[1]));
    }
}

/// Testes de propriedade — `docs/08 §1`. `div_e_from_ratio_concordam` é a
/// generalização do bug de arredondamento corrigido nesta revisão: o teste
/// de regressão em `mod tests` prova que o par `(2, 3)` ficou consistente;
/// este prova que **todo** par numerador/denominador nas faixas testadas
/// concorda, para que essa classe de bug não volte por outra combinação.
#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn from_int_e_trunc_sao_inversas(n in -1_000_000i32..1_000_000i32) {
            prop_assert_eq!(Fixed::from_int(n).trunc(), n);
        }

        #[test]
        fn div_e_from_ratio_concordam_para_pares_arbitrarios(
            num in -1_000i32..1_000i32,
            den in prop_oneof![1i32..1_000i32, -999i32..0i32],
        ) {
            let via_div = Fixed::from_int(num) / Fixed::from_int(den);
            let via_ratio = Fixed::from_ratio(i64::from(num), i64::from(den));
            prop_assert_eq!(via_div, via_ratio);
        }

        #[test]
        fn abs_nunca_e_negativo(raw in (i32::MIN / 2)..(i32::MAX / 2)) {
            prop_assert!(Fixed::from_milli(raw).abs().raw_milli() >= 0);
        }

        #[test]
        fn clamp_fica_dentro_dos_limites(v in -100_000i32..100_000i32, lo in -1_000i32..0i32, hi in 0i32..1_000i32) {
            let clamped = Fixed::from_int(v).clamp(Fixed::from_int(lo), Fixed::from_int(hi));
            prop_assert!(clamped >= lo && clamped <= hi);
        }

        /// Soma e subtração são exatas (sem arredondamento envolvido), então
        /// precisam formar par perfeito: `(a + b) - b == a`, para qualquer
        /// `a`/`b` que não estoure `i32` na soma intermediária.
        #[test]
        fn soma_e_subtracao_sao_inversas(a in -500_000i32..500_000i32, b in -500_000i32..500_000i32) {
            let a = Fixed::from_milli(a);
            let b = Fixed::from_milli(b);
            prop_assert_eq!((a + b) - b, a);
        }
    }
}
