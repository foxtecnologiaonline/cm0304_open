//! Treino básico (RF-TC-01, `docs/01 §2.6`): uma **intensidade** por
//! jogador do clube sob comando do técnico humano, com efeito mensurável e
//! um custo real — sem custo, "intenso" seria a única escolha racional.
//!
//! | Intensidade | Evolução de CA na temporada | Propensão a lesão | Condição de entrada |
//! |---|---|---|---|
//! | `Light`  | -1 até 28 anos | -8  | +5 |
//! | `Normal` | (curva de idade/minutos/profissionalismo, sem ajuste) | 0 | 0 |
//! | `Intense`| +2 até 23, +1 até 28 | +12 | -5 |
//!
//! O ajuste de CA soma à progressão normal de `crate::progression` (que
//! continua sendo a fonte principal) depois que o jogador envelheceu um ano
//! (as faixas de idade usam a idade nova); o limite de `PA` continua valendo
//! (`domain::Ability::adjust_current`). Os atributos visíveis (36) ainda não
//! evoluem na carreira (`crate::quality`): o treino mexe no CA — que dá a
//! força do time — não em atributos individuais, e foco por atributo
//! (RF-TC-01 completo, RF-TC-05) fica para o M3/M4. Só jogadores do clube
//! sob comando são afetados; a IA treina sempre em `Normal`.
//! Nada calibrado (`docs/04 §4`).

use std::borrow::Cow;

use domain::ClubId;

use crate::condition::FULL_CONDITION;
use crate::progression::PlayerState;

/// Intensidade de treino de um jogador.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Training {
    Light,
    #[default]
    Normal,
    Intense,
}

impl Training {
    /// Código estável de 1 byte (log de comandos): **nunca renumerar**.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Training::Light => 0,
            Training::Normal => 1,
            Training::Intense => 2,
        }
    }

    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Training::Light),
            1 => Some(Training::Normal),
            2 => Some(Training::Intense),
            _ => None,
        }
    }
}

/// Ajuste de CA da temporada para um jogador de `age_years` (já
/// envelhecido).
#[must_use]
pub fn ca_bonus(training: Training, age_years: i32) -> i16 {
    match training {
        Training::Normal => 0,
        Training::Intense => match age_years {
            ..=23 => 2,
            24..=28 => 1,
            _ => 0,
        },
        Training::Light => {
            if age_years <= 28 {
                -1
            } else {
                0
            }
        }
    }
}

/// Ajuste da propensão a lesão (0-100) usada no sorteio da temporada.
#[must_use]
pub fn injury_proneness_delta(training: Training) -> i16 {
    match training {
        Training::Light => -8,
        Training::Normal => 0,
        Training::Intense => 12,
    }
}

/// Ajuste da condição de entrada na temporada seguinte.
#[must_use]
pub fn condition_delta(training: Training) -> i16 {
    match training {
        Training::Light => 5,
        Training::Normal => 0,
        Training::Intense => -5,
    }
}

/// Piso da condição depois do ajuste de treino — mesmo espírito do piso de
/// `crate::condition`: a pré-temporada recupera a maior parte do desgaste.
const MIN_CONDITION: i16 = 50;

fn affected<'a>(
    roster: &'a [PlayerState],
    training: &'a [Training],
    managed: ClubId,
) -> impl Iterator<Item = (usize, Training)> + 'a {
    roster
        .iter()
        .zip(training)
        .enumerate()
        .filter(move |(_, (p, t))| p.club == managed && **t != Training::Normal)
        .map(|(i, (_, &t))| (i, t))
}

/// Soma o bônus de CA do treino aos jogadores do clube `managed`. Chamar
/// **depois** de `crate::progression::advance_season`.
pub fn apply_training_growth(roster: &mut [PlayerState], training: &[Training], managed: ClubId) {
    let changes: Vec<(usize, i16)> = affected(roster, training, managed)
        .map(|(i, t)| (i, ca_bonus(t, roster[i].age_years)))
        .collect();
    for (i, bonus) in changes {
        roster[i].ability = roster[i].ability.adjust_current(bonus);
    }
}

/// Aplica o custo/alívio de condição do treino. Chamar **depois** de
/// `crate::condition::apply_season_fatigue`.
pub fn apply_training_condition(
    roster: &mut [PlayerState],
    training: &[Training],
    managed: ClubId,
) {
    let changes: Vec<(usize, Training)> = affected(roster, training, managed).collect();
    for (i, t) in changes {
        let now = i16::from(roster[i].condition) + condition_delta(t);
        roster[i].condition = now.clamp(MIN_CONDITION, i16::from(FULL_CONDITION)) as u8;
    }
}

