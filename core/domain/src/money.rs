//! Dinheiro — `docs/03-modelo-de-dados.md §4`: "`i64` em **centavos** da
//! moeda base; câmbio por data". Câmbio entre moedas não existe ainda (só
//! há uma moeda base fictícia por pack); este tipo só garante a escala
//! certa e aritmética inteira seguro contra estouro.

use std::fmt;
use std::ops::{Add, Sub};

/// Quantia em centavos da moeda base de um pack. Sempre `i64`, nunca ponto
/// flutuante (`ADR 0002`) — o mesmo motivo de `Fixed`, mas sem escala
/// fracionária: dinheiro já é um inteiro na sua própria unidade mínima.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Default)]
pub struct Money(i64);

impl Money {
    pub const ZERO: Money = Money(0);

    #[must_use]
    pub const fn from_cents(cents: i64) -> Self {
        Self(cents)
    }

    #[must_use]
    pub const fn cents(self) -> i64 {
        self.0
    }

    /// Soma saturando em vez de estourar — um pack/mercado com quantias
    /// absurdas não deveria travar o núcleo (`docs/02 §9.2`: núcleo nunca
    /// panica com dado externo), só parar de crescer no teto de `i64`.
    #[must_use]
    pub fn saturating_add(self, other: Money) -> Money {
        Money(self.0.saturating_add(other.0))
    }

    /// Subtração saturando no piso `0` — dinheiro nunca fica negativo aqui;
    /// quem chama decide separadamente se uma compra cabe no orçamento
    /// (`ai::market`), este tipo só garante que o resultado nunca é um
    /// valor "negativo" sem sentido para um saldo.
    #[must_use]
    pub fn saturating_sub(self, other: Money) -> Money {
        Money(self.0.saturating_sub(other.0).max(0))
    }
}

impl Add for Money {
    type Output = Money;
    fn add(self, rhs: Money) -> Money {
        self.saturating_add(rhs)
    }
}

impl Sub for Money {
    type Output = Money;
    fn sub(self, rhs: Money) -> Money {
        self.saturating_sub(rhs)
    }
}

impl fmt::Display for Money {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (sign, cents) = if self.0 < 0 {
            ("-", -self.0)
        } else {
            ("", self.0)
        };
        write!(f, "{sign}{}.{:02}", cents / 100, cents % 100)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn soma_e_subtracao_exatas() {
        let a = Money::from_cents(1000);
        let b = Money::from_cents(300);
        assert_eq!((a + b).cents(), 1300);
        assert_eq!((a - b).cents(), 700);
    }

    #[test]
    fn subtracao_nunca_fica_negativa() {
        let a = Money::from_cents(100);
        let b = Money::from_cents(500);
        assert_eq!((a - b).cents(), 0);
    }

    #[test]
    fn soma_satura_em_vez_de_estourar() {
        let a = Money::from_cents(i64::MAX - 1);
        let b = Money::from_cents(100);
        assert_eq!((a + b).cents(), i64::MAX);
    }

    #[test]
    fn display_formata_como_decimal() {
        assert_eq!(Money::from_cents(150_075).to_string(), "1500.75");
        assert_eq!(Money::from_cents(5).to_string(), "0.05");
        assert_eq!(Money::from_cents(0).to_string(), "0.00");
    }
}

/// Testes de propriedade (`docs/08 §1`).
#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// Subtração nunca produz um valor negativo, para quaisquer dois
        /// valores não-negativos.
        #[test]
        fn subtracao_nunca_e_negativa(a in 0i64..i64::MAX, b in 0i64..i64::MAX) {
            let result = Money::from_cents(a) - Money::from_cents(b);
            prop_assert!(result.cents() >= 0);
        }

        /// `Money::add` satura exatamente como `i64::saturating_add` — nunca
        /// um wraparound silencioso (o que aconteceria com `+` comum em
        /// release, sem `overflow-checks`, para qualquer par de `i64`).
        #[test]
        fn soma_satura_igual_a_i64_saturating_add(a: i64, b: i64) {
            let result = Money::from_cents(a) + Money::from_cents(b);
            prop_assert_eq!(result.cents(), a.saturating_add(b));
        }
    }
}
