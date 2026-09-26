//! Estruturas "resolvidas": as referências de `raw` (Strings) já viraram ids
//! densos (`docs/03 §2`) — a forma como o resto do núcleo (a partir de
//! `world`/`rules`, no M1) vai consumir os dados de um pack.

use domain::{ClubId, CompetitionId, NationId};

#[derive(Debug, Clone)]
pub struct ResolvedNation {
    pub id: NationId,
    /// Id externo original (`"br"`, `"ex"`...) — mantido para diagnóstico e
    /// para a futura camada de import/export (`docs/03 §2`); nada no
    /// caminho quente da simulação deve depender dele.
    pub external_id: String,
    pub name: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Format {
    RoundRobin { legs: u8, teams: u32 },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Tiebreaker {
    Points,
    Wins,
    Draws,
    GoalDifference,
    GoalsFor,
    GoalsAgainst,
    HeadToHead,
}

impl Tiebreaker {
    /// Analisa o texto usado nos data packs (`docs/03 §7`). `None` para
    /// qualquer string desconhecida — vira um item em `issues`, nunca um
    /// `panic` ou um `PackError` (um critério de desempate inválido não
    /// impede o resto do pack de ser lido e reportado).
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        match raw {
            "points" => Some(Self::Points),
            "wins" => Some(Self::Wins),
            "draws" => Some(Self::Draws),
            "goal_difference" => Some(Self::GoalDifference),
            "goals_for" => Some(Self::GoalsFor),
            "goals_against" => Some(Self::GoalsAgainst),
            "head_to_head" => Some(Self::HeadToHead),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Movement {
    pub to: Option<CompetitionId>,
    pub slots: u32,
}

#[derive(Debug, Clone)]
pub struct ResolvedCompetition {
    pub id: CompetitionId,
    pub external_id: String,
    pub name: String,
    pub nation: NationId,
    pub tier: u8,
    pub format: Format,
    pub tiebreakers: Vec<Tiebreaker>,
    pub promotion: Movement,
    pub relegation: Movement,
}

#[derive(Debug, Clone)]
pub struct ResolvedClub {
    pub id: ClubId,
    pub external_id: String,
    pub name: String,
    pub nation: NationId,
    pub competition: CompetitionId,
    pub founded: Option<i32>,
    pub stadium: Option<String>,
}

/// Um data pack completamente carregado e com ids resolvidos. Ver
/// [`crate::PackReport::is_valid`] antes de usar para o jogo de verdade —
/// um `LoadedPack` pode conter menos entidades do que o pack original tinha
/// arquivos, se algum registro foi descartado por referência inválida
/// (`docs/03 §7`, `resolve.rs`).
#[derive(Debug, Clone, Default)]
pub struct LoadedPack {
    pub nations: Vec<ResolvedNation>,
    pub competitions: Vec<ResolvedCompetition>,
    pub clubs: Vec<ResolvedClub>,
}

impl LoadedPack {
    #[must_use]
    pub fn nation(&self, id: NationId) -> &ResolvedNation {
        &self.nations[id.as_usize()]
    }

    #[must_use]
    pub fn competition(&self, id: CompetitionId) -> &ResolvedCompetition {
        &self.competitions[id.as_usize()]
    }

    #[must_use]
    pub fn club(&self, id: ClubId) -> &ResolvedClub {
        &self.clubs[id.as_usize()]
    }

    /// Clubes inscritos numa competição — usado tanto pelo `world` (M1) para
    /// montar o calendário quanto pela própria validação (contagem de
    /// clubes vs. `format.teams`).
    #[must_use]
    pub fn clubs_in(&self, competition: CompetitionId) -> Vec<&ResolvedClub> {
        self.clubs
            .iter()
            .filter(|c| c.competition == competition)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tiebreaker_parse_aceita_o_vocabulario_conhecido() {
        assert_eq!(Tiebreaker::parse("points"), Some(Tiebreaker::Points));
        assert_eq!(
            Tiebreaker::parse("head_to_head"),
            Some(Tiebreaker::HeadToHead)
        );
        assert_eq!(Tiebreaker::parse("chute_de_moeda"), None);
    }
}
