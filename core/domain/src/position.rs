//! Posição primária de um jogador — simplificação deliberada de
//! `docs/03-modelo-de-dados.md §3`, que descreve uma familiaridade por
//! posição (`positions: [u8; N_POS]`, 0..=20 cada). Essa matriz completa
//! ainda não existe: o que há aqui é só a posição principal, o suficiente
//! para o cálculo aproximado de força de elenco (`world::squad_strength`) e
//! para o filtro básico de escalação. Quando a familiaridade por posição
//! for implementada (M2/M3, táticas completas), este tipo deve virar um dos
//! valores possíveis dentro do array, não ser substituído.

/// As quatro famílias de posição usadas para pesar atributos na força de
/// elenco (`world::squad_strength`) — não o detalhe fino de lateral vs.
/// zagueiro vs. volante que o jogo final terá.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Position {
    Goalkeeper,
    Defender,
    Midfielder,
    Forward,
}

impl Position {
    /// Analisa o vocabulário usado nos data packs (`docs/03 §7`). `None`
    /// para qualquer string desconhecida — vira um item em `issues` de
    /// `pack::resolve`, nunca um `panic` (mesmo padrão de
    /// `pack::Tiebreaker::parse`).
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "gk" => Some(Self::Goalkeeper),
            "df" => Some(Self::Defender),
            "mf" => Some(Self::Midfielder),
            "fw" => Some(Self::Forward),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_aceita_o_vocabulario_conhecido() {
        assert_eq!(Position::parse("gk"), Some(Position::Goalkeeper));
        assert_eq!(Position::parse("df"), Some(Position::Defender));
        assert_eq!(Position::parse("mf"), Some(Position::Midfielder));
        assert_eq!(Position::parse("fw"), Some(Position::Forward));
        assert_eq!(Position::parse("ponta_direita"), None);
    }
}
