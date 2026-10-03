//! Condição física — fatia mínima de RF-JG-07 (`docs/01-requisitos.md §2.3`:
//! "Moral, condição física, fadiga acumulada e frescor de jogo").
//!
//! A versão completa pede frescor dia a dia (dias de descanso reais entre
//! partidas) — não dá para calcular sem um calendário diário ligado a
//! `world`, que não existe ainda (`docs/02-arquitetura.md §5`, a mesma
//! lacuna que já limita `crate::progression` a "uma temporada = um ano").
//! O que existe aqui, em vez disso: condição **por temporada**, recalculada
//! do zero a cada uma a partir de **quantas partidas de liga o jogador
//! titularizou na temporada que acabou de terminar** — não cumulativo entre
//! temporadas (um jogador não "carrega" desgaste de carreira, só da última
//! temporada), porque sem granularidade de dias não há como distinguir
//! "jogou muito mas descansou bem entre partidas" de "jogou muito e nunca
//! descansou" — a pré-temporada inteira (meses, no mundo real) já reseta a
//! maior parte da fadiga física mesmo assim. Mesma disciplina de
//! `crate::injuries`: uma fatia pequena e honesta, não uma simulação de
//! "condição" fingida.
//!
//! A escalação em si (`ai::select_starting_eleven`) continua ignorando
//! condição — ainda escolhe só por CA (RF-PA-01 completo, com rotação de
//! elenco por cansaço, é M3). O que a condição afeta é a força efetiva do
//! titular na temporada seguinte (`crate::quality::build_profile`): um
//! jogador que titularizou a temporada inteira sem alternância rende um
//! pouco menos no começo da próxima que um reserva que descansou.

use std::collections::{HashMap, HashSet};

use domain::{ClubId, PlayerId};

use crate::progression::PlayerState;

/// Condição máxima — jogador plenamente descansado (nunca titularizou na
/// temporada anterior, ou está entrando na carreira agora).
pub const FULL_CONDITION: u8 = 100;

/// Quanto cada partida de liga titularizada desgasta a condição de entrada
/// na temporada seguinte.
const FATIGUE_PER_MATCH: u8 = 2;

/// Piso de condição de início de temporada — mesmo um titular inabalável a
/// temporada inteira nunca começa a próxima abaixo disto. Representa a
/// pré-temporada: meses de preparação física sempre recuperam a maior
/// parte do desgaste, mesmo para quem não descansou nada.
const MIN_SEASON_START_CONDITION: u8 = 70;

/// Condição de início de temporada para um jogador que titularizou
/// `matches_started` partidas de liga na temporada anterior — pura e sem
/// estado: não depende da condição de entrada da temporada anterior, só da
/// carga de jogos (ver doc do módulo, "não cumulativo").
#[must_use]
fn condition_for_next_season(matches_started: u32) -> u8 {
    let fatigue = u32::from(FATIGUE_PER_MATCH).saturating_mul(matches_started);
    FULL_CONDITION
        .saturating_sub(u8::try_from(fatigue).unwrap_or(u8::MAX))
        .max(MIN_SEASON_START_CONDITION)
}

/// Aplica a condição de início da próxima temporada a todo `roster`:
/// `FULL_CONDITION` para quem não está em `starters`, [`condition_for_next_season`]
/// (a partir de `matches_played_by_club[roster[i].club]`) para quem está.
/// Partidas de copa (`crate::cup`) não entram em `matches_played_by_club`
/// — só partidas de liga desgastam condição nesta fatia mínima (mesma
/// exclusão documentada em `crate::season::SeasonResult::cups`: copa fica
/// de fora das contagens que afetam qualquer outro subsistema).
pub fn apply_season_fatigue(
    roster: &mut [PlayerState],
    starters: &HashSet<PlayerId>,
    matches_played_by_club: &HashMap<ClubId, u32>,
) {
    for player in roster.iter_mut() {
        player.condition = if starters.contains(&player.player) {
            let matches = matches_played_by_club
                .get(&player.club)
                .copied()
                .unwrap_or(0);
            condition_for_next_season(matches)
        } else {
            FULL_CONDITION
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zero_partidas_da_condicao_plena() {
        assert_eq!(condition_for_next_season(0), FULL_CONDITION);
    }

    #[test]
    fn muitas_partidas_nunca_passa_do_piso() {
        assert_eq!(condition_for_next_season(14), FULL_CONDITION - 14 * 2);
        assert_eq!(condition_for_next_season(1000), MIN_SEASON_START_CONDITION);
    }

    #[test]
    fn condicao_cresce_monotonicamente_com_menos_partidas() {
        for n in 1..30u32 {
            assert!(condition_for_next_season(n - 1) >= condition_for_next_season(n));
        }
    }

    #[test]
    fn apply_season_fatigue_poupa_quem_nao_foi_titular() {
        let mut roster = vec![PlayerState {
            player: PlayerId::new(0),
            club: ClubId::new(0),
            position: domain::Position::Midfielder,
            age_years: 25,
            ability: domain::Ability::new(100, 150),
            injured: false,
            condition: 40, // começa desgastado, de propósito
        }];
        let starters = HashSet::new(); // ninguém titularizou
        let matches = HashMap::from([(ClubId::new(0), 14)]);
        apply_season_fatigue(&mut roster, &starters, &matches);
        assert_eq!(
            roster[0].condition, FULL_CONDITION,
            "reserva deveria entrar na próxima temporada plenamente descansado"
        );
    }

    #[test]
    fn apply_season_fatigue_desgasta_titular_proporcional_as_partidas_do_clube() {
        let mut roster = vec![PlayerState {
            player: PlayerId::new(0),
            club: ClubId::new(0),
            position: domain::Position::Midfielder,
            age_years: 25,
            ability: domain::Ability::new(100, 150),
            injured: false,
            condition: FULL_CONDITION,
        }];
        let starters = HashSet::from([PlayerId::new(0)]);
        let matches = HashMap::from([(ClubId::new(0), 14)]);
        apply_season_fatigue(&mut roster, &starters, &matches);
        assert_eq!(roster[0].condition, condition_for_next_season(14));
        assert!(roster[0].condition < FULL_CONDITION);
    }

    #[test]
    fn clube_sem_entrada_no_mapa_de_partidas_nao_desgasta() {
        // Defensivo: nunca deveria acontecer com um `SeasonResult` real
        // (todo clube de liga aparece em alguma tabela), mas não é um
        // `panic!` se acontecer — `docs/02 §9.2`.
        let mut roster = vec![PlayerState {
            player: PlayerId::new(0),
            club: ClubId::new(0),
            position: domain::Position::Midfielder,
            age_years: 25,
            ability: domain::Ability::new(100, 150),
            injured: false,
            condition: FULL_CONDITION,
        }];
        let starters = HashSet::from([PlayerId::new(0)]);
        apply_season_fatigue(&mut roster, &starters, &HashMap::new());
        assert_eq!(roster[0].condition, FULL_CONDITION);
    }
}

/// Testes de propriedade (`docs/08 §1`).
#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn condition_for_next_season_sempre_dentro_dos_limites(matches in 0u32..1000) {
            let c = condition_for_next_season(matches);
            prop_assert!((MIN_SEASON_START_CONDITION..=FULL_CONDITION).contains(&c));
        }

        #[test]
        fn condition_for_next_season_e_deterministico(matches in 0u32..1000) {
            prop_assert_eq!(condition_for_next_season(matches), condition_for_next_season(matches));
        }
    }
}
