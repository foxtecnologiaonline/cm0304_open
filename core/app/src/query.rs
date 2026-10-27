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
    /// recente já simulada. `None` também para uma competição em formato
    /// de copa (`pack::Format::Knockout`) — copa não tem tabela, usar
    /// [`Query::CupChampion`] para essa (`world::cup`).
    Standings { competition: CompetitionId },
    /// Campeão de uma competição em formato de copa, na temporada mais
    /// recente já simulada. `None` também para uma competição de liga.
    CupChampion { competition: CompetitionId },
    /// Em que temporada a sessão está agora (quantas já foram simuladas).
    CurrentSeason,
    /// Onde a temporada em andamento está: quantas rodadas faltam, se já
    /// começou — o que a UI precisa para mostrar "Rodada 5 de 14" e para
    /// decidir entre "Iniciar temporada" e "Próxima rodada".
    SeasonProgress,
    /// O técnico e o clube sob seu comando (`Command::TakeClub`); `None`
    /// numa sessão só observadora.
    Manager,
    /// Todos os clubes do pack, para escolher um (`Command::TakeClub`) ou
    /// navegar. Ordem de id.
    Clubs,
    /// Ficha de um clube: finanças, estádio, folha. `None` se não existe.
    Club { club: domain::ClubId },
    /// Elenco de um clube, ordenado por posição (goleiro → atacante) e, em
    /// cada posição, do melhor pro pior. `None` se o clube não existe.
    Squad { club: domain::ClubId },
    /// Detalhe de um jogador: linha de elenco + atributos visíveis. `None`
    /// se não existe.
    Player { player: domain::PlayerId },
    /// Calendário do clube na temporada em andamento (ou na última
    /// concluída, entre temporadas), com placar das partidas já jogadas.
    /// `None` se nenhuma temporada foi montada ainda ou o clube não está em
    /// nenhuma liga.
    Fixtures { club: domain::ClubId },
    /// Resultados de todas as ligas numa rodada da temporada em andamento
    /// (ou da última concluída). `matchday: None` = a última jogada.
    /// `None` se nada foi jogado ainda ou a rodada não foi jogada.
    RoundResults { matchday: Option<u32> },
    /// O onze que entraria em campo na **próxima** rodada para o clube sob
    /// comando (já com lesões e suspensões resolvidas), mais formação e
    /// instruções. `None` sem clube sob comando.
    Lineup,
}

/// Resposta a uma [`Query`]. Cada variante corresponde a exatamente uma
/// variante de `Query` — a UI sabe estaticamente que tipo de dado esperar
/// de volta para cada pergunta que fez.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryResult {
    Competitions(Vec<CompetitionSummary>),
    /// `None` quando a competição existe mas nenhuma temporada rodou ainda,
    /// quando o id não existe neste pack, ou quando é uma competição de
    /// copa (sem tabela — ver [`QueryResult::CupChampion`]).
    Standings(Option<Vec<StandingsRow>>),
    /// `None` pelos mesmos motivos de [`QueryResult::Standings`], ou quando
    /// a competição é uma liga (sem campeão único por mata-mata).
    CupChampion(Option<CupChampionRow>),
    CurrentSeason(u32),
    SeasonProgress(SeasonProgress),
    Manager(Option<ManagerInfo>),
    Clubs(Vec<ClubRow>),
    Club(Option<ClubDetail>),
    Squad(Option<Vec<PlayerRow>>),
    Player(Option<PlayerDetail>),
    Fixtures(Option<FixtureList>),
    RoundResults(Option<RoundResults>),
    Lineup(Option<LineupInfo>),
}

/// Um espaço do onze (ver [`Query::Lineup`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineupSlot {
    /// Posição do espaço na formação.
    pub slot_position: domain::Position,
    pub player: domain::PlayerId,
    pub name: String,
    /// Posição natural do jogador; diferente de `slot_position` = fora de
    /// posição.
    pub natural_position: domain::Position,
    pub condition: u8,
    /// Quanto da habilidade o jogador aproveita neste espaço, em milésimos
    /// (1000 = posição natural; ver `world::position_fit_permille`).
    pub fit_permille: u32,
}

/// Escalação e instruções do clube sob comando (ver [`Query::Lineup`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LineupInfo {
    pub formation: ai::Formation,
    /// `true` se o técnico escolheu os titulares; `false` = o melhor onze
    /// disponível a cada rodada.
    pub manual: bool,
    pub mentality: world::Mentality,
    pub pressing: world::Pressing,
    pub slots: Vec<LineupSlot>,
    /// `(escalado que não pode jogar, quem entra no lugar)` na próxima
    /// rodada.
    pub adjustments: Vec<(domain::PlayerId, domain::PlayerId)>,
}

/// Um clube numa lista (ver [`Query::Clubs`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClubRow {
    pub id: domain::ClubId,
    pub name: String,
    /// Competição **atual** (muda com acesso/queda).
    pub competition_name: String,
    /// 0-100.
    pub reputation: u8,
}