/// Propensão a lesão com o treino aplicado, para o sorteio da temporada.
/// Sem nenhum treino fora do `Normal` devolve `base` sem copiar.
#[must_use]
pub fn adjusted_proneness<'a>(
    roster: &[PlayerState],
    base: &'a [u8],
    training: &[Training],
    managed: Option<ClubId>,
) -> Cow<'a, [u8]> {
    let Some(managed) = managed else {
        return Cow::Borrowed(base);
    };
    let changes: Vec<(usize, Training)> = affected(roster, training, managed).collect();
    if changes.is_empty() {
        return Cow::Borrowed(base);
    }
    let mut adjusted = base.to_vec();
    for (i, t) in changes {
        let v = i16::from(adjusted[i]) + injury_proneness_delta(t);
        adjusted[i] = v.clamp(0, 100) as u8;
    }
    Cow::Owned(adjusted)
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{Ability, PlayerId, Position};

    fn player(id: u32, club: u32, age: i32, ca: u8, pa: u8) -> PlayerState {
        PlayerState {
            player: PlayerId::new(id),
            club: ClubId::new(club),
            position: Position::Midfielder,
            age_years: age,
            ability: Ability::new(ca, pa),
            injured: false,
            condition: 80,
        }
    }

    #[test]
    fn treino_intenso_ajuda_mais_o_jovem_e_o_leve_so_atrasa_quem_ainda_evolui() {
        assert_eq!(ca_bonus(Training::Intense, 19), 2);
        assert_eq!(ca_bonus(Training::Intense, 26), 1);
        assert_eq!(ca_bonus(Training::Intense, 31), 0);
        assert_eq!(ca_bonus(Training::Normal, 19), 0);
        assert_eq!(ca_bonus(Training::Light, 19), -1);
        assert_eq!(ca_bonus(Training::Light, 31), 0);
    }

    #[test]
    fn todo_ganho_do_treino_intenso_tem_custo_de_lesao_e_condicao() {
        assert!(injury_proneness_delta(Training::Intense) > 0);
        assert!(condition_delta(Training::Intense) < 0);
        assert!(injury_proneness_delta(Training::Light) < 0);
        assert!(condition_delta(Training::Light) > 0);
        assert_eq!(injury_proneness_delta(Training::Normal), 0);
        assert_eq!(condition_delta(Training::Normal), 0);
    }

    #[test]
    fn crescimento_so_afeta_o_clube_do_tecnico_e_respeita_o_potencial() {
        let mut roster = vec![
            player(0, 3, 20, 100, 150),
            player(1, 3, 20, 100, 100), // já no teto
            player(2, 4, 20, 100, 150), // outro clube
        ];
        let training = vec![Training::Intense; 3];
        apply_training_growth(&mut roster, &training, ClubId::new(3));
        assert_eq!(roster[0].ability.current(), 102);
        assert_eq!(roster[1].ability.current(), 100, "PA continua sendo o teto");
        assert_eq!(roster[2].ability.current(), 100, "IA treina Normal");
    }

    #[test]
    fn condicao_fica_entre_o_piso_e_o_maximo() {
        let mut roster = vec![player(0, 3, 25, 100, 100), player(1, 3, 25, 100, 100)];
        roster[0].condition = 52;
        roster[1].condition = 98;
        apply_training_condition(
            &mut roster,
            &[Training::Intense, Training::Light],
            ClubId::new(3),
        );
        assert_eq!(roster[0].condition, MIN_CONDITION as u8);
        assert_eq!(roster[1].condition, FULL_CONDITION);
    }

    #[test]
    fn propensao_so_copia_quando_ha_treino_fora_do_normal() {
        let roster = vec![player(0, 3, 25, 100, 100), player(1, 3, 25, 100, 100)];
        let base = vec![50u8, 3];
        let all_normal = vec![Training::Normal; 2];
        assert!(matches!(
            adjusted_proneness(&roster, &base, &all_normal, Some(ClubId::new(3))),
            Cow::Borrowed(_)
        ));
        assert!(matches!(
            adjusted_proneness(&roster, &base, &[Training::Intense; 2], None),
            Cow::Borrowed(_)
        ));
        let adjusted = adjusted_proneness(
            &roster,
            &base,
            &[Training::Intense, Training::Light],
            Some(ClubId::new(3)),
        );
        assert_eq!(
            adjusted.as_ref(),
            &[62, 0],
            "intenso +12; leve -8 com piso em 0"
        );
        assert_eq!(base, vec![50, 3], "a base nunca é alterada");
    }

    #[test]
    fn codigos_de_treino_fazem_round_trip() {
        for t in [Training::Light, Training::Normal, Training::Intense] {
            assert_eq!(Training::from_code(t.code()), Some(t));
        }
        assert_eq!(Training::from_code(7), None);
        assert_eq!(Training::default(), Training::Normal);
    }
}
