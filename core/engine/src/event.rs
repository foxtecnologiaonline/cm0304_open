//! `MatchEvent` — o contrato de saída do motor (`docs/04-motor-de-partida.md §5`).
//!
//! O contrato final tem `Shot`, `Foul`, `Injury`, `Sub`, `Positions`, com
//! `PlayerId`, `Zone`, `ShotOutcome` etc. — nenhum desses tipos existe ainda,
//! porque dependem de jogadores em campo, zonas espaciais e táticas, que só
//! chegam com o motor v2 (`docs/07-roadmap.md#m3--alpha-14-semanas`). Este
//! módulo define o **subconjunto real do v0**: só o que a camada estatística
//! de fato produz. Adicionar uma variante nova aqui é sempre compatível
//! (nenhum código existente depende de `MatchEvent` ser exaustivo fora deste
//! crate) — é assim que o contrato "nasce completo aos poucos" sem inventar
//! campos que não têm como ser preenchidos ainda.

/// Lado da partida — mandante ou visitante.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Home,
    Away,
}

/// Um evento da partida, na ordem em que aconteceu.
///
/// `minute` é `1..=90` (sem acréscimos no v0 — chegam com o motor v1/v2,
/// junto de cartões e lesões, que são o que hoje estende o tempo de jogo).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchEvent {
    /// Início da partida.
    KickOff,
    /// Um gol, sem autor — jogadores entram no v1 (`docs/07` M2), quando o
    /// motor passa a receber uma escalação de verdade, não só duas forças.
    Goal { minute: u8, side: Side },
    /// Fim de jogo, com o placar final. `stats`/`ratings` do contrato
    /// completo (`docs/04 §5`) chegam com finalizações, posse e notas — v1/v2.
    FullTime { home_goals: u32, away_goals: u32 },
}

/// Conta o placar percorrendo os eventos — a fonte de verdade é sempre a
/// lista de eventos, nunca um campo solto que possa dessincronizar dela
/// (`docs/02 §5`, mesmo princípio de "verdade única" aplicado aqui).
#[must_use]
pub fn score(events: &[MatchEvent]) -> (u32, u32) {
    events
        .iter()
        .fold((0, 0), |(home, away), event| match event {
            MatchEvent::Goal {
                side: Side::Home, ..
            } => (home + 1, away),
            MatchEvent::Goal {
                side: Side::Away, ..
            } => (home, away + 1),
            _ => (home, away),
        })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn score_conta_gols_por_lado() {
        let events = [
            MatchEvent::KickOff,
            MatchEvent::Goal {
                minute: 10,
                side: Side::Home,
            },
            MatchEvent::Goal {
                minute: 45,
                side: Side::Away,
            },
            MatchEvent::Goal {
                minute: 80,
                side: Side::Home,
            },
            MatchEvent::FullTime {
                home_goals: 2,
                away_goals: 1,
            },
        ];
        assert_eq!(score(&events), (2, 1));
    }

    #[test]
    fn score_de_lista_vazia_e_zero_a_zero() {
        assert_eq!(score(&[]), (0, 0));
    }
}
