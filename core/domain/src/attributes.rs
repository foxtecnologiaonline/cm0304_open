//! Atributos de jogador — escala 1–20 visível, CA/PA 0–200 oculta.
//!
//! Espelha `docs/03-modelo-de-dados.md §3` e `§5`. O peso de cada atributo no
//! cálculo de CA por posição **não** mora aqui — mora em dado
//! (`packs/core/ability.toml`, RNF-23); este módulo só garante que os valores
//! nunca saiam dos limites válidos, para qualquer origem (pack, geração
//! procedural, progressão de carreira).

/// Quantidade de atributos técnicos, mentais e físicos visíveis (exclui goleiro).
pub const N_ATTR_OUTFIELD: usize = 32;
/// Atributos específicos de goleiro — só relevantes com familiaridade em GOL ≥ 10.
pub const N_ATTR_GOALKEEPING: usize = 4;
/// Total de atributos visíveis por jogador (`docs/03 §3.1`).
pub const N_ATTR: usize = N_ATTR_OUTFIELD + N_ATTR_GOALKEEPING;
/// Atributos ocultos, nunca expostos numericamente na UI (`docs/03 §3.2`).
pub const N_HIDDEN: usize = 10;

/// Limite inferior/superior de um atributo visível.
pub const ATTR_MIN: u8 = 1;
pub const ATTR_MAX: u8 = 20;

/// Os 36 atributos visíveis, na ordem em que aparecem em `docs/03 §3.1`.
/// A variante determina o índice em `PlayerAttributes` — a ordem é estável
/// e faz parte do formato de save; **não reordenar**, só adicionar ao final
/// antes de `N_ATTR` seria uma mudança de schema (`docs/02 §8.3`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
#[repr(u8)]
pub enum Attribute {
    // -- Técnicos (14) --
    Finishing = 0,
    Heading,
    Passing,
    Crossing,
    Dribbling,
    FirstTouch,
    Tackling,
    Marking,
    LongShots,
    SetPieces,
    Penalties,
    LongThrows,
    Dominance,
    Technique,
    // -- Mentais (12) --
    Vision,
    Decisions,
    Positioning,
    Anticipation,
    Concentration,
    Determination,
    Leadership,
    Teamwork,
    Aggression,
    Composure,
    Creativity,
    OffTheBall,
    // -- Físicos (6) --
    Pace,
    Acceleration,
    Stamina,
    Strength,
    Agility,
    Balance,
    // -- Goleiro (4) --
    Reflexes,
    Rushing,
    AerialAbility,
    Distribution,
}

impl Attribute {
    /// Todas as variantes, na ordem estável do array.
    pub const ALL: [Attribute; N_ATTR] = [
        Attribute::Finishing,
        Attribute::Heading,
        Attribute::Passing,
        Attribute::Crossing,
        Attribute::Dribbling,
        Attribute::FirstTouch,
        Attribute::Tackling,
        Attribute::Marking,
        Attribute::LongShots,
        Attribute::SetPieces,
        Attribute::Penalties,
        Attribute::LongThrows,
        Attribute::Dominance,
        Attribute::Technique,
        Attribute::Vision,
        Attribute::Decisions,
        Attribute::Positioning,
        Attribute::Anticipation,
        Attribute::Concentration,
        Attribute::Determination,
        Attribute::Leadership,
        Attribute::Teamwork,
        Attribute::Aggression,
        Attribute::Composure,
        Attribute::Creativity,
        Attribute::OffTheBall,
        Attribute::Pace,
        Attribute::Acceleration,
        Attribute::Stamina,
        Attribute::Strength,
        Attribute::Agility,
        Attribute::Balance,
        Attribute::Reflexes,
        Attribute::Rushing,
        Attribute::AerialAbility,
        Attribute::Distribution,
    ];

    #[must_use]
    pub const fn index(self) -> usize {
        self as usize
    }

    #[must_use]
    pub const fn is_goalkeeping(self) -> bool {
        matches!(
            self,
            Attribute::Reflexes
                | Attribute::Rushing
                | Attribute::AerialAbility
                | Attribute::Distribution
        )
    }
}

