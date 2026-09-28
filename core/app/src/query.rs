//! Consultas — nunca mudam estado, sempre devolvem **projeções**, não
//! entidades de domínio (`docs/02-arquitetura.md §4`: "a UI recebe
//! `PlayerRow`... não o agregado `Player`"). `StandingsRow` já vem com o
//! nome do clube resolvido — a UI nunca precisa fazer um segundo lookup em
//! `pack` só para mostrar uma tabela.

use domain::CompetitionId;

/// O que a UI pode perguntar ao núcleo.
#[derive(Debug, Clone)]
pub enum Query {
    /// Lista todas as competições do pack carregado (id + nome + país) —
    /// para a UI montar um seletor sem tocar em `pack` diretamente.
    Competitions,
    /// Tabela de classificação de uma competição, na temporada mais
    /// recente já simulada.
    Standings { competition: CompetitionId },
    /// Em que temporada a sessão está agora (quantas já foram simuladas).
    CurrentSeason,
}

/// Resposta a uma [`Query`]. Cada variante corresponde a exatamente uma
/// variante de `Query` — a UI sabe estaticamente que tipo de dado esperar
/// de volta para cada pergunta que fez.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryResult {
    Competitions(Vec<CompetitionSummary>),
    /// `None` quando a competição existe mas nenhuma temporada rodou ainda,
    /// ou quando o id não existe neste pack.
    Standings(Option<Vec<StandingsRow>>),
    CurrentSeason(u32),
}

/// Uma competição, já com o nome resolvido — o suficiente para popular um
/// seletor de competições na UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompetitionSummary {
    pub id: CompetitionId,
    pub name: String,
    pub nation_name: String,
}

/// Uma linha de tabela pronta para exibição — nome do clube já resolvido,
/// posição já calculada, pontos já somados. A UI não recalcula nada disto;
/// só desenha (`docs/02 §4`: "se um cálculo decide algo, mora no núcleo").
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StandingsRow {
    pub position: u32,
    pub club_name: String,
    pub played: u32,
    pub wins: u32,
    pub draws: u32,
    pub losses: u32,
    pub goals_for: u32,
    pub goals_against: u32,
    pub points: u32,
}
