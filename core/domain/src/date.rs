//! `GameDate` — calendário do jogo, inteiro puro do início ao fim.
//!
//! Nunca lê o relógio do sistema nem o locale (`ADR 0002`, `docs/02 §5`): toda
//! data do jogo nasce do calendário simulado, e toda formatação para humanos
//! é responsabilidade da camada de apresentação, não deste tipo.
//!
//! A conversão calendário↔dias usa o algoritmo de Howard Hinnant para o
//! calendário gregoriano proléptico (domínio público, amplamente auditado),
//! reescrito aqui em aritmética inteira — sem ponto flutuante, sem
//! dependência de biblioteca de data do sistema operacional.

use std::fmt;
use std::ops::{Add, Sub};

/// Data do jogo: dias desde 1970-01-01 (pode ser negativo — datas antes de 1970
/// são válidas, o calendário é proléptico em ambas as direções).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GameDate {
    days_since_epoch: i32,
}

/// Dia da semana, `Sunday = 0` (convenção ISO-C, não ISO-8601).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum Weekday {
    Sunday = 0,
    Monday = 1,
    Tuesday = 2,
    Wednesday = 3,
    Thursday = 4,
    Friday = 5,
    Saturday = 6,
}

impl GameDate {
    /// Constrói a partir de ano/mês/dia do calendário gregoriano proléptico.
    ///
    /// Não valida se `day` é válido para `month` (ex.: 31 de fevereiro) —
    /// isso é responsabilidade do carregador de data pack, que deve rejeitar
    /// datas inválidas antes de chegar aqui (`docs/03 §7`, validador de pack).
    #[must_use]
    pub fn from_ymd(year: i32, month: u32, day: u32) -> Self {
        Self {
            days_since_epoch: days_from_civil(year, month, day),
        }
    }

    /// Constrói a partir do offset bruto em dias desde a época (uso interno/serialização).
    #[must_use]
    pub const fn from_epoch_days(days: i32) -> Self {
        Self {
            days_since_epoch: days,
        }
    }

    #[must_use]
    pub const fn epoch_days(self) -> i32 {
        self.days_since_epoch
    }

    /// Decompõe em `(ano, mês 1..=12, dia 1..=31)`.
    #[must_use]
    pub fn to_ymd(self) -> (i32, u32, u32) {
        civil_from_days(self.days_since_epoch)
    }

    #[must_use]
    pub fn year(self) -> i32 {
        self.to_ymd().0
    }

    #[must_use]
    pub fn month(self) -> u32 {
        self.to_ymd().1
    }

    #[must_use]
    pub fn day(self) -> u32 {
        self.to_ymd().2
    }

    #[must_use]
    pub fn weekday(self) -> Weekday {
        weekday_from_days(self.days_since_epoch)
    }

    #[must_use]
    pub fn add_days(self, n: i32) -> Self {
        Self {
            days_since_epoch: self.days_since_epoch + n,
        }
    }

    /// Diferença, em dias, entre `self` e `other` (`self - other`).
    #[must_use]
    pub fn days_since(self, other: GameDate) -> i32 {
        self.days_since_epoch - other.days_since_epoch
    }
}

impl Add<i32> for GameDate {
    type Output = GameDate;
    fn add(self, days: i32) -> Self::Output {
        self.add_days(days)
    }
}

impl Sub<i32> for GameDate {
    type Output = GameDate;
    fn sub(self, days: i32) -> Self::Output {
        self.add_days(-days)
    }
}

impl fmt::Display for GameDate {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (y, m, d) = self.to_ymd();
        write!(f, "{y:04}-{m:02}-{d:02}")
    }
}

/// Dias desde 1970-01-01 — algoritmo de Hinnant, calendário gregoriano proléptico.
fn days_from_civil(year: i32, month: u32, day: u32) -> i32 {
    let y = i64::from(year) - i64::from(month <= 2);
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400; // [0, 399]
    let mp = i64::from(month) + if month > 2 { -3 } else { 9 };
    let doy = (153 * mp + 2) / 5 + i64::from(day) - 1; // [0, 365]
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy; // [0, 146096]
    (era * 146_097 + doe - 719_468) as i32
}

/// Inverso de `days_from_civil`.
fn civil_from_days(days: i32) -> (i32, u32, u32) {
    let z = i64::from(days) + 719_468;
    let era = if z >= 0 { z } else { z - 146_096 } / 146_097;
    let doe = z - era * 146_097; // [0, 146096]
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365; // [0, 399]
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100); // [0, 365]
    let mp = (5 * doy + 2) / 153; // [0, 11]
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32; // [1, 31]
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32; // [1, 12]
    let year = if m <= 2 { y + 1 } else { y };
    (year as i32, m, d)
}

fn weekday_from_days(days: i32) -> Weekday {
    let z = i64::from(days);
    let idx = if z >= -4 {
        (z + 4) % 7
    } else {
        (z + 5) % 7 + 6
    };
    match idx {
        0 => Weekday::Sunday,
        1 => Weekday::Monday,
        2 => Weekday::Tuesday,
        3 => Weekday::Wednesday,
        4 => Weekday::Thursday,
        5 => Weekday::Friday,
        _ => Weekday::Saturday,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn epoch_e_dia_zero() {
        assert_eq!(GameDate::from_ymd(1970, 1, 1).epoch_days(), 0);
    }

    #[test]
    fn round_trip_ymd() {
        for (y, m, d) in [(2003, 8, 9), (2028, 2, 29), (1900, 1, 1), (1899, 12, 31)] {
            let date = GameDate::from_ymd(y, m, d);
            assert_eq!(
                date.to_ymd(),
                (y, m, d),
                "round-trip falhou para {y}-{m}-{d}"
            );
        }
    }

    #[test]
    fn datas_antes_da_epoca_sao_validas() {
        let date = GameDate::from_ymd(1950, 6, 15);
        assert!(date.epoch_days() < 0);
        assert_eq!(date.to_ymd(), (1950, 6, 15));
    }

    #[test]
    fn soma_de_dias_atravessa_mes_e_ano() {
        let new_year_eve = GameDate::from_ymd(2026, 12, 31);
        assert_eq!(new_year_eve.add_days(1).to_ymd(), (2027, 1, 1));

        let end_of_feb_leap = GameDate::from_ymd(2028, 2, 28);
        assert_eq!(end_of_feb_leap.add_days(1).to_ymd(), (2028, 2, 29)); // 2028 é bissexto
    }

    #[test]
    fn dia_da_semana_bate_com_data_conhecida() {
        // 2003-08-09 (lançamento histórico de referência do gênero) foi um sábado.
        assert_eq!(GameDate::from_ymd(2003, 8, 9).weekday(), Weekday::Saturday);
        // 1970-01-01 foi uma quinta-feira.
        assert_eq!(GameDate::from_ymd(1970, 1, 1).weekday(), Weekday::Thursday);
        // 2000-01-01 foi um sábado.
        assert_eq!(GameDate::from_ymd(2000, 1, 1).weekday(), Weekday::Saturday);
    }

    #[test]
    fn days_since_e_a_diferenca_correta() {
        let a = GameDate::from_ymd(2026, 1, 1);
        let b = GameDate::from_ymd(2026, 4, 10);
        assert_eq!(b.days_since(a), 99);
        assert_eq!(a.days_since(b), -99);
    }

    #[test]
    fn display_formata_iso() {
        assert_eq!(GameDate::from_ymd(2026, 4, 5).to_string(), "2026-04-05");
    }
}