/// Os 36 atributos visíveis de um jogador, cada um em `1..=20`.
///
/// Internamente é um array denso (`docs/02 §6.1`, layout SoA-friendly): o
/// `Player` "de verdade" guarda um `[PlayerAttributes; N]` contíguo, não um
/// `HashMap<Attribute, u8>` por jogador.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerAttributes([u8; N_ATTR]);

impl Default for PlayerAttributes {
    fn default() -> Self {
        Self([ATTR_MIN; N_ATTR])
    }
}

impl PlayerAttributes {
    /// Constrói a partir de um array já preenchido, saturando cada valor em `1..=20`.
    #[must_use]
    pub fn from_raw(raw: [u8; N_ATTR]) -> Self {
        let mut clamped = raw;
        for v in &mut clamped {
            *v = (*v).clamp(ATTR_MIN, ATTR_MAX);
        }
        Self(clamped)
    }

    #[must_use]
    pub fn get(&self, attr: Attribute) -> u8 {
        self.0[attr.index()]
    }

    /// Define o valor, saturando em `1..=20` — nunca produz um atributo fora de faixa.
    pub fn set(&mut self, attr: Attribute, value: u8) {
        self.0[attr.index()] = value.clamp(ATTR_MIN, ATTR_MAX);
    }

    /// Vista somente-leitura do array denso subjacente (para serialização/SoA).
    #[must_use]
    pub fn as_slice(&self) -> &[u8; N_ATTR] {
        &self.0
    }
}

/// Par oculto de habilidade — `docs/03 §5`. `current` nunca ultrapassa `potential`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ability {
    current: u8,   // CA, 0..=200
    potential: u8, // PA, 0..=200 — fixo desde a criação do jogador
}

/// Limite superior de CA/PA (`docs/03 §4`).
pub const ABILITY_MAX: u8 = 200;

impl Ability {
    /// Constrói impondo a invariante `current <= potential` (`docs/08` I2):
    /// se `current` vier maior, é saturado no próprio `potential`.
    #[must_use]
    pub fn new(current: u8, potential: u8) -> Self {
        let potential = potential.min(ABILITY_MAX);
        let current = current.min(potential);
        Self { current, potential }
    }

    #[must_use]
    pub const fn current(self) -> u8 {
        self.current
    }

    #[must_use]
    pub const fn potential(self) -> u8 {
        self.potential
    }

    /// Ajusta CA por um delta (progressão mensal — `docs/03 §5.1`), sem
    /// nunca deixar `current` sair de `0..=potential`.
    #[must_use]
    pub fn adjust_current(self, delta: i16) -> Self {
        let next = i16::from(self.current) + delta;
        let clamped = next.clamp(0, i16::from(self.potential)) as u8;
        Self {
            current: clamped,
            potential: self.potential,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn todas_as_variantes_tem_indice_unico_e_denso() {
        let mut seen = [false; N_ATTR];
        for attr in Attribute::ALL {
            assert!(!seen[attr.index()], "índice repetido: {:?}", attr);
            seen[attr.index()] = true;
        }
        assert!(seen.iter().all(|&s| s));
    }

    #[test]
    fn apenas_quatro_atributos_sao_de_goleiro() {
        let count = Attribute::ALL.iter().filter(|a| a.is_goalkeeping()).count();
        assert_eq!(count, N_ATTR_GOALKEEPING);
    }

    #[test]
    fn player_attributes_satura_fora_de_faixa() {
        let mut attrs = PlayerAttributes::default();
        attrs.set(Attribute::Finishing, 255);
        assert_eq!(attrs.get(Attribute::Finishing), ATTR_MAX);
        attrs.set(Attribute::Finishing, 0);
        assert_eq!(attrs.get(Attribute::Finishing), ATTR_MIN);
    }

    #[test]
    fn ability_nunca_deixa_current_passar_de_potential() {
        let ability = Ability::new(180, 150);
        assert_eq!(ability.current(), 150);
        assert_eq!(ability.potential(), 150);
    }

    #[test]
    fn ability_adjust_current_respeita_o_teto_e_o_piso() {
        let ability = Ability::new(140, 160);
        let up = ability.adjust_current(50);
        assert_eq!(up.current(), 160); // não passa do potencial

        let down = ability.adjust_current(-500);
        assert_eq!(down.current(), 0); // não fica negativo
        assert_eq!(down.potential(), 160); // potencial é imutável (I2, docs/08)
    }
}