/// Ficha de um clube (ver [`Query::Club`]). Valores em centavos
/// (`domain::Money`); a UI formata.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClubDetail {
    pub id: domain::ClubId,
    pub name: String,
    pub nation_name: String,
    pub competition_name: String,
    pub reputation: u8,
    pub budget: domain::Money,
    pub debt: domain::Money,
    pub stadium_capacity: u32,
    pub squad_size: u32,
    /// Folha salarial de uma temporada, pelos contratos de hoje.
    pub payroll_per_season: domain::Money,
}

/// Um jogador numa lista de elenco (ver [`Query::Squad`]). Nunca expõe o
/// potencial (PA): é oculto até o scouting existir (RF-SC-01, M3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerRow {
    pub id: domain::PlayerId,
    pub club: domain::ClubId,
    pub name: String,
    pub position: domain::Position,
    pub age: u8,
    /// CA 0-200.
    pub current_ability: u8,
    /// 0-100 (`world::FULL_CONDITION` = descansado).
    pub condition: u8,
    pub injured: bool,
    /// Cumpre suspensão na próxima rodada.
    pub suspended: bool,
    pub contract_seasons_remaining: u8,
    pub salary_per_season: domain::Money,
    /// Quanto o mercado paga hoje (`ai::market_value`).
    pub market_value: domain::Money,
}

/// Um atributo visível e seu valor (1-20).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AttributeValue {
    /// `attribute.key()` é a chave de localização.
    pub attribute: domain::Attribute,
    pub value: u8,
}

/// Detalhe de um jogador (ver [`Query::Player`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerDetail {
    pub row: PlayerRow,
    pub club_name: String,
    pub nation_name: String,
    /// Os 36 atributos visíveis, na ordem de `domain::Attribute::ALL`.
    pub attributes: Vec<AttributeValue>,
}

/// Placar de uma partida já jogada.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MatchScore {
    pub home_goals: u32,
    pub away_goals: u32,
}

/// Uma partida do calendário (ver [`Query::Fixtures`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixtureRow {
    /// Rodada, 0-based.
    pub matchday: u32,
    pub competition_name: String,
    pub home: domain::ClubId,
    pub home_name: String,
    pub away: domain::ClubId,
    pub away_name: String,
    /// `None` se ainda não foi jogada.
    pub score: Option<MatchScore>,
}

/// Calendário de um clube numa temporada (ver [`Query::Fixtures`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FixtureList {
    pub season_index: u32,
    pub rows: Vec<FixtureRow>,
}

/// Uma partida de uma rodada, já jogada (ver [`Query::RoundResults`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResultRow {
    pub competition_name: String,
    pub home_name: String,
    pub away_name: String,
    pub score: MatchScore,
}

/// Resultados de uma rodada (ver [`Query::RoundResults`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoundResults {
    pub season_index: u32,
    pub matchday: u32,
    pub rows: Vec<ResultRow>,
}

/// Quem está no comando e de quê (ver [`Query::Manager`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ManagerInfo {
    pub coach_name: String,
    pub coach_nation_name: String,
    pub coach_reputation: u8,
    pub club: domain::ClubId,
    pub club_name: String,
    /// Competição **atual** do clube (muda com acesso/queda).
    pub competition_name: String,
}

/// Progresso da temporada atual (ver [`Query::SeasonProgress`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SeasonProgress {
    /// Índice (0-based) da temporada em andamento — ou da próxima a
    /// começar, quando `in_progress` é `false`.
    pub season_index: u32,
    /// `false` antes da primeira rodada: a pré-temporada (folha, mercado,
    /// lesões) só acontece quando a primeira rodada é despachada.
    pub in_progress: bool,
    /// Próxima rodada a jogar (0-based). `0` se `!in_progress`.
    pub next_matchday: u32,
    /// Total de rodadas da temporada. `0` se `!in_progress` (o calendário
    /// só é montado ao começar a temporada, depois do mercado).
    pub total_matchdays: u32,
}

/// O campeão de uma copa, já com o nome do clube resolvido — a UI não
/// recalcula nada (`docs/02 §4`), só mostra. `matches_played` é só as
/// partidas da copa (não conta na calibração de liga, `world::cup`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CupChampionRow {
    pub champion_club_name: String,
    pub rounds_played: u32,
    pub matches_played: u32,
}

/// Uma competição, já com o nome resolvido — o suficiente para popular um
/// seletor de competições na UI.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompetitionSummary {
    pub id: CompetitionId,
    pub name: String,
    pub nation_name: String,
    /// Qual das duas consultas específicas faz sentido pedir para esta
    /// competição — [`Query::Standings`] para `League`,
    /// [`Query::CupChampion`] para `Cup`. Evita que quem chama precise
    /// tentar as duas e ver qual devolve `Some` (`pack::Format`, projetado
    /// sem importar o tipo de `pack` aqui — `docs/02 §4`).
    pub kind: CompetitionKind,
}

/// Projeção mínima de `pack::Format`: só o suficiente para a UI (ou outro
/// consumidor do núcleo) saber qual consulta fazer para uma competição,
/// sem precisar importar `pack` nem conhecer os parâmetros do formato
/// (`legs`, `teams` — isso é detalhe de simulação, não de exibição).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompetitionKind {
    League,
    Cup,
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
