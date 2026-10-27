//! `GameSession` — o estado de uma carreira em memória, e o único lugar
//! onde `dispatch`/`query` (`docs/02-arquitetura.md §4`) ganham corpo.
//! O estado da sessão é função pura de `(pack, world_seed, log de
//! comandos)` (`ADR 0002`): cada comando despachado com sucesso entra no
//! log (`GameSession::to_save`), e `GameSession::load` reconstrói a sessão
//! repetindo o log. Hoje há dois comandos — avançar uma rodada
//! ([`Command::AdvanceRound`]) ou a temporada inteira
//! ([`Command::AdvanceSeason`]) — e as consultas de tabela/campeão/progresso.

use std::collections::{HashMap, HashSet};
use std::path::Path;

use domain::CompetitionId;

use crate::command::{CoachProfile, Command, MAX_COACH_NAME_BYTES};
use crate::error::AppError;
use crate::query::{
    AttributeValue, ClubDetail, ClubRow, CompetitionKind, CompetitionSummary, CupChampionRow,
    FixtureList, FixtureRow, IncomingOfferRow, LineupInfo, LineupSlot, ManagerInfo, MatchLineRow,
    MatchScore, MatchView, PitchPlayer, PlayerDetail, PlayerMatchRow, PlayerRef, PlayerRow, Query,
    QueryResult, ResultRow, RoundResults, SearchPage, SeasonProgress, StandingsRow,
    TransferWindowInfo,
};
use crate::query::{MAX_SEARCH_PAGE, PlayerFilter};
use engine::Side;

/// Confirmação de que um [`Command`] foi processado, com o suficiente para
/// a UI decidir o que mostrar em seguida — nunca o estado inteiro (isso é
/// papel de uma `Query`).
///
/// Cada campo reporta o que **este comando** fez: num [`Command::AdvanceRound`]
/// do meio da temporada só `matches_played`, `suspensions` e
/// `match_day_revenue` são não-zero; pré-temporada (`payroll_paid`,
/// `debt_repaid`, `transfers`, `injuries`) vem no recibo da primeira rodada e
/// fechamento (`movements`, premiação, TV, patrocínio, `tired_players`,
/// `contracts_renewed`) no da última. [`Command::AdvanceSeason`] devolve a
/// soma de todos.
#[derive(Debug, Clone, Copy, Default)]
pub struct CommandReceipt {
    /// Índice da temporada que acabou de ser (ou está sendo) simulada (0-based).
    pub season_index: u32,
    /// Rodada (0-based) que acabou de ser jogada — a última, num
    /// `AdvanceSeason`.
    pub matchday: u32,
    /// Quantas rodadas a temporada tem.
    pub matchdays_total: u32,
    /// `true` se este comando fechou a temporada.
    pub season_completed: bool,
    pub matches_played: u32,
    /// Quantos clubes mudaram de competição para a próxima temporada.
    pub movements: usize,
    /// Quantas transferências a IA de mercado fechou antes da temporada
    /// (`world::run_market_day`, `ai::run_market_day` — `docs/07-roadmap.md`
    /// M1: "primeira IA de mercado").
    pub transfers: usize,
    /// Quantos jogadores estão machucados nesta temporada
    /// (`world::roll_injuries` — `docs/01 §2.3`, RF-JG-08, fatia mínima).
    pub injuries: usize,
    /// Quantos jogadores começam a **próxima** temporada com condição
    /// abaixo de `world::FULL_CONDITION` — titulares que titularizaram a
    /// temporada que acabou de terminar (`world::apply_season_fatigue`,
    /// `docs/01 §2.1`, RF-JG-07, fatia mínima).
    pub tired_players: usize,
    /// Quantas suspensões por expulsão aconteceram nesta temporada, em
    /// partidas de liga (`world::season::SeasonResult::suspensions` —
    /// `crate::discipline`, `docs/01 §2.3`, RF-JG-09, fatia mínima).
    pub suspensions: usize,
    /// Quanto os clubes pagaram de folha salarial nesta temporada, no
    /// total (`world::pay_salaries`, `docs/01 §2.2`, RF-CL-02, fatia
    /// mínima) — pode ser menor que a soma nominal dos salários se algum
    /// clube não tinha caixa suficiente (nunca fica negativo); a
    /// diferença vira dívida (`self.debt`), ver `prize_money_paid`/
    /// `debt_repaid` abaixo.
    pub payroll_paid: domain::Money,
    /// Quanto os clubes receberam de bilheteria nesta temporada, no total
    /// (`world::SeasonResult::match_day_revenue`, `docs/01 §2.2`, RF-CL-03,
    /// fatia mínima) — só partidas de liga em casa, copa não gera receita
    /// aqui.
    pub match_day_revenue: domain::Money,
    /// Quantos contratos renovaram automaticamente por chegar a zero de
    /// duração nesta temporada (`world::advance_contracts`, `docs/01 §2.2`,
    /// RF-CL-02, fatia de duração) — não conta renovação por transferência
    /// (`world::run_market_day`), essa já sai do mercado com contrato novo.
    pub contracts_renewed: usize,
    /// Quanto os clubes receberam de premiação de fim de temporada, no
    /// total (`world::distribute_prize_money`, `docs/01 §2.2`, RF-CL-03,
    /// segunda fatia) — um pool fixo por competição de liga, dividido por
    /// posição final na tabela.
    pub prize_money_paid: domain::Money,
    /// Quanto os clubes receberam de TV nesta temporada, no total
    /// (`world::distribute_tv_revenue`, `docs/01 §2.2`, RF-CL-03, terceira
    /// fatia) — outro pool fixo por competição de liga, mas dividido por
    /// reputação do clube, não posição final (ver `prize_money_paid`,
    /// acima).
    pub tv_revenue_paid: domain::Money,
    /// Quanto os clubes pagaram de dívida acumulada nesta temporada, no
    /// total (`world::repay_debt`, `docs/01 §2.2`, RF-CL-03, quarta
    /// fatia) — descontado do caixa antes da folha salarial desta mesma
    /// temporada (ver `payroll_paid`, acima).
    pub debt_repaid: domain::Money,
    /// Quanto os clubes receberam de patrocínio nesta temporada, no total
    /// (`world::pay_sponsorship`, `docs/01 §2.2`, RF-CL-03, quinta fatia) —
    /// valor absoluto por clube escalado pela reputação, não um pool
    /// dividido entre clubes (ver `tv_revenue_paid`, acima).
    pub sponsorship_paid: domain::Money,
    /// Quantos titulares escolhidos pelo técnico não puderam jogar nesta
    /// rodada e foram trocados pelo melhor substituto (lesão nova,
    /// suspensão, jogador vendido) — `world::resolve_xi`. `Query::Lineup`
    /// diz quem, antes da rodada.
    pub lineup_adjustments: usize,
    /// Resposta a `Command::MakeOffer`/`Command::RenewContract`; `None` nos
    /// demais comandos.
    pub offer: Option<world::OfferOutcome>,
}

/// A janela de verão fecha antes desta rodada (0-based): rodadas 0 a 3.
const SUMMER_WINDOW_MATCHDAYS: u32 = 4;
/// A janela de inverno abre na metade da temporada e dura esta quantidade
/// de rodadas.
const WINTER_WINDOW_MATCHDAYS: u32 = 2;

impl CommandReceipt {
    /// Soma o recibo de um comando posterior da mesma temporada: totais
    /// somam, posição (`season_index`, `matchday`, `matchdays_total`,
    /// `season_completed`) fica a do mais recente.
    fn absorb(&mut self, later: CommandReceipt) {
        self.season_index = later.season_index;
        self.matchday = later.matchday;
        self.matchdays_total = later.matchdays_total;
        self.season_completed = later.season_completed;
        self.matches_played += later.matches_played;
        self.movements += later.movements;
        self.transfers += later.transfers;
        self.injuries += later.injuries;
        self.tired_players += later.tired_players;
        self.suspensions += later.suspensions;
        self.payroll_paid = self.payroll_paid + later.payroll_paid;
        self.match_day_revenue = self.match_day_revenue + later.match_day_revenue;
        self.contracts_renewed += later.contracts_renewed;
        self.prize_money_paid = self.prize_money_paid + later.prize_money_paid;
        self.tv_revenue_paid = self.tv_revenue_paid + later.tv_revenue_paid;
        self.debt_repaid = self.debt_repaid + later.debt_repaid;
        self.sponsorship_paid = self.sponsorship_paid + later.sponsorship_paid;
        self.lineup_adjustments += later.lineup_adjustments;
        self.offer = later.offer.or(self.offer);
    }
}

/// Temporada que já passou da pré-temporada e está no meio das rodadas.
#[derive(Debug)]
struct ActiveSeason {
    run: world::SeasonRun,
    /// Titulares da temporada, escolhidos na pré-temporada — a mesma
    /// seleção que `world::apply_season_fatigue`/`advance_season` precisam
    /// no fechamento (ver `GameSession::end_season`).
    starters: HashSet<domain::PlayerId>,
}

/// Monta o `CareerRound` a partir dos campos da sessão — macro (e não
/// método) porque precisa emprestar campos disjuntos de `self` ao mesmo
/// tempo que `self.pack`/`self.profiles` são lidos por quem chama.
macro_rules! career_round {
    ($s:expr) => {
        world::CareerRound {
            roster: &$s.roster,
            budgets: &mut $s.budgets,
            stadium_capacities: &$s.stadium_capacities,
            club_reputation: &$s.club_reputation,
            dirtiness: &$s.dirtiness,
            big_game_mentality: &$s.big_game_mentality,
            consistency: &$s.consistency,
            adaptation: &$s.adaptation,
            seasons_since_transfer: &$s.seasons_since_transfer,
            setups: $s
                .manager
                .as_ref()
                .map_or(&[][..], |m| std::slice::from_ref(&m.setup)),
        }
    };
}

/// Uma carreira carregada em memória.
#[derive(Debug)]
pub struct GameSession {
    pack: pack::LoadedPack,
    /// Identidade do pack (`pack::PackManifest::id`/`version`) — guardada
    /// para gravar no save e para `load` recusar reabrir a carreira contra
    /// um pack diferente (`AppError::SavePackMismatch`).
    pack_id: String,
    pack_version: String,
    /// Competição atual de cada clube, indexada por id denso — começa como
    /// `pack.clubs[i].competition` e muda com promoção/rebaixamento
    /// (mesmo desenho de `world::season`, só que preservado entre
    /// despachos em vez de interno a uma chamada só).
    membership: Vec<CompetitionId>,
    /// Força/finalização/goleiro de cada clube para o motor
    /// (`engine::TeamMatchProfile`) — recalculado a cada `AdvanceSeason`
    /// (`world::generate_match_profiles_from_roster`) a partir do roster
    /// atual, então muda de temporada pra temporada mesmo sem promoção/
    /// rebaixamento (progressão, `world::advance_season`) ou com (mercado,
    /// `world::run_market_day`).
    profiles: Vec<engine::TeamMatchProfile>,
    /// CA e idade de cada jogador nesta carreira (`world::progression`) —
    /// separado de `pack.players`, que nunca muda.
    roster: Vec<world::PlayerState>,
    /// Orçamento de cada clube, indexado por id denso (`world::generate_budgets`,
    /// sintético — pack ainda não declara finanças, `world::finance`). Muda
    /// por transferência (`world::run_market_day`, conserva dinheiro entre
    /// clubes), folha salarial (`world::pay_salaries`, só debita) e
    /// bilheteria (`world::run_season` com `CareerRound`, só credita) — uma
    /// economia aberta desde que a folha existe (`docs/07-roadmap.md` M2).
    budgets: Vec<domain::Money>,
    /// Dívida de cada clube, indexada por id denso — o que
    /// `world::pay_salaries` não conseguiu debitar do caixa numa
    /// temporada, paga de volta por `world::repay_debt` na seguinte,
    /// antes de qualquer outro gasto (fatia mínima de RF-CL-03). Começa
    /// em zero em [`Self::new`]: só a economia simulada cria dívida, não
    /// há dívida herdada do pack.
    debt: Vec<domain::Money>,
    /// Capacidade de estádio de cada clube, indexada por id denso
    /// (`world::generate_stadium_capacities`, sintética — mesmo status de
    /// `budgets`). Gerada uma vez em [`Self::new`] e nunca muda depois:
    /// não há obras de estádio ainda (RF-CL-05, M4).
    stadium_capacities: Vec<u32>,
    /// Reputação de cada clube, 0-100, indexada por id denso
    /// (`world::generate_club_reputation`, sintética — mesmo status de
    /// `stadium_capacities`). Gerada uma vez em [`Self::new`] e nunca muda
    /// depois: sem histórico de títulos ainda (RF-CL-08, M5). Influencia
    /// a bilheteria (RF-MU-08, `world::finance::match_day_revenue`).
    club_reputation: Vec<u8>,
    /// Contrato de cada jogador, no mesmo índice denso que `roster`
    /// (`world::ContractState`, invariante do módulo `world::contract`) —
    /// duração restante e CA travado pro salário (`world::pay_salaries`
    /// usa o CA travado, não o corrente). Muda a cada `AdvanceSeason`
    /// (`world::advance_contracts`, renovação automática) e a cada
    /// transferência (`world::run_market_day`, contrato novo no comprador).
    contracts: Vec<world::ContractState>,
    /// Propensão a lesão de cada jogador, 0-100, no mesmo índice denso que
    /// `roster` (`world::generate_injury_proneness`, sintética — mesmo
    /// status de `club_reputation`). Gerada uma vez em [`Self::new`] e
    /// nunca muda depois: sem atributos ocultos de verdade ainda (RF-JG-02
    /// completo). Usada por `world::roll_injuries` pra variar a chance de
    /// lesão por jogador.
    injury_proneness: Vec<u8>,
    /// Profissionalismo de cada jogador, 0-100, no mesmo índice denso que
    /// `roster` (`world::generate_professionalism`, sintético — mesmo
    /// status de `injury_proneness`). Gerado uma vez em [`Self::new`] e
    /// nunca muda depois. Usado por `world::advance_season` como bônus de
    /// evolução por cima da curva de idade/minutos jogados.
    professionalism: Vec<u8>,
    /// Lealdade de cada jogador ao clube atual, 0-100, no mesmo índice
    /// denso que `roster` (`world::generate_loyalty`, sintética — mesmo
    /// status de `professionalism`). Gerada uma vez em [`Self::new`] e
    /// nunca muda depois. Usada por `world::run_market_day` pra nunca
    /// vender um jogador muito leal, mesmo que o preço bata.
    loyalty: Vec<u8>,
    /// Ambição de cada jogador, 0-100, no mesmo índice denso que `roster`
    /// (`world::generate_ambition`, sintética — mesmo status de
    /// `loyalty`). Gerada uma vez em [`Self::new`] e nunca muda depois.
    /// Usada por `world::run_market_day`, junto de `club_reputation`, pra
    /// recusar transferência que seria um passo atrás de reputação pra um
    /// jogador muito ambicioso.
    ambition: Vec<u8>,
    /// Sujeira de cada jogador, 0-100, no mesmo índice denso que `roster`
    /// (`world::generate_dirtiness`, sintética — mesmo status de
    /// `loyalty`). Gerada uma vez em [`Self::new`] e nunca muda depois.
    /// Usada por `world::roll_suspensions` pra pesar quem é escolhido
    /// quando o time sofre uma expulsão.
    dirtiness: Vec<u8>,
    /// Mentalidade de "jogo importante" de cada jogador, 0-100, no mesmo
    /// índice denso que `roster` (`world::generate_big_game_mentality`,
    /// sintética — mesmo status de `dirtiness`). Gerada uma vez em
    /// [`Self::new`] e nunca muda depois. Usada por
    /// `world::CareerRound::big_game_mentality` pra escalar a força de
    /// cada clube só na copa.
    big_game_mentality: Vec<u8>,
    /// Consistência de cada jogador, 0-100, no mesmo índice denso que
    /// `roster` (`world::generate_consistency`, sintética — mesmo status
    /// de `big_game_mentality`). Gerada uma vez em [`Self::new`] e nunca
    /// muda depois. Usada por `world::CareerRound::consistency` pra
    /// balançar a força de cada clube a cada rodada de liga.
    consistency: Vec<u8>,
    /// Velocidade de adaptação de cada jogador, 0-100, no mesmo índice
    /// denso que `roster` (`world::generate_adaptation`, sintética —
    /// mesmo status de `consistency`). Gerada uma vez em [`Self::new`] e
    /// nunca muda depois. Usada por `world::CareerRound::adaptation`
    /// junto de `seasons_since_transfer` pra reduzir a força de um
    /// recém-chegado por até duas temporadas.
    adaptation: Vec<u8>,
    /// Quantas temporadas desde a última transferência de cada jogador,
    /// no mesmo índice denso que `roster` — ao contrário de todo outro
    /// atributo acima, **muda durante a carreira**:
    /// `world::initial_seasons_since_transfer` em [`Self::new`] (todo
    /// mundo "já assentado"), `world::mark_transferred` zera quem acabou
    /// de ser comprado (`dispatch`, logo depois de `world::run_market_day`)
    /// e `world::advance_seasons_since_transfer` incrementa todo mundo em
    /// 1 no fim de cada temporada.
    seasons_since_transfer: Vec<u8>,
    world_seed: u64,
    /// Uma entrada por temporada já **concluída** — a fonte de verdade para
    /// toda `Query` sobre o estado atual do mundo.
    history: Vec<world::SeasonResult>,
    /// Temporada em andamento (pré-temporada feita, rodadas em curso), se
    /// houver. `None` entre temporadas.
    season: Option<ActiveSeason>,
    /// A última temporada **concluída**, mantida só pra `Query::Fixtures`/
    /// `Query::RoundResults` responderem entre temporadas. Substituída a
    /// cada fechamento (nunca acumula — custo de memória constante).
    last_run: Option<world::SeasonRun>,
    /// Partida do clube sob comando em andamento (`Command::StartMatch`).
    live: Option<world::LiveGame>,
    /// Relatório da última partida concluída do clube sob comando.
    last_match: Option<world::MatchReport>,
    /// Propostas da IA por jogadores do clube sob comando, ainda de pé.
    incoming: Vec<world::IncomingOffer>,
    next_offer_id: u32,
    /// `(temporada, rodada)` para a qual já se sorteou proposta — no máximo
    /// uma geração por rodada.
    offers_generated_for: Option<(u32, u32)>,
    /// Comandos despachados com sucesso, codificados (`Command::encode`),
    /// na ordem — vira o `command_log` do save.
    command_log: Vec<u8>,
    /// O técnico e o clube que ele assumiu (`Command::TakeClub`); `None`
    /// numa sessão só observadora.
    manager: Option<Manager>,
}

/// Estado do técnico sob comando de um clube.
#[derive(Debug)]
struct Manager {
    club: domain::ClubId,
    coach: CoachProfile,
    /// Escalação e instruções (começa no padrão, indistinguível da IA —
    /// `world::ClubSetup::new`).
    setup: world::ClubSetup,
}

impl GameSession {
    /// Carrega um data pack e começa uma sessão nova, temporada 0, ainda
    /// sem nenhuma partida simulada. Recusa um pack inválido — a sessão
    /// nunca começa com um mundo inconsistente (`docs/03 §7`).
    pub fn new(pack_path: &Path, world_seed: u64) -> Result<Self, AppError> {
        let report = pack::load_and_validate(pack_path).map_err(AppError::PackLoad)?;
        if !report.is_valid() {
            return Err(AppError::PackInvalid {
                issues: report.issues,
            });
        }
        if report.pack.competitions.is_empty() {
            return Err(AppError::NoCompetitions);
        }

        let roster = world::initial_roster(&report.pack);
        let budgets = world::generate_budgets(&report.pack, world_seed);
        let debt = vec![domain::Money::ZERO; report.pack.clubs.len()];
        let stadium_capacities = world::generate_stadium_capacities(&report.pack, world_seed);
        let club_reputation = world::generate_club_reputation(&report.pack, world_seed);
        let contracts = world::generate_initial_contracts(&report.pack, world_seed);
        let injury_proneness = world::generate_injury_proneness(&report.pack, world_seed);
        let professionalism = world::generate_professionalism(&report.pack, world_seed);
        let loyalty = world::generate_loyalty(&report.pack, world_seed);
        let ambition = world::generate_ambition(&report.pack, world_seed);
        let dirtiness = world::generate_dirtiness(&report.pack, world_seed);
        let big_game_mentality = world::generate_big_game_mentality(&report.pack, world_seed);
        let consistency = world::generate_consistency(&report.pack, world_seed);
        let adaptation = world::generate_adaptation(&report.pack, world_seed);
        let seasons_since_transfer = world::initial_seasons_since_transfer(&report.pack);
        let profiles =
            world::generate_match_profiles_from_roster(&report.pack, &roster, world_seed);
        let membership: Vec<CompetitionId> =
            report.pack.clubs.iter().map(|c| c.competition).collect();

        Ok(Self {
            pack: report.pack,
            pack_id: report.manifest.id,
            pack_version: report.manifest.version,
            membership,
            profiles,
            roster,
            budgets,
            debt,
            stadium_capacities,
            club_reputation,
            contracts,
            injury_proneness,
            professionalism,
            loyalty,
            ambition,
            dirtiness,
            big_game_mentality,
            consistency,
            adaptation,
            seasons_since_transfer,
            world_seed,
            history: Vec::new(),
            season: None,
            last_run: None,
            live: None,
            last_match: None,
            incoming: Vec::new(),
            next_offer_id: 0,
            offers_generated_for: None,
            command_log: Vec::new(),
            manager: None,
        })
    }

    /// Reabre uma carreira salva: carrega o pack do zero e **repete** cada
    /// comando do log via [`Self::dispatch`], em vez de restaurar um
    /// instantâneo. Funciona porque a sessão inteira é uma função pura de
    /// `(pack, world_seed, log de comandos)` (`ADR 0002`) — o "log de
    /// comandos" de `docs/02 §4`. Um save feito no meio da temporada reabre
    /// no mesmo ponto.
    pub fn load(pack_path: &Path, save: &persist::SaveFile) -> Result<Self, AppError> {
        let mut session = Self::new(pack_path, save.world_seed)?;
        if session.pack_id != save.pack_id || session.pack_version != save.pack_version {
            return Err(AppError::SavePackMismatch {
                save_pack_id: save.pack_id.clone(),
                save_pack_version: save.pack_version.clone(),
                loaded_pack_id: session.pack_id,
                loaded_pack_version: session.pack_version,
            });
        }
        let mut position = 0;
        while position < save.command_log.len() {
            let command = Command::decode(&save.command_log, &mut position)?;
            session.dispatch(command)?;
        }
        Ok(session)
    }

    /// Lê um arquivo de save em `save_path` e reabre a carreira contra o
    /// pack em `pack_path` — o par de operações que `managerfc-cli
    /// load`/uma futura UI de fato chamam; [`Self::load`] (a partir de um
    /// [`persist::SaveFile`] já em memória) existe separado para ser
    /// testável sem tocar disco.
    pub fn load_from_path(pack_path: &Path, save_path: &Path) -> Result<Self, AppError> {
        let save = persist::load_from_path(save_path).map_err(AppError::Persist)?;
        Self::load(pack_path, &save)
    }

    /// Extrai o [`persist::SaveFile`] desta sessão — puro, não toca disco
    /// (ver [`Self::save_to_path`] para isso).
    #[must_use]
    pub fn to_save(&self) -> persist::SaveFile {
        persist::SaveFile {
            pack_id: self.pack_id.clone(),
            pack_version: self.pack_version.clone(),
            world_seed: self.world_seed,
            command_log: self.command_log.clone(),
        }
    }

    /// Grava esta sessão em disco (escrita atômica — `persist::save_to_path`).
    pub fn save_to_path(&self, path: &Path) -> Result<(), AppError> {
        persist::save_to_path(path, &self.to_save()).map_err(AppError::Persist)
    }

    /// Processa um comando, mudando o estado da sessão. Todo comando que
    /// dá certo entra no log do save (`Self::to_save`).
    pub fn dispatch(&mut self, command: Command) -> Result<CommandReceipt, AppError> {
        let receipt = match &command {
            Command::AdvanceRound => self.advance_round(),
            Command::AdvanceSeason => {
                let mut total = self.advance_round();
                while !total.season_completed {
                    let next = self.advance_round();
                    total.absorb(next);
                }
                total
            }
            Command::TakeClub { club, coach } => {
                self.take_club(*club, coach.clone())?;
                self.idle_receipt()
            }
            Command::SetFormation { formation } => {
                self.set_formation(*formation)?;
                self.idle_receipt()
            }
            Command::SetLineup {
                formation,
                starters,
            } => {
                self.set_lineup(*formation, starters)?;
                self.idle_receipt()
            }
            Command::SetInstructions {
                mentality,
                pressing,
            } => {
                self.set_instructions(*mentality, *pressing)?;
                self.idle_receipt()
            }
            Command::StartMatch => self.start_match()?,
            Command::BeginSeason => self.begin_season_command()?,
            Command::RespondToOffer { offer, accept } => {
                let outcome = self.respond_to_offer(*offer, *accept)?;
                CommandReceipt {
                    offer: outcome,
                    ..self.idle_receipt()
                }
            }
            Command::MakeOffer {
                player,
                fee,
                wage_permille,
                seasons,
            } => {
                let outcome = self.make_offer(*player, *fee, *wage_permille, *seasons)?;
                CommandReceipt {
                    offer: Some(outcome),
                    ..self.idle_receipt()
                }
            }
            Command::RenewContract {
                player,
                wage_permille,
                seasons,
            } => {
                let outcome = self.renew_contract(*player, *wage_permille, *seasons)?;
                CommandReceipt {
                    offer: Some(outcome),
                    ..self.idle_receipt()
                }
            }
            Command::PlayMinutes { minutes } => {
                self.play_minutes(*minutes)?;
                self.idle_receipt()
            }
            Command::Substitute { out, incoming } => {
                self.substitute(*out, *incoming)?;
                self.idle_receipt()
            }
        };
        command.encode(&mut self.command_log);
        Ok(receipt)
    }

    /// Recibo de um comando que não joga nada (técnico, escalação, ...).
    fn idle_receipt(&self) -> CommandReceipt {
        CommandReceipt {
            season_index: self.history.len() as u32,
            ..CommandReceipt::default()
        }
    }

    /// Valida e registra o técnico e seu clube (ver [`Command::TakeClub`]).
    fn take_club(&mut self, club: domain::ClubId, coach: CoachProfile) -> Result<(), AppError> {
        if self.manager.is_some() {
            return Err(AppError::ClubAlreadyTaken);
        }
        if !self.history.is_empty() || self.season.is_some() {
            return Err(AppError::CareerAlreadyStarted);
        }
        if club.as_usize() >= self.pack.clubs.len() {
            return Err(AppError::UnknownClub(club));
        }
        if coach.name.trim().is_empty() {
            return Err(AppError::InvalidCoach("nome vazio"));
        }
        if coach.name.len() > MAX_COACH_NAME_BYTES {
            return Err(AppError::InvalidCoach("nome longo demais"));
        }
        if coach.nation.as_usize() >= self.pack.nations.len() {
            return Err(AppError::InvalidCoach("nacionalidade inexistente no pack"));
        }
        if coach.reputation > 100 {
            return Err(AppError::InvalidCoach("reputação acima de 100"));
        }
        self.manager = Some(Manager {
            club,
            coach,
            setup: world::ClubSetup::new(club),
        });
        Ok(())
    }

    /// Joga a partida do clube sob comando — a ao vivo que estava em
    /// andamento (terminando o que faltar) ou, se não há, uma instantânea
    /// (a mesma `LiveGame`, sem intervenção, então o placar é o mesmo que o
    /// do modo ao vivo sem mexer em nada). Guarda o relatório em
    /// `last_match` e devolve o resultado para a rodada usar no lugar da
    /// simulação. `None` se não há técnico ou o clube não joga nesta rodada.
    fn play_managed_fixture(&mut self, active: &mut ActiveSeason) -> Option<world::ExternalResult> {
        self.manager.as_ref()?;
        let mut game = match self.live.take() {
            Some(game) => game,
            None => {
                {
                    let career = career_round!(self);
                    active
                        .run
                        .begin_matchday(&self.pack, &career, self.world_seed);
                }
                self.new_live_game(active)?
            }
        };
        game.play_to_end(&self.pack);
        let external = game.external_result();
        self.last_match = Some(game.report());
        Some(external)
    }

    /// A partida do clube sob comando na rodada já preparada, parada no
    /// minuto 0.
    fn new_live_game(&self, active: &ActiveSeason) -> Option<world::LiveGame> {
        let manager = self.manager.as_ref()?;
        let prepared = active.run.prepared_fixture_of(manager.club)?;
        let unavailable = self.suspended_in(&active.run, manager.club);
        Some(world::LiveGame::new(
            &self.pack,
            &self.roster,
            &prepared,
            Some(&manager.setup),
            &unavailable,
            self.world_seed,
        ))
    }

    /// Ver [`Command::StartMatch`].
    fn start_match(&mut self) -> Result<CommandReceipt, AppError> {
        if self.manager.is_none() {
            return Err(AppError::NoClubTaken);
        }
        if self.live.is_some() {
            return Err(AppError::MatchInProgress);
        }
        let mut receipt = CommandReceipt::default();
        let mut active = match self.season.take() {
            Some(active) => active,
            None => self.begin_season(&mut receipt),
        };
        receipt.season_index = active.run.season_index();
        receipt.matchdays_total = active.run.total_matchdays();
        receipt.matchday = active.run.next_matchday();
        {
            let career = career_round!(self);
            active
                .run
                .begin_matchday(&self.pack, &career, self.world_seed);
        }
        self.live = self.new_live_game(&active);
        self.season = Some(active);
        self.refresh_offers();
        Ok(receipt)
    }

    fn play_minutes(&mut self, minutes: u8) -> Result<(), AppError> {
        let game = self.live.as_mut().ok_or(AppError::NoLiveMatch)?;
        game.advance(&self.pack, minutes);
        Ok(())
    }

    fn substitute(
        &mut self,
        out: domain::PlayerId,
        incoming: domain::PlayerId,
    ) -> Result<(), AppError> {
        use world::SubstitutionError as E;
        let game = self.live.as_mut().ok_or(AppError::NoLiveMatch)?;
        let side = game.managed_side().ok_or(AppError::NoLiveMatch)?;
        game.substitute(&self.pack, &self.roster, side, out, incoming)
            .map_err(|e| {
                AppError::InvalidSubstitution(match e {
                    E::MatchFinished => "a partida já acabou",
                    E::SideNotManaged => "só o seu time pode substituir",
                    E::NoSubstitutionsLeft => "limite de 3 substituições por partida",
                    E::PlayerNotOnPitch => "o jogador não está em campo",
                    E::PlayerNotOnBench => "o jogador não está no banco",
                })
            })
    }

    fn player_ref(&self, id: domain::PlayerId) -> PlayerRef {
        let p = &self.pack.players[id.as_usize()];
        PlayerRef {
            id,
            name: format!("{} {}", p.first_name, p.last_name),
        }
    }

    fn pitch_player(&self, slot_position: domain::Position, id: domain::PlayerId) -> PitchPlayer {
        let state = &self.roster[id.as_usize()];
        PitchPlayer {
            player: self.player_ref(id),
            slot_position,
            natural_position: state.position,
            condition: state.condition,
        }
    }

    /// Monta o retrato de uma partida; `game` só existe ao vivo e traz o
    /// onze em campo e o banco.
    fn match_view(
        &self,
        report: &world::MatchReport,
        lines_from: usize,
        game: Option<&world::LiveGame>,
    ) -> MatchView {
        let lines = report
            .lines
            .iter()
            .skip(lines_from)
            .map(|l| MatchLineRow {
                minute: l.minute,
                kind: l.kind,
                side: l.side,
                player: l.player.map(|p| self.player_ref(p)),
                other: l.other.map(|p| self.player_ref(p)),
                home_goals: l.home_goals,
                away_goals: l.away_goals,
            })
            .collect();
        let players = report
            .players
            .iter()
            .map(|p| PlayerMatchRow {
                player: self.player_ref(p.player),
                club: p.club,
                side: p.side,
                position: p.position,
                started: p.started,
                minutes: p.minutes,
                shots: p.shots,
                goals: p.goals,
                assists: p.assists,
                rating_x10: p.rating_x10,
            })
            .collect();
        let managed_side = game.and_then(world::LiveGame::managed_side).or_else(|| {
            let club = self.manager.as_ref()?.club;
            if club == report.home {
                Some(Side::Home)
            } else if club == report.away {
                Some(Side::Away)
            } else {
                None
            }
        });
        let (on_pitch, bench, substitutions_left) = match (game, managed_side) {
            (Some(game), Some(side)) => (
                game.on_pitch(side)
                    .iter()
                    .map(|&(slot, id)| self.pitch_player(slot, id))
                    .collect(),
                game.bench(side)
                    .iter()
                    .map(|&id| self.pitch_player(self.roster[id.as_usize()].position, id))
                    .collect(),
                game.substitutions_left(side),
            ),
            _ => (Vec::new(), Vec::new(), 0),
        };
        MatchView {
            season_index: report.season_index,
            competition_name: self.pack.competition(report.competition).name.clone(),
            home: report.home,
            home_name: self.pack.club(report.home).name.clone(),
            away: report.away,
            away_name: self.pack.club(report.away).name.clone(),
            minute: report.minute,
            finished: report.finished,
            home_goals: report.goals.0,
            away_goals: report.goals.1,
            home_shots: report.shots.0,
            away_shots: report.shots.1,
            home_xg_milli: report.xg_milli.0,
            away_xg_milli: report.xg_milli.1,
            managed_side,
            lines,
            total_lines: report.lines.len(),
            players,
            on_pitch,
            bench,
            substitutions_left,
        }
    }

    /// Janelas de transferência (RF-TR-01): a de verão vai da pré-temporada
    /// até a rodada `SUMMER_WINDOW_MATCHDAYS`; a de inverno abre no meio da
    /// temporada e dura `WINTER_WINDOW_MATCHDAYS` rodadas. Fora de uma
    /// temporada em andamento não há janela.
    fn window_info(&self) -> TransferWindowInfo {
        let Some(active) = &self.season else {
            return TransferWindowInfo {
                open: false,
                matchdays_until_change: None,
            };
        };
        let next = active.run.next_matchday();
        let winter_start = active.run.total_matchdays() / 2;
        let winter_end = winter_start + WINTER_WINDOW_MATCHDAYS;
        let (open, until) = if next < SUMMER_WINDOW_MATCHDAYS {
            (true, Some(SUMMER_WINDOW_MATCHDAYS - next))
        } else if (winter_start..winter_end).contains(&next) {
            (true, Some(winter_end - next))
        } else if next < winter_start {
            (false, Some(winter_start - next))
        } else {
            (false, None)
        };
        TransferWindowInfo {
            open,
            matchdays_until_change: until,
        }
    }

    /// Ver [`Command::BeginSeason`].
    fn begin_season_command(&mut self) -> Result<CommandReceipt, AppError> {
        if self.season.is_some() {
            return Err(AppError::SeasonAlreadyBegun);
        }
        let mut receipt = CommandReceipt::default();
        let active = self.begin_season(&mut receipt);
        receipt.season_index = active.run.season_index();
        receipt.matchdays_total = active.run.total_matchdays();
        self.season = Some(active);
        self.refresh_offers();
        Ok(receipt)
    }

    /// Ver [`Command::MakeOffer`].
    fn make_offer(
        &mut self,
        player: domain::PlayerId,
        fee: domain::Money,
        wage_permille: u16,
        seasons: u8,
    ) -> Result<world::OfferOutcome, AppError> {
        use world::{ClubVerdict, OfferOutcome, Refusal};
        let buyer = self.manager.as_ref().ok_or(AppError::NoClubTaken)?.club;
        if self.live.is_some() {
            return Err(AppError::MatchInProgress);
        }
        if !self.window_info().open {
            return Err(AppError::TransferWindowClosed);
        }
        self.validate_terms(wage_permille, seasons)?;
        let idx = player.as_usize();
        let state = *self
            .roster
            .get(idx)
            .ok_or(AppError::InvalidOffer("jogador inexistente"))?;
        if state.club == buyer {
            return Err(AppError::InvalidOffer("o jogador já é do seu clube"));
        }
        let seller = state.club;

        // 1. O clube vendedor.
        if self.loyalty[idx] >= ai::LOYALTY_REFUSAL_THRESHOLD {
            return Ok(OfferOutcome::ClubRefuses(Refusal::NotForSale));
        }
        if !world::seller_can_part(&self.roster, seller, player) {
            return Ok(OfferOutcome::ClubRefuses(Refusal::SquadLimits));
        }
        let value = ai::market_value(
            state.ability.current(),
            self.contracts[idx].seasons_remaining,
            state.age_years.clamp(0, 255) as u8,
            state.ability.potential(),
            self.club_reputation[seller.as_usize()],
        );
        match world::evaluate_club(fee, value) {
            ClubVerdict::Refuse(reason) => return Ok(OfferOutcome::ClubRefuses(reason)),
            ClubVerdict::Counter(asking) => return Ok(OfferOutcome::ClubWantsMore { asking }),
            ClubVerdict::Accept => {}
        }
        if self.budgets[buyer.as_usize()] < fee {
            return Ok(OfferOutcome::CannotAfford);
        }

        // 2. O jogador.
        let seller_rep = self.club_reputation[seller.as_usize()];
        let buyer_rep = self.club_reputation[buyer.as_usize()];
        if !ai::accepts_destination(self.ambition[idx], seller_rep, buyer_rep) {
            return Ok(OfferOutcome::PlayerRefuses(Refusal::WantsBiggerClub));
        }
        let ask = world::wage_ask_permille(self.ambition[idx], seller_rep, buyer_rep);
        if wage_permille < ask {
            return Ok(OfferOutcome::PlayerWantsMore {
                asking_wage_permille: ask,
            });
        }

        // 3. Fechado: o dinheiro muda de mãos, o jogador troca de clube e
        // assina o contrato negociado, e volta a sofrer o malus de
        // adaptação (`world::adaptation`).
        self.budgets[buyer.as_usize()] = self.budgets[buyer.as_usize()] - fee;
        self.budgets[seller.as_usize()] = self.budgets[seller.as_usize()] + fee;
        self.roster[idx].club = buyer;
        self.contracts[idx] = world::ContractState {
            seasons_remaining: seasons,
            signed_ability: state.ability.current(),
            wage_permille,
        };
        world::mark_transferred(&mut self.seasons_since_transfer, &[player]);
        Ok(OfferOutcome::Accepted)
    }

    /// Ver [`Command::RenewContract`].
    fn renew_contract(
        &mut self,
        player: domain::PlayerId,
        wage_permille: u16,
        seasons: u8,
    ) -> Result<world::OfferOutcome, AppError> {
        let club = self.manager.as_ref().ok_or(AppError::NoClubTaken)?.club;
        self.validate_terms(wage_permille, seasons)?;
        let idx = player.as_usize();
        let state = *self
            .roster
            .get(idx)
            .ok_or(AppError::InvalidOffer("jogador inexistente"))?;
        if state.club != club {
            return Err(AppError::InvalidOffer("o jogador não é do seu clube"));
        }
        let ask = world::renewal_ask_permille(self.ambition[idx], self.loyalty[idx]);
        if wage_permille < ask {
            return Ok(world::OfferOutcome::PlayerWantsMore {
                asking_wage_permille: ask,
            });
        }
        self.contracts[idx] = world::ContractState {
            seasons_remaining: seasons,
            signed_ability: state.ability.current(),
            wage_permille,
        };
        Ok(world::OfferOutcome::Accepted)
    }

    fn validate_terms(&self, wage_permille: u16, seasons: u8) -> Result<(), AppError> {
        if !(world::MIN_CONTRACT_SEASONS..=world::MAX_CONTRACT_SEASONS).contains(&seasons) {
            return Err(AppError::InvalidOffer("contrato de 1 a 5 temporadas"));
        }
        if !(world::MIN_WAGE_PERMILLE..=world::MAX_WAGE_PERMILLE).contains(&wage_permille) {
            return Err(AppError::InvalidOffer("salário fora da faixa aceita"));
        }
        Ok(())
    }

    /// Expira as propostas vencidas e, com a janela aberta, sorteia a desta
    /// rodada (no máximo uma por rodada — `offers_generated_for`).
    fn refresh_offers(&mut self) {
        let Some(club) = self.manager.as_ref().map(|m| m.club) else {
            return;
        };
        let Some(active) = &self.season else {
            self.incoming.clear();
            return;
        };
        let next = active.run.next_matchday();
        let season = active.run.season_index();
        let open = self.window_info().open;
        let roster = &self.roster;
        self.incoming.retain(|o| {
            open && next < o.expires_matchday && roster[o.player.as_usize()].club == club
        });
        if !open || self.offers_generated_for == Some((season, next)) {
            return;
        }
        self.offers_generated_for = Some((season, next));
        let pending: Vec<domain::PlayerId> = self.incoming.iter().map(|o| o.player).collect();
        let world_view = world::OfferWorld {
            roster: &self.roster,
            contracts: &self.contracts,
            budgets: &self.budgets,
            club_reputation: &self.club_reputation,
            ambition: &self.ambition,
        };
        if let Some((player, from, fee)) = world::generate_incoming_offer(
            &world_view,
            club,
            &pending,
            self.world_seed,
            season,
            next,
        ) {
            let id = self.next_offer_id;
            self.next_offer_id += 1;
            self.incoming.push(world::IncomingOffer {
                id,
                player,
                from,
                fee,
                expires_matchday: next + world::OFFER_LIFETIME_MATCHDAYS,
            });
        }
    }

    /// Ver [`Command::RespondToOffer`].
    fn respond_to_offer(
        &mut self,
        id: u32,
        accept: bool,
    ) -> Result<Option<world::OfferOutcome>, AppError> {
        let club = self.manager.as_ref().ok_or(AppError::NoClubTaken)?.club;
        if self.live.is_some() {
            return Err(AppError::MatchInProgress);
        }
        let position = self
            .incoming
            .iter()
            .position(|o| o.id == id)
            .ok_or(AppError::UnknownOffer)?;
        if !accept {
            self.incoming.remove(position);
            return Ok(None);
        }
        let offer = self.incoming[position];
        if !self.window_info().open {
            return Err(AppError::TransferWindowClosed);
        }
        let buyer = offer.from;
        if self.budgets[buyer.as_usize()] < offer.fee {
            return Err(AppError::OfferNoLongerValid);
        }
        if !world::seller_can_part(&self.roster, club, offer.player) {
            return Err(AppError::SquadTooSmall);
        }
        let idx = offer.player.as_usize();
        self.budgets[buyer.as_usize()] = self.budgets[buyer.as_usize()] - offer.fee;
        self.budgets[club.as_usize()] = self.budgets[club.as_usize()] + offer.fee;
        self.roster[idx].club = buyer;
        self.contracts[idx] = world::fresh_contract(self.roster[idx].ability.current());
        world::mark_transferred(&mut self.seasons_since_transfer, &[offer.player]);
        self.incoming.retain(|o| o.player != offer.player);
        Ok(Some(world::OfferOutcome::Accepted))
    }

    fn market_value_of(&self, idx: usize) -> domain::Money {
        let state = &self.roster[idx];
        ai::market_value(
            state.ability.current(),
            self.contracts[idx].seasons_remaining,
            state.age_years.clamp(0, 255) as u8,
            state.ability.potential(),
            self.club_reputation[state.club.as_usize()],
        )
    }

    fn search(&self, filter: &PlayerFilter, offset: u32, limit: u16) -> SearchPage {
        let mut hits: Vec<usize> = (0..self.roster.len())
            .filter(|&i| {
                let p = &self.roster[i];
                filter.position.is_none_or(|pos| p.position == pos)
                    && p.ability.current() >= filter.min_ability
                    && filter
                        .max_age
                        .is_none_or(|max| p.age_years <= i32::from(max))
                    && filter.not_from_club != Some(p.club)
                    && filter
                        .max_value
                        .is_none_or(|max| self.market_value_of(i) <= max)
            })
            .collect();
        hits.sort_by(|&a, &b| {
            self.roster[b]
                .ability
                .current()
                .cmp(&self.roster[a].ability.current())
                .then(a.cmp(&b))
        });
        let total = hits.len() as u32;
        let rows = hits
            .into_iter()
            .skip(offset as usize)
            .take(usize::from(limit.clamp(1, MAX_SEARCH_PAGE)))
            .map(|i| self.player_row(i))
            .collect();
        SearchPage { total, rows }
    }

    fn incoming_offer_rows(&self) -> Vec<IncomingOfferRow> {
        let next = self.season.as_ref().map_or(0, |a| a.run.next_matchday());
        self.incoming
            .iter()
            .map(|o| {
                let state = &self.roster[o.player.as_usize()];
                IncomingOfferRow {
                    id: o.id,
                    player: self.player_ref(o.player),
                    position: state.position,
                    current_ability: state.ability.current(),
                    from: o.from,
                    from_name: self.pack.club(o.from).name.clone(),
                    fee: o.fee,
                    market_value: self.player_row(o.player.as_usize()).market_value,
                    expires_in_matchdays: o.expires_matchday.saturating_sub(next),
                }
            })
            .collect()
    }

    /// Joga uma rodada; se a temporada ainda não começou, faz a
    /// pré-temporada antes, e se a rodada foi a última, o fechamento depois.
    fn advance_round(&mut self) -> CommandReceipt {
        let mut receipt = CommandReceipt::default();
        let mut active = match self.season.take() {
            Some(active) => active,
            None => self.begin_season(&mut receipt),
        };
        receipt.season_index = active.run.season_index();
        receipt.matchdays_total = active.run.total_matchdays();

        let external = self.play_managed_fixture(&mut active);
        let report = {
            let mut career = career_round!(self);
            active
                .run
                .commit_matchday(&self.pack, &mut career, self.world_seed, external)
        };
        receipt.matchday = report.matchday;
        receipt.matches_played = report.matches_played;
        receipt.suspensions = report.suspensions;
        receipt.lineup_adjustments = report.lineup_adjustments.len();
        receipt.match_day_revenue = report.match_day_revenue;

        if active.run.is_finished() {
            self.end_season(active, &mut receipt);
            receipt.season_completed = true;
        } else {
            self.season = Some(active);
        }
        self.refresh_offers();
        receipt
    }

    /// Pré-temporada: tudo que acontece uma vez, antes da primeira rodada.
    fn begin_season(&mut self, receipt: &mut CommandReceipt) -> ActiveSeason {
        let season_index = self.history.len() as u32;
        // Dívida acumulada antes de qualquer outro gasto
        // (`world::repay_debt`, `docs/01 §2.2` RF-CL-03) — o que
        // sobrou de temporadas anteriores sem caixa pra pagar a
        // folha (`self.debt`) é pago primeiro, igual contabilidade
        // de verdade.
        receipt.debt_repaid = world::repay_debt(&mut self.budgets, &mut self.debt);

        // Folha salarial: cada clube paga o elenco inteiro pelo CA
        // travado no contrato de cada um (`world::pay_salaries`,
        // `docs/01 §2.2` RF-CL-02) — só então o que sobrar no
        // orçamento entra no mercado. Nunca deixa um orçamento
        // negativo (zera); o que não coube vira dívida em
        // `self.debt`, paga na temporada seguinte (acima).
        receipt.payroll_paid = world::pay_salaries(
            &self.roster,
            &self.contracts,
            &mut self.budgets,
            &mut self.debt,
        );

        // Mercado antes da temporada: clubes tentam substituir seu
        // titular mais fraco por um reserva melhor de outro clube
        // que caiba no orçamento (`world::run_market_day`,
        // `docs/07-roadmap.md` M1: "primeira IA de mercado"). Feito
        // antes de recalcular a força, pra quem comprou reforço já
        // jogar mais forte nesta mesma temporada. Todo jogador
        // transferido assina contrato novo no comprador (duração
        // máxima, salário no CA do momento da venda); jogador com
        // `self.loyalty` acima do limiar nunca é candidato a saída,
        // e `self.ambition` recusa um passo atrás de
        // `self.club_reputation` (RF-JG-02) — ver
        // `world::run_market_day`.
        let transfers = world::run_market_day(
            &mut self.roster,
            &mut self.contracts,
            &self.loyalty,
            &self.ambition,
            &self.club_reputation,
            &mut self.budgets,
            self.manager.as_ref().map(|m| m.club),
        );
        receipt.transfers = transfers.len();
        // Quem acabou de ser transferido volta a sofrer o malus de
        // adaptação (RF-JG-02, `world::adaptation`) por até duas
        // temporadas, mesmo que já tivesse se adaptado a um clube
        // anterior.
        let transferred_players: Vec<domain::PlayerId> =
            transfers.iter().map(|t| t.player).collect();
        world::mark_transferred(&mut self.seasons_since_transfer, &transferred_players);

        // Lesões da temporada: substitui completamente o estado
        // anterior (`world::roll_injuries`), então um jogador
        // recuperado não some do roster — só volta a ser elegível
        // pra escalação. Chance varia por jogador conforme
        // `self.injury_proneness` (RF-JG-02, `world::injuries`).
        world::roll_injuries(
            &mut self.roster,
            &self.injury_proneness,
            self.world_seed,
            season_index,
        );
        receipt.injuries = self.roster.iter().filter(|p| p.injured).count();

        // Recalcula o perfil a partir do roster atual — reflete
        // progressão (`world::advance_season`), as transferências que
        // acabaram de acontecer e as lesões desta temporada (jogador
        // machucado nunca titulariza, ver `world::strength_from_roster`/
        // `world::profile_from_roster`). Em `season_index == 0` sem
        // transferência nem lesão, é idêntico ao que `Self::new` já
        // calculou. Copa usa este instantâneo a temporada inteira.
        self.profiles =
            world::generate_match_profiles_from_roster(&self.pack, &self.roster, self.world_seed);
        // Titulares desta temporada — a mesma seleção que o cálculo de
        // perfil acima acabou de fazer clube a clube, guardada à parte
        // porque `world::apply_season_fatigue` (no fechamento) precisa
        // saber exatamente quem jogou, não só o perfil agregado do time.
        let starters = world::starters_from_roster_all_clubs(&self.pack, &self.roster);

        ActiveSeason {
            run: world::SeasonRun::start(&self.pack, &self.membership, season_index),
            starters,
        }
    }

    /// Fechamento depois da última rodada.
    fn end_season(&mut self, active: ActiveSeason, receipt: &mut CommandReceipt) {
        let ActiveSeason { run, mut starters } = active;
        let season_index = run.season_index();
        // Para o desgaste e a evolução do fim da temporada, o clube do
        // técnico conta o onze que ele de fato escalou (na formação e
        // escalação atuais), não o onze automático da pré-temporada.
        // Aproximação: sem contar minutos por jogador ao longo da
        // temporada (fadiga acumulada de verdade é RF-JG-07, F7).
        if let Some(manager) = &self.manager {
            starters.retain(|id| self.roster[id.as_usize()].club != manager.club);
            let xi = world::resolve_xi(&self.roster, &manager.setup, &HashSet::new());
            starters.extend(xi.players());
        }
        let result = {
            let career = career_round!(self);
            run.finish(&self.pack, &self.profiles, Some(&career), self.world_seed)
        };
        receipt.movements = result.movements.len();

        for &(club, _from, to) in &result.movements {
            self.membership[club.as_usize()] = to;
        }

        // Premiação e TV de fim de temporada
        // (`world::distribute_prize_money`/`distribute_tv_revenue`,
        // `docs/01 §2.2` RF-CL-03, segunda e terceira fatia): dois
        // pools fixos por competição de liga, um dividido por
        // posição final, outro por reputação — `result.tables` só
        // tem entrada pra competição de liga que de fato rodou
        // (copa não tem tabela, ver `world::season::SeasonResult`).
        for (_competition, table) in &result.tables {
            receipt.prize_money_paid =
                receipt.prize_money_paid + world::distribute_prize_money(table, &mut self.budgets);
            let clubs: Vec<domain::ClubId> = table.iter().map(|row| row.club).collect();
            receipt.tv_revenue_paid = receipt.tv_revenue_paid
                + world::distribute_tv_revenue(&clubs, &self.club_reputation, &mut self.budgets);
        }

        // Patrocínio de fim de temporada (`world::pay_sponsorship`,
        // RF-CL-03, quinta fatia): valor absoluto por clube do
        // pack inteiro, não por competição — por isso fica fora
        // do loop de tabelas acima.
        receipt.sponsorship_paid = world::pay_sponsorship(&self.club_reputation, &mut self.budgets);

        // Condição de entrada na próxima temporada
        // (`world::apply_season_fatigue`, `docs/01 §2.1` RF-JG-07,
        // fatia mínima): titular que jogou muita partida de liga
        // rende um pouco menos na temporada seguinte, reserva entra
        // descansado. Só partidas de liga contam — `result.tables`
        // nunca inclui copa (`world::cup`, `SeasonResult::cups`).
        let matches_played_by_club: HashMap<domain::ClubId, u32> = result
            .tables
            .iter()
            .flat_map(|(_, table)| table.iter().map(|row| (row.club, row.played)))
            .collect();
        world::apply_season_fatigue(&mut self.roster, &starters, &matches_played_by_club);
        receipt.tired_players = self
            .roster
            .iter()
            .filter(|p| p.condition < world::FULL_CONDITION)
            .count();

        world::advance_season(
            &mut self.roster,
            &starters,
            &self.professionalism,
            self.world_seed,
            season_index,
        );

        // Duração de contrato (`world::advance_contracts`, `docs/01
        // §2.2` RF-CL-02): decrementa a duração restante de todo
        // contrato; quem chega a zero renova automaticamente com o
        // CA pós-progressão desta temporada (por isso vem depois
        // de `world::advance_season`, acima).
        receipt.contracts_renewed = world::advance_contracts(
            &mut self.contracts,
            &self.roster,
            self.world_seed,
            season_index,
        );

        // Fecha a contabilidade de adaptação da temporada: o loop
        // por rodada já leu `self.seasons_since_transfer` de
        // quem foi transferido nesta mesma temporada (contando
        // como `0`); agora todo mundo avança 1, preparando a
        // contagem pra próxima (`world::adaptation`, RF-JG-02).
        world::advance_seasons_since_transfer(&mut self.seasons_since_transfer);

        self.history.push(result);
        self.last_run = Some(run);
    }

    /// Responde uma consulta — nunca muda estado (`docs/02 §4`).
    #[must_use]
    pub fn query(&self, query: Query) -> QueryResult {
        match query {
            Query::Competitions => QueryResult::Competitions(self.competitions()),
            Query::Standings { competition } => QueryResult::Standings(self.standings(competition)),
            Query::CupChampion { competition } => {
                QueryResult::CupChampion(self.cup_champion(competition))
            }
            Query::CurrentSeason => QueryResult::CurrentSeason(self.history.len() as u32),
            Query::SeasonProgress => QueryResult::SeasonProgress(self.season_progress()),
            Query::Manager => QueryResult::Manager(self.manager_info()),
            Query::Clubs => QueryResult::Clubs(self.clubs()),
            Query::Club { club } => QueryResult::Club(self.club_detail(club)),
            Query::Squad { club } => QueryResult::Squad(self.squad(club)),
            Query::Player { player } => QueryResult::Player(self.player_detail(player)),
            Query::Fixtures { club } => QueryResult::Fixtures(self.fixtures(club)),
            Query::RoundResults { matchday } => {
                QueryResult::RoundResults(self.round_results(matchday))
            }
            Query::Lineup => QueryResult::Lineup(self.lineup_info()),
            Query::LiveMatch { lines_from } => QueryResult::LiveMatch(
                self.live
                    .as_ref()
                    .map(|game| self.match_view(&game.report(), lines_from, Some(game))),
            ),
            Query::TransferWindow => QueryResult::TransferWindow(self.window_info()),
            Query::IncomingOffers => QueryResult::IncomingOffers(self.incoming_offer_rows()),
            Query::Search {
                filter,
                offset,
                limit,
            } => QueryResult::Search(self.search(&filter, offset, limit)),
            Query::LastMatch => QueryResult::LastMatch(
                self.last_match
                    .as_ref()
                    .map(|report| self.match_view(report, 0, None)),
            ),
        }
    }

    /// Exige um clube sob comando.
    fn manager_mut(&mut self) -> Result<&mut Manager, AppError> {
        self.manager.as_mut().ok_or(AppError::NoClubTaken)
    }

    fn set_formation(&mut self, formation: ai::Formation) -> Result<(), AppError> {
        if self.live.is_some() {
            return Err(AppError::MatchInProgress);
        }
        let setup = &mut self.manager_mut()?.setup;
        setup.formation = formation;
        setup.starters = None;
        Ok(())
    }

    fn set_instructions(
        &mut self,
        mentality: world::Mentality,
        pressing: world::Pressing,
    ) -> Result<(), AppError> {
        let manager = self.manager_mut()?;
        manager.setup.mentality = mentality;
        manager.setup.pressing = pressing;
        // Com a partida ao vivo, vale do próximo minuto em diante.
        if let Some(game) = self.live.as_mut()
            && let Some(side) = game.managed_side()
        {
            game.set_instructions(&self.pack, &self.roster, side, mentality, pressing);
        }
        Ok(())
    }

    /// Valida e registra uma escalação manual (ver [`Command::SetLineup`]).
    fn set_lineup(
        &mut self,
        formation: ai::Formation,
        starters: &[domain::PlayerId],
    ) -> Result<(), AppError> {
        let club = self.manager.as_ref().ok_or(AppError::NoClubTaken)?.club;
        if self.live.is_some() {
            return Err(AppError::MatchInProgress);
        }
        if starters.len() != 11 {
            return Err(AppError::InvalidLineup(
                "a escalação precisa de exatamente 11 jogadores",
            ));
        }
        let mut seen = HashSet::with_capacity(11);
        for &id in starters {
            if !seen.insert(id) {
                return Err(AppError::InvalidLineup("jogador repetido na escalação"));
            }
            let Some(state) = self.roster.get(id.as_usize()) else {
                return Err(AppError::InvalidLineup("jogador inexistente"));
            };
            if state.club != club {
                return Err(AppError::InvalidLineup("jogador de outro clube"));
            }
            if state.injured {
                return Err(AppError::InvalidLineup("jogador lesionado"));
            }
            if self.season.as_ref().is_some_and(|a| a.run.is_suspended(id)) {
                return Err(AppError::InvalidLineup(
                    "jogador suspenso na próxima rodada",
                ));
            }
        }
        let setup = &mut self.manager_mut()?.setup;
        setup.formation = formation;
        setup.starters = Some(starters.to_vec());
        Ok(())
    }

    /// Quem está suspenso na próxima rodada, entre os jogadores de `club`.
    fn suspended_of(&self, club: domain::ClubId) -> HashSet<domain::PlayerId> {
        match &self.season {
            Some(active) => self.suspended_in(&active.run, club),
            None => HashSet::new(),
        }
    }

    fn suspended_in(
        &self,
        run: &world::SeasonRun,
        club: domain::ClubId,
    ) -> HashSet<domain::PlayerId> {
        self.roster
            .iter()
            .filter(|p| p.club == club && run.is_suspended(p.player))
            .map(|p| p.player)
            .collect()
    }

    fn lineup_info(&self) -> Option<LineupInfo> {
        let manager = self.manager.as_ref()?;
        let setup = &manager.setup;
        let xi = world::resolve_xi(&self.roster, setup, &self.suspended_of(manager.club));
        let slots = xi
            .slots
            .iter()
            .map(|&(slot_position, player)| {
                let state = &self.roster[player.as_usize()];
                let resolved = &self.pack.players[player.as_usize()];
                LineupSlot {
                    slot_position,
                    player,
                    name: format!("{} {}", resolved.first_name, resolved.last_name),
                    natural_position: state.position,
                    condition: state.condition,
                    fit_permille: world::position_fit_permille(state.position, slot_position),
                }
            })
            .collect();
        Some(LineupInfo {
            formation: setup.formation,
            manual: setup.starters.is_some(),
            mentality: setup.mentality,
            pressing: setup.pressing,
            slots,
            adjustments: xi.adjustments,
        })
    }

    /// A temporada a consultar: a em andamento ou, entre temporadas, a
    /// última concluída.
    fn queried_run(&self) -> Option<&world::SeasonRun> {
        self.season
            .as_ref()
            .map(|a| &a.run)
            .or(self.last_run.as_ref())
    }

    fn competition_name_of(&self, club: domain::ClubId) -> &str {
        &self.pack.competition(self.membership[club.as_usize()]).name
    }

    fn clubs(&self) -> Vec<ClubRow> {
        self.pack
            .clubs
            .iter()
            .map(|c| ClubRow {
                id: c.id,
                name: c.name.clone(),
                competition_name: self.competition_name_of(c.id).to_string(),
                reputation: self.club_reputation[c.id.as_usize()],
            })
            .collect()
    }

    fn club_detail(&self, club: domain::ClubId) -> Option<ClubDetail> {
        let resolved = self.pack.clubs.get(club.as_usize())?;
        let mut squad_size = 0u32;
        let mut payroll = domain::Money::ZERO;
        for (state, contract) in self.roster.iter().zip(&self.contracts) {
            if state.club == club {
                squad_size += 1;
                payroll = payroll + world::contract_salary(contract);
            }
        }
        Some(ClubDetail {
            id: club,
            name: resolved.name.clone(),
            nation_name: self.pack.nation(resolved.nation).name.clone(),
            competition_name: self.competition_name_of(club).to_string(),
            reputation: self.club_reputation[club.as_usize()],
            budget: self.budgets[club.as_usize()],
            debt: self.debt[club.as_usize()],
            stadium_capacity: self.stadium_capacities[club.as_usize()],
            squad_size,
            payroll_per_season: payroll,
        })
    }

    fn player_row(&self, idx: usize) -> PlayerRow {
        let state = &self.roster[idx];
        let contract = &self.contracts[idx];
        let resolved = &self.pack.players[idx];
        let age = state.age_years.clamp(0, 255) as u8;
        PlayerRow {
            id: state.player,
            club: state.club,
            name: format!("{} {}", resolved.first_name, resolved.last_name),
            position: state.position,
            age,
            current_ability: state.ability.current(),
            condition: state.condition,
            injured: state.injured,
            suspended: self
                .season
                .as_ref()
                .is_some_and(|a| a.run.is_suspended(state.player)),
            contract_seasons_remaining: contract.seasons_remaining,
            salary_per_season: world::contract_salary(contract),
            wage_permille: contract.wage_permille,
            market_value: ai::market_value(
                state.ability.current(),
                contract.seasons_remaining,
                age,
                state.ability.potential(),
                self.club_reputation[state.club.as_usize()],
            ),
        }
    }

    fn squad(&self, club: domain::ClubId) -> Option<Vec<PlayerRow>> {
        if club.as_usize() >= self.pack.clubs.len() {
            return None;
        }
        let position_order = |p: domain::Position| match p {
            domain::Position::Goalkeeper => 0u8,
            domain::Position::Defender => 1,
            domain::Position::Midfielder => 2,
            domain::Position::Forward => 3,
        };
        let mut rows: Vec<PlayerRow> = (0..self.roster.len())
            .filter(|&i| self.roster[i].club == club)
            .map(|i| self.player_row(i))
            .collect();
        rows.sort_by(|a, b| {
            position_order(a.position)
                .cmp(&position_order(b.position))
                .then(b.current_ability.cmp(&a.current_ability))
                .then(a.id.cmp(&b.id))
        });
        Some(rows)
    }

    fn player_detail(&self, player: domain::PlayerId) -> Option<PlayerDetail> {
        let idx = player.as_usize();
        let resolved = self.pack.players.get(idx)?;
        let row = self.player_row(idx);
        let attributes = domain::Attribute::ALL
            .iter()
            .map(|&attribute| AttributeValue {
                attribute,
                value: resolved.attributes.get(attribute),
            })
            .collect();
        Some(PlayerDetail {
            club_name: self.pack.club(row.club).name.clone(),
            nation_name: self.pack.nation(resolved.nation).name.clone(),
            row,
            attributes,
        })
    }

    fn fixtures(&self, club: domain::ClubId) -> Option<FixtureList> {
        if club.as_usize() >= self.pack.clubs.len() {
            return None;
        }
        let run = self.queried_run()?;
        let entries = run.fixtures_of(club);
        if entries.is_empty() {
            return None;
        }
        let rows = entries
            .into_iter()
            .map(|e| FixtureRow {
                matchday: e.matchday,
                competition_name: self.pack.competition(e.competition).name.clone(),
                home: e.fixture.home,
                home_name: self.pack.club(e.fixture.home).name.clone(),
                away: e.fixture.away,
                away_name: self.pack.club(e.fixture.away).name.clone(),
                score: e.score.map(|s| MatchScore {
                    home_goals: s.home_goals,
                    away_goals: s.away_goals,
                }),
            })
            .collect();
        Some(FixtureList {
            season_index: run.season_index(),
            rows,
        })
    }

    fn round_results(&self, matchday: Option<u32>) -> Option<RoundResults> {
        let run = self.queried_run()?;
        let matchday = match matchday {
            Some(m) => m,
            None => run.next_matchday().checked_sub(1)?,
        };
        let results = run.round_results(matchday);
        if results.is_empty() {
            return None;
        }
        let rows = results
            .into_iter()
            .map(|(competition, fixture, score)| ResultRow {
                competition_name: self.pack.competition(competition).name.clone(),
                home_name: self.pack.club(fixture.home).name.clone(),
                away_name: self.pack.club(fixture.away).name.clone(),
                score: MatchScore {
                    home_goals: score.home_goals,
                    away_goals: score.away_goals,
                },
            })
            .collect();
        Some(RoundResults {
            season_index: run.season_index(),
            matchday,
            rows,
        })
    }

    fn manager_info(&self) -> Option<ManagerInfo> {
        let manager = self.manager.as_ref()?;
        let club = self.pack.club(manager.club);
        Some(ManagerInfo {
            coach_name: manager.coach.name.clone(),
            coach_nation_name: self.pack.nation(manager.coach.nation).name.clone(),
            coach_reputation: manager.coach.reputation,
            club: manager.club,
            club_name: club.name.clone(),
            competition_name: self
                .pack
                .competition(self.membership[manager.club.as_usize()])
                .name
                .clone(),
        })
    }

    fn season_progress(&self) -> SeasonProgress {
        match &self.season {
            Some(active) => SeasonProgress {
                season_index: active.run.season_index(),
                in_progress: true,
                next_matchday: active.run.next_matchday(),
                total_matchdays: active.run.total_matchdays(),
            },
            None => SeasonProgress {
                season_index: self.history.len() as u32,
                in_progress: false,
                next_matchday: 0,
                total_matchdays: 0,
            },
        }
    }

    fn competitions(&self) -> Vec<CompetitionSummary> {
        self.pack
            .competitions
            .iter()
            .map(|comp| CompetitionSummary {
                id: comp.id,
                name: comp.name.clone(),
                nation_name: self.pack.nation(comp.nation).name.clone(),
                kind: match comp.format {
                    pack::Format::RoundRobin { .. } => CompetitionKind::League,
                    pack::Format::Knockout { .. } => CompetitionKind::Cup,
                },
            })
            .collect()
    }

    fn standings(&self, competition: CompetitionId) -> Option<Vec<StandingsRow>> {
        let latest = self.history.last()?;
        let (_, table) = latest.tables.iter().find(|(id, _)| *id == competition)?;

        Some(
            table
                .iter()
                .enumerate()
                .map(|(index, row)| StandingsRow {
                    position: index as u32 + 1,
                    club_name: self.pack.club(row.club).name.clone(),
                    played: row.played,
                    wins: row.wins,
                    draws: row.draws,
                    losses: row.losses,
                    goals_for: row.goals_for,
                    goals_against: row.goals_against,
                    points: row.points(),
                })
                .collect(),
        )
    }

    fn cup_champion(&self, competition: CompetitionId) -> Option<CupChampionRow> {
        let latest = self.history.last()?;
        let cup = latest.cups.iter().find(|c| c.competition == competition)?;
        Some(CupChampionRow {
            champion_club_name: self.pack.club(cup.champion).name.clone(),
            rounds_played: cup.rounds.len() as u32,
            matches_played: cup.rounds.iter().map(|r| r.len() as u32).sum(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ai::Formation;
    use std::path::PathBuf;
    use world::{MatchLineKind, Mentality, Pressing};

    fn example_pack_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/example-two-tier")
    }

    fn tier1_id(session: &GameSession) -> CompetitionId {
        match session.query(Query::Competitions) {
            QueryResult::Competitions(list) => {
                list.iter()
                    .find(|c| c.name.contains("Primeira"))
                    .unwrap()
                    .id
            }
            _ => unreachable!(),
        }
    }

    fn cup_id(session: &GameSession) -> CompetitionId {
        match session.query(Query::Competitions) {
            QueryResult::Competitions(list) => {
                list.iter().find(|c| c.name.contains("Copa")).unwrap().id
            }
            _ => unreachable!(),
        }
    }

    #[test]
    fn nova_sessao_comeca_na_temporada_zero_sem_tabela() {
        let session = GameSession::new(&example_pack_path(), 42).unwrap();
        assert_eq!(
            session.query(Query::CurrentSeason),
            QueryResult::CurrentSeason(0)
        );
    }

    #[test]
    fn pack_inexistente_da_erro_tipado_nao_panic() {
        let err = GameSession::new(Path::new("/tmp/nao-existe-de-verdade"), 42).unwrap_err();
        assert!(matches!(err, AppError::PackLoad(_)));
    }

    #[test]
    fn listar_competicoes_traz_nomes_resolvidos() {
        let session = GameSession::new(&example_pack_path(), 42).unwrap();
        let QueryResult::Competitions(list) = session.query(Query::Competitions) else {
            panic!("esperava QueryResult::Competitions");
        };
        assert_eq!(list.len(), 3); // 2 ligas + 1 copa (docs/07 M1)
        assert!(list.iter().all(|c| c.nation_name == "Estrelônia"));
    }

    #[test]
    fn tabela_e_none_antes_de_qualquer_temporada_rodar() {
        let session = GameSession::new(&example_pack_path(), 42).unwrap();
        let id = tier1_id(&session);
        assert_eq!(
            session.query(Query::Standings { competition: id }),
            QueryResult::Standings(None)
        );
    }

    #[test]
    fn avancar_temporada_produz_recibo_e_tabela_consultavel() {
        let mut session = GameSession::new(&example_pack_path(), 42).unwrap();
        let id = tier1_id(&session);

        let receipt = session.dispatch(Command::AdvanceSeason).unwrap();
        assert_eq!(receipt.season_index, 0);
        assert_eq!(receipt.matches_played, 56 * 2); // 2 competições, 56 jogos cada
        assert_eq!(receipt.movements, 4); // 2 promovem + 2 rebaixam, por competição = 4 no total

        assert_eq!(
            session.query(Query::CurrentSeason),
            QueryResult::CurrentSeason(1)
        );

        let QueryResult::Standings(Some(table)) =
            session.query(Query::Standings { competition: id })
        else {
            panic!("esperava tabela preenchida após avançar a temporada");
        };
        assert_eq!(table.len(), 8);
        assert_eq!(table[0].position, 1);
        // Tabela vem ordenada: posição 1 tem pontos >= posição 2, sempre.
        assert!(table[0].points >= table[1].points);
    }

    #[test]
    fn copa_produz_campeao_consultavel_sem_contar_na_calibracao_de_liga() {
        let mut session = GameSession::new(&example_pack_path(), 42).unwrap();
        let cup = cup_id(&session);
        let league = tier1_id(&session);

        let receipt = session.dispatch(Command::AdvanceSeason).unwrap();
        // 2 ligas (56 jogos cada) — a copa (15 jogos: 8+4+2+1) não entra
        // aqui, por design (`world::cup`, `docs/04 §4.1`).
        assert_eq!(receipt.matches_played, 56 * 2);

        let QueryResult::CupChampion(Some(champion)) =
            session.query(Query::CupChampion { competition: cup })
        else {
            panic!("esperava campeão de copa após avançar a temporada");
        };
        assert_eq!(champion.rounds_played, 4); // 16 clubes: oitavas/quartas/semi/final
        assert_eq!(champion.matches_played, 15); // 8+4+2+1
        assert!(!champion.champion_club_name.is_empty());

        // Uma liga não tem campeão de copa, e uma copa não tem tabela.
        assert_eq!(
            session.query(Query::CupChampion {
                competition: league
            }),
            QueryResult::CupChampion(None)
        );
        assert_eq!(
            session.query(Query::Standings { competition: cup }),
            QueryResult::Standings(None)
        );
    }

    #[test]
    fn vinte_temporadas_seguidas_nao_travam_e_preservam_os_invariantes_do_mundo() {
        // Portão de saída do M1 (`docs/07-roadmap.md` §2): "20 temporadas
        // seguidas sem divergência entre plataformas e com métricas... dentro
        // da tolerância". `golden verify` (docs/08 §3) já cobre "sem
        // divergência entre plataformas" e `calibrate --check` (docs/04 §4.2)
        // já cobre "métricas dentro da tolerância" — nenhum dos dois roda 20
        // temporadas pela fronteira real de carreira (`app::GameSession`,
        // mercado + lesões + condição + progressão + copa + liga, todos
        // juntos, não isolados uns dos outros). Este teste é o que de fato
        // exercita essa duração, e prova que nada degenera silenciosamente
        // ao longo do caminho: dinheiro sempre conservado, CA nunca passa do
        // potencial, elenco nunca cresce/encolhe, ligas nunca desbalanceiam,
        // copa sempre produz exatamente um campeão.
        let mut session = GameSession::new(&example_pack_path(), 2026).unwrap();
        let initial_roster_len = session.roster.len();
        let initial_money_total: i64 = session.budgets.iter().map(|b| b.cents()).sum();

        let QueryResult::Competitions(competitions) = session.query(Query::Competitions) else {
            unreachable!()
        };
        let mut initial_league_sizes: Vec<(CompetitionId, usize)> = Vec::new();

        for season in 0..20 {
            let receipt = session.dispatch(Command::AdvanceSeason).unwrap();
            assert!(
                receipt.matches_played > 0,
                "temporada {season} não simulou nenhuma partida"
            );

            if season == 0 {
                for comp in &competitions {
                    if comp.kind == CompetitionKind::League {
                        let QueryResult::Standings(Some(table)) = session.query(Query::Standings {
                            competition: comp.id,
                        }) else {
                            panic!("liga '{}' sem tabela após a 1ª temporada", comp.name);
                        };
                        initial_league_sizes.push((comp.id, table.len()));
                    }
                }
            }
        }

        assert_eq!(
            session.roster.len(),
            initial_roster_len,
            "elenco total não deveria crescer nem encolher em 20 temporadas"
        );
        for player in &session.roster {
            assert!(
                player.ability.current() <= player.ability.potential(),
                "jogador {:?} com CA acima do PA após 20 temporadas",
                player.player
            );
            assert!(
                player.condition <= world::FULL_CONDITION,
                "condição de {:?} acima do máximo",
                player.player
            );
            assert!(
                player.age_years > 0,
                "jogador {:?} com idade não-positiva",
                player.player
            );
        }

        // Transferência conserva dinheiro entre clubes, mas a folha
        // salarial (`world::pay_salaries`, RF-CL-02) é um gasto real que
        // sai da economia a cada temporada — o total só pode diminuir ao
        // longo de 20 temporadas, nunca aumentar nem ficar igual (todo
        // clube tem elenco com CA > 0, então todo clube paga algo).
        let final_money_total: i64 = session.budgets.iter().map(|b| b.cents()).sum();
        assert!(
            final_money_total < initial_money_total,
            "dinheiro total do mundo deveria diminuir (nunca aumentar) em 20 temporadas, por \
             causa da folha salarial — ficou igual ou maior, algo está conservando demais ou \
             criando dinheiro do nada"
        );

        for (competition, expected_size) in initial_league_sizes {
            let QueryResult::Standings(Some(table)) =
                session.query(Query::Standings { competition })
            else {
                panic!("liga {competition:?} sem tabela após 20 temporadas");
            };
            assert_eq!(
                table.len(),
                expected_size,
                "liga {competition:?} mudou de tamanho em 20 temporadas — promoção/rebaixamento desbalanceado"
            );
        }

        for comp in &competitions {
            if comp.kind == CompetitionKind::Cup {
                let QueryResult::CupChampion(Some(champion)) = session.query(Query::CupChampion {
                    competition: comp.id,
                }) else {
                    panic!("copa '{}' sem campeão após 20 temporadas", comp.name);
                };
                assert!(!champion.champion_club_name.is_empty());
            }
        }
    }

    #[test]
    fn avancar_varias_temporadas_e_deterministico() {
        let mut a = GameSession::new(&example_pack_path(), 7).unwrap();
        let mut b = GameSession::new(&example_pack_path(), 7).unwrap();
        let id = tier1_id(&a);

        for _ in 0..3 {
            a.dispatch(Command::AdvanceSeason).unwrap();
            b.dispatch(Command::AdvanceSeason).unwrap();
        }

        assert_eq!(
            a.query(Query::Standings { competition: id }),
            b.query(Query::Standings { competition: id })
        );
    }

    #[test]
    fn to_save_reflete_seed_e_comandos_despachados() {
        let mut session = GameSession::new(&example_pack_path(), 99).unwrap();
        session.dispatch(Command::AdvanceSeason).unwrap();
        session.dispatch(Command::AdvanceSeason).unwrap();

        let save = session.to_save();
        assert_eq!(save.pack_id, "example.two-tier");
        assert_eq!(save.world_seed, 99);
        assert_eq!(save.command_log, vec![1, 1], "dois AdvanceSeason");
    }

    #[test]
    fn avancar_temporadas_envelhece_o_elenco_e_muda_a_forca_dos_clubes() {
        // Teste de caixa branca (acessa campos privados — este módulo de
        // teste é filho de `session`): prova que a progressão
        // (`world::advance_season`) está de fato ligada ao dispatch, não só
        // existe isolada em `world`. Sem isto, seria fácil o CA do roster
        // nunca evoluir e ninguém perceber — as tabelas continuariam
        // plausíveis mesmo com a força de clube congelada.
        let mut session = GameSession::new(&example_pack_path(), 5).unwrap();
        let initial_ages: Vec<i32> = session.roster.iter().map(|p| p.age_years).collect();
        let initial_profiles = session.profiles.clone();

        for _ in 0..5 {
            session.dispatch(Command::AdvanceSeason).unwrap();
        }

        let final_ages: Vec<i32> = session.roster.iter().map(|p| p.age_years).collect();
        assert!(
            initial_ages
                .iter()
                .zip(&final_ages)
                .all(|(before, after)| *after == before + 5),
            "cada jogador deveria ter envelhecido exatamente 5 anos"
        );
        assert_ne!(
            initial_profiles, session.profiles,
            "perfil dos clubes deveria mudar após 5 temporadas de progressão"
        );
    }

    #[test]
    fn titulares_de_fato_desgastam_e_reporte_de_cansados_bate_com_o_roster() {
        // Caixa branca: prova que `world::apply_season_fatigue` está
        // ligado ao dispatch (não só existe isolada em `world`) — depois
        // de uma temporada inteira, pelo menos os titulares (que jogaram
        // partidas de liga de verdade) deveriam entrar na próxima com
        // condição abaixo de `FULL_CONDITION`.
        let mut session = GameSession::new(&example_pack_path(), 21).unwrap();
        assert!(
            session
                .roster
                .iter()
                .all(|p| p.condition == world::FULL_CONDITION),
            "ninguém deveria começar a carreira cansado"
        );

        let receipt = session.dispatch(Command::AdvanceSeason).unwrap();

        assert_eq!(
            receipt.tired_players,
            session
                .roster
                .iter()
                .filter(|p| p.condition < world::FULL_CONDITION)
                .count(),
            "receipt.tired_players deveria bater com o roster logo após o dispatch"
        );
        assert!(
            receipt.tired_players > 0,
            "esperava pelo menos um jogador cansado após uma temporada inteira de liga \
             (256 jogadores, 16 × 11 titulares no pack de exemplo)"
        );
    }

    #[test]
    fn suspensoes_de_fato_acontecem_ligadas_ao_dispatch() {
        // Caixa branca: prova que o caminho de carreira
        // (`world::season::run_season` com `roster: Some`, rodada a
        // rodada) está de fato ligado ao dispatch, não só existe isolado
        // em `world::season`/`world::discipline`. ~4% por time por
        // partida, 16 clubes de liga, 14 partidas cada: em 10 temporadas é
        // extremamente improvável nunca acontecer nenhuma se o sorteio
        // estivesse de fato rodando.
        let mut session = GameSession::new(&example_pack_path(), 55).unwrap();
        let mut total_suspensions = 0usize;
        for _ in 0..10 {
            let receipt = session.dispatch(Command::AdvanceSeason).unwrap();
            total_suspensions += receipt.suspensions;
        }
        assert!(
            total_suspensions > 0,
            "esperava pelo menos uma suspensão em 10 temporadas no pack de exemplo"
        );
    }

    #[test]
    fn folha_salarial_de_fato_debita_o_orcamento_e_bate_com_o_reporte() {
        // Caixa branca: prova que `world::pay_salaries` está ligado ao
        // dispatch — todo clube tem elenco com CA > 0, então o orçamento
        // total tem que cair estritamente a cada temporada, e o valor
        // reportado em `CommandReceipt::payroll_paid` tem que bater com
        // a queda observada no orçamento.
        let mut session = GameSession::new(&example_pack_path(), 3).unwrap();
        let budget_before: i64 = session.budgets.iter().map(|b| b.cents()).sum();

        let receipt = session.dispatch(Command::AdvanceSeason).unwrap();

        assert!(
            receipt.payroll_paid.cents() > 0,
            "esperava folha salarial positiva no pack de exemplo (256 jogadores com CA > 0)"
        );
        // `payroll_paid` é a folha debitada, `match_day_revenue`,
        // `prize_money_paid`, `tv_revenue_paid` e `sponsorship_paid` são as
        // receitas creditadas (`world::finance::match_day_revenue`/
        // `distribute_prize_money`/`distribute_tv_revenue`/
        // `pay_sponsorship`, RF-CL-03) — todas juntas explicam exatamente a
        // variação do orçamento total: transferências (`run_market_day`)
        // conservam dinheiro entre clubes, então não aparecem aqui.
        let budget_after: i64 = session.budgets.iter().map(|b| b.cents()).sum();
        assert_eq!(
            budget_before - budget_after,
            receipt.payroll_paid.cents()
                - receipt.match_day_revenue.cents()
                - receipt.prize_money_paid.cents()
                - receipt.tv_revenue_paid.cents()
                - receipt.sponsorship_paid.cents(),
            "a queda do orçamento total deveria ser exatamente folha paga menos bilheteria, \
             premiação, TV e patrocínio recebidos, já que transferências conservam dinheiro \
             entre clubes"
        );
    }

    #[test]
    fn divida_de_fato_se_acumula_e_e_paga_na_temporada_seguinte_ligada_ao_dispatch() {
        // Caixa branca: prova que `world::pay_salaries`/`world::repay_debt`
        // estão de fato ligados ao dispatch, não só isolados em
        // `world::finance` — zera o caixa do clube 0 antes da 1ª
        // temporada, força a folha a faltar caixa, e confirma que vira
        // dívida reportada (não evapora); na 2ª temporada, a dívida é
        // debitada do caixa antes da folha, e `debt_repaid` reporta isso.
        let mut session = GameSession::new(&example_pack_path(), 11).unwrap();
        session.budgets[0] = domain::Money::ZERO;

        let receipt1 = session.dispatch(Command::AdvanceSeason).unwrap();
        assert_eq!(
            receipt1.debt_repaid,
            domain::Money::ZERO,
            "não deveria haver dívida nenhuma pra pagar antes da 1ª folha"
        );
        assert!(
            session.debt[0].cents() > 0,
            "clube sem caixa deveria ter acumulado dívida ao pagar a folha"
        );

        let debt_before_season2 = session.debt[0];
        let receipt2 = session.dispatch(Command::AdvanceSeason).unwrap();
        assert!(
            receipt2.debt_repaid.cents() > 0,
            "receita de bilheteria/premiação da 1ª temporada deveria ter sobrado caixa pra \
             pagar ao menos parte da dívida na 2ª"
        );
        assert!(
            session.debt[0] < debt_before_season2,
            "dívida deveria ter diminuído depois de ser paga"
        );
    }

    #[test]
    fn bilheteria_de_fato_credita_o_orcamento_ligada_ao_dispatch() {
        // Caixa branca: prova que `world::finance::match_day_revenue` está
        // de fato ligado ao dispatch (não só existe isolado em
        // `world::season`/`world::finance`) — toda temporada tem partidas
        // de liga em casa pra todo clube, então a bilheteria reportada
        // nunca pode ser zero.
        let mut session = GameSession::new(&example_pack_path(), 8).unwrap();
        let receipt = session.dispatch(Command::AdvanceSeason).unwrap();
        assert!(
            receipt.match_day_revenue.cents() > 0,
            "esperava bilheteria positiva no pack de exemplo (16 clubes de liga, 112 partidas)"
        );
    }

    #[test]
    fn contratos_de_fato_renovam_ligados_ao_dispatch() {
        // Caixa branca: prova que `world::advance_contracts` está de fato
        // ligado ao dispatch — duração inicial sorteada entre 1 e 5
        // temporadas pra 256 jogadores, em 10 temporadas é extremamente
        // improvável que nenhum contrato chegue a zero e renove.
        let mut session = GameSession::new(&example_pack_path(), 21).unwrap();
        let mut total_renewed = 0usize;
        for _ in 0..10 {
            let receipt = session.dispatch(Command::AdvanceSeason).unwrap();
            total_renewed += receipt.contracts_renewed;
        }
        assert!(
            total_renewed > 0,
            "esperava pelo menos uma renovação de contrato em 10 temporadas no pack de exemplo"
        );
    }

    #[test]
    fn mercado_de_transferencias_de_fato_move_jogadores_entre_clubes() {
        // Outro teste de caixa branca: prova que `world::run_market_day`
        // está ligado ao dispatch e produz transferências de verdade no
        // pack de exemplo (256 jogadores reais, `docs/07-roadmap.md` M1) —
        // não só que a função isolada funciona (isso já é testado em
        // `ai::market` e `world::market`).
        let mut session = GameSession::new(&example_pack_path(), 11).unwrap();
        let initial_clubs: Vec<domain::ClubId> = session.roster.iter().map(|p| p.club).collect();
        let initial_budgets = session.budgets.clone();

        let mut total_transfers = 0usize;
        for _ in 0..5 {
            let receipt = session.dispatch(Command::AdvanceSeason).unwrap();
            total_transfers += receipt.transfers;
        }

        assert!(
            total_transfers > 0,
            "esperava pelo menos uma transferência em 5 temporadas de mercado"
        );
        let final_clubs: Vec<domain::ClubId> = session.roster.iter().map(|p| p.club).collect();
        assert_ne!(
            initial_clubs, final_clubs,
            "pelo menos um jogador deveria ter mudado de clube"
        );
        assert_ne!(
            initial_budgets, session.budgets,
            "orçamentos deveriam ter mudado com as transferências"
        );
        // Transferência em si conserva dinheiro (mesma propriedade de
        // `ai::market::proptests::dinheiro_total_e_conservado`), mas o
        // total do mundo não fica mais igual depois de 5 temporadas: a
        // folha salarial (`world::pay_salaries`, RF-CL-02) drena dinheiro
        // da economia a cada uma — só pode diminuir, nunca aumentar.
        let total_before: i64 = initial_budgets.iter().map(|b| b.cents()).sum();
        let total_after: i64 = session.budgets.iter().map(|b| b.cents()).sum();
        assert!(
            total_after < total_before,
            "folha salarial de 5 temporadas deveria ter drenado algum dinheiro da economia"
        );
    }

    #[test]
    fn lesoes_de_fato_acontecem_e_excluem_jogador_da_escalacao() {
        // Caixa branca: prova que `world::roll_injuries` está ligado ao
        // dispatch (256 jogadores reais, ~6% de chance/temporada — esperar
        // zero lesões em 5 temporadas seria ~1 em milhares) e que o
        // CommandReceipt reporta a contagem certa.
        let mut session = GameSession::new(&example_pack_path(), 13).unwrap();

        let mut saw_injury = false;
        for _ in 0..5 {
            let receipt = session.dispatch(Command::AdvanceSeason).unwrap();
            assert_eq!(
                receipt.injuries,
                session.roster.iter().filter(|p| p.injured).count(),
                "receipt.injuries deveria bater com o roster logo após o dispatch"
            );
            if receipt.injuries > 0 {
                saw_injury = true;
            }
        }
        assert!(
            saw_injury,
            "esperava pelo menos uma lesão em 5 temporadas no pack de exemplo"
        );
    }

    #[test]
    fn load_a_partir_do_save_reproduz_a_mesma_tabela_por_replay() {
        let mut original = GameSession::new(&example_pack_path(), 7).unwrap();
        let id = tier1_id(&original);
        for _ in 0..3 {
            original.dispatch(Command::AdvanceSeason).unwrap();
        }
        let save = original.to_save();

        let reloaded = GameSession::load(&example_pack_path(), &save).unwrap();

        assert_eq!(
            reloaded.query(Query::CurrentSeason),
            original.query(Query::CurrentSeason)
        );
        assert_eq!(
            reloaded.query(Query::Standings { competition: id }),
            original.query(Query::Standings { competition: id })
        );
    }

    #[test]
    fn save_to_path_e_load_from_path_fazem_round_trip_em_disco() {
        let dir =
            std::env::temp_dir().join(format!("managerfc-app-save-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let save_path = dir.join("slot0.cm0304save");

        let mut original = GameSession::new(&example_pack_path(), 3).unwrap();
        let id = tier1_id(&original);
        original.dispatch(Command::AdvanceSeason).unwrap();
        original.save_to_path(&save_path).unwrap();

        let reloaded = GameSession::load_from_path(&example_pack_path(), &save_path).unwrap();
        assert_eq!(
            reloaded.query(Query::Standings { competition: id }),
            original.query(Query::Standings { competition: id })
        );

        std::fs::remove_dir_all(&dir).ok();
    }

    fn progress(session: &GameSession) -> SeasonProgress {
        match session.query(Query::SeasonProgress) {
            QueryResult::SeasonProgress(p) => p,
            other => panic!("resposta inesperada: {other:?}"),
        }
    }

    #[test]
    fn advance_round_joga_uma_rodada_por_vez_e_fecha_a_temporada_na_ultima() {
        let mut session = GameSession::new(&example_pack_path(), 42).unwrap();
        assert_eq!(
            progress(&session),
            SeasonProgress {
                season_index: 0,
                in_progress: false,
                next_matchday: 0,
                total_matchdays: 0
            }
        );

        let first = session.dispatch(Command::AdvanceRound).unwrap();
        assert_eq!(first.matchday, 0);
        assert_eq!(first.matchdays_total, 14);
        assert!(!first.season_completed);
        assert_eq!(first.matches_played, 8, "4 jogos por liga, 2 ligas");
        assert!(first.payroll_paid.cents() > 0, "pré-temporada no 1º recibo");
        assert_eq!(first.movements, 0, "fechamento só no último recibo");
        assert_eq!(
            progress(&session),
            SeasonProgress {
                season_index: 0,
                in_progress: true,
                next_matchday: 1,
                total_matchdays: 14
            }
        );
        assert!(
            matches!(
                session.query(Query::CurrentSeason),
                QueryResult::CurrentSeason(0)
            ),
            "temporada só conta como concluída no fim"
        );

        for expected in 1..13 {
            let mid = session.dispatch(Command::AdvanceRound).unwrap();
            assert_eq!(mid.matchday, expected);
            assert!(!mid.season_completed);
            assert_eq!(mid.payroll_paid, domain::Money::ZERO, "folha só no 1º");
            assert_eq!(mid.prize_money_paid, domain::Money::ZERO);
        }

        let last = session.dispatch(Command::AdvanceRound).unwrap();
        assert_eq!(last.matchday, 13);
        assert!(last.season_completed);
        assert!(last.prize_money_paid.cents() > 0, "fechamento no último");
        assert!(last.sponsorship_paid.cents() > 0);
        assert_eq!(last.movements, 4);
        assert!(matches!(
            session.query(Query::CurrentSeason),
            QueryResult::CurrentSeason(1)
        ));
        assert_eq!(
            progress(&session),
            SeasonProgress {
                season_index: 1,
                in_progress: false,
                next_matchday: 0,
                total_matchdays: 0
            }
        );
    }

    #[test]
    fn advance_season_e_a_soma_exata_de_advance_round_ate_o_fim() {
        // Garantia central de F1: parar entre rodadas não muda o mundo nem
        // a contabilidade — o recibo de `AdvanceSeason` é a soma dos
        // recibos das rodadas, e o estado final é o mesmo.
        let mut whole = GameSession::new(&example_pack_path(), 7).unwrap();
        let mut stepped = GameSession::new(&example_pack_path(), 7).unwrap();
        for _ in 0..3 {
            let a = whole.dispatch(Command::AdvanceSeason).unwrap();
            let mut sum = stepped.dispatch(Command::AdvanceRound).unwrap();
            while !sum.season_completed {
                let next = stepped.dispatch(Command::AdvanceRound).unwrap();
                sum.absorb(next);
            }
            assert_eq!(format!("{a:?}"), format!("{sum:?}"));
        }
        assert_eq!(whole.budgets, stepped.budgets);
        assert_eq!(whole.debt, stepped.debt);
        for comp in whole.pack.competitions.iter().map(|c| c.id) {
            assert_eq!(
                whole.query(Query::Standings { competition: comp }),
                stepped.query(Query::Standings { competition: comp })
            );
        }
    }

    #[test]
    fn advance_season_no_meio_da_temporada_completa_so_o_que_falta() {
        let mut session = GameSession::new(&example_pack_path(), 5).unwrap();
        for _ in 0..4 {
            session.dispatch(Command::AdvanceRound).unwrap();
        }
        let rest = session.dispatch(Command::AdvanceSeason).unwrap();
        assert!(rest.season_completed);
        assert_eq!(rest.matchday, 13);
        assert_eq!(rest.matches_played, 10 * 8, "10 rodadas restantes");
        assert_eq!(rest.payroll_paid, domain::Money::ZERO, "pré já foi paga");
    }

    #[test]
    fn save_e_load_no_meio_da_temporada_reabrem_no_mesmo_ponto() {
        let mut original = GameSession::new(&example_pack_path(), 11).unwrap();
        original.dispatch(Command::AdvanceSeason).unwrap();
        for _ in 0..6 {
            original.dispatch(Command::AdvanceRound).unwrap();
        }
        let save = original.to_save();
        let mut reloaded = GameSession::load(&example_pack_path(), &save).unwrap();
        assert_eq!(progress(&reloaded), progress(&original));
        assert_eq!(reloaded.budgets, original.budgets);

        // E os dois continuam idênticos dali em diante.
        let a = original.dispatch(Command::AdvanceSeason).unwrap();
        let b = reloaded.dispatch(Command::AdvanceSeason).unwrap();
        assert_eq!(format!("{a:?}"), format!("{b:?}"));
        assert_eq!(reloaded.budgets, original.budgets);
        assert_eq!(reloaded.to_save(), original.to_save());
    }

    fn coach(name: &str) -> CoachProfile {
        CoachProfile {
            name: name.to_string(),
            nation: domain::NationId::new(0),
            reputation: 30,
        }
    }

    fn take(session: &mut GameSession, club: u32, name: &str) -> Result<CommandReceipt, AppError> {
        session.dispatch(Command::TakeClub {
            club: domain::ClubId::new(club),
            coach: coach(name),
        })
    }

    fn manager(session: &GameSession) -> Option<ManagerInfo> {
        match session.query(Query::Manager) {
            QueryResult::Manager(m) => m,
            other => panic!("resposta inesperada: {other:?}"),
        }
    }

    #[test]
    fn take_club_registra_o_tecnico_e_a_query_manager_resolve_os_nomes() {
        let mut session = GameSession::new(&example_pack_path(), 42).unwrap();
        assert_eq!(manager(&session), None, "sem TakeClub, sessão observadora");

        let receipt = take(&mut session, 3, "Zé Treinador").unwrap();
        assert_eq!(receipt.matches_played, 0, "TakeClub não joga nada");
        let info = manager(&session).unwrap();
        assert_eq!(info.coach_name, "Zé Treinador");
        assert_eq!(info.coach_reputation, 30);
        assert_eq!(info.club, domain::ClubId::new(3));
        assert_eq!(info.club_name, session.pack.club(info.club).name);
        assert_eq!(info.coach_nation_name, "Estrelônia");
        assert_eq!(info.competition_name, "Primeira Divisão de Estrelônia");
    }

    #[test]
    fn take_club_recusa_pedidos_invalidos_sem_mudar_o_estado() {
        let mut session = GameSession::new(&example_pack_path(), 42).unwrap();
        assert!(matches!(
            take(&mut session, 999, "Zé"),
            Err(AppError::UnknownClub(_))
        ));
        assert!(matches!(
            take(&mut session, 0, "   "),
            Err(AppError::InvalidCoach("nome vazio"))
        ));
        assert!(matches!(
            take(&mut session, 0, &"x".repeat(MAX_COACH_NAME_BYTES + 1)),
            Err(AppError::InvalidCoach("nome longo demais"))
        ));
        let bad_nation = session.dispatch(Command::TakeClub {
            club: domain::ClubId::new(0),
            coach: CoachProfile {
                nation: domain::NationId::new(77),
                ..coach("Zé")
            },
        });
        assert!(matches!(bad_nation, Err(AppError::InvalidCoach(_))));
        let bad_rep = session.dispatch(Command::TakeClub {
            club: domain::ClubId::new(0),
            coach: CoachProfile {
                reputation: 101,
                ..coach("Zé")
            },
        });
        assert!(matches!(bad_rep, Err(AppError::InvalidCoach(_))));
        assert_eq!(manager(&session), None);
        assert!(
            session.to_save().command_log.is_empty(),
            "comando recusado nunca entra no log"
        );
    }

    #[test]
    fn take_club_so_vale_uma_vez_e_so_antes_da_primeira_rodada() {
        let mut session = GameSession::new(&example_pack_path(), 42).unwrap();
        take(&mut session, 0, "Zé").unwrap();
        assert!(matches!(
            take(&mut session, 1, "Outro"),
            Err(AppError::ClubAlreadyTaken)
        ));

        let mut late = GameSession::new(&example_pack_path(), 42).unwrap();
        late.dispatch(Command::AdvanceRound).unwrap();
        assert!(matches!(
            take(&mut late, 0, "Zé"),
            Err(AppError::CareerAlreadyStarted)
        ));
    }

    #[test]
    fn clube_do_tecnico_sai_do_mercado_automatico_e_o_resto_do_mundo_continua() {
        // Com técnico, a IA não compra nem vende pelo clube dele: o elenco
        // do clube só muda por decisão do usuário (que não houve aqui).
        let mut managed = GameSession::new(&example_pack_path(), 21).unwrap();
        take(&mut managed, 2, "Zé Treinador").unwrap();
        let before: Vec<_> = squad_of(&managed, 2).iter().map(|p| p.id).collect();
        let mut transfers_elsewhere = 0;
        for _ in 0..5 {
            transfers_elsewhere += managed.dispatch(Command::AdvanceSeason).unwrap().transfers;
        }
        let mut after: Vec<_> = squad_of(&managed, 2).iter().map(|p| p.id).collect();
        let mut before_sorted = before;
        before_sorted.sort();
        after.sort();
        assert_eq!(before_sorted, after, "elenco do técnico intacto");
        assert!(
            transfers_elsewhere > 0,
            "a IA segue negociando entre os demais"
        );
    }

    #[test]
    fn take_club_sobrevive_ao_save_e_load() {
        let mut managed = GameSession::new(&example_pack_path(), 21).unwrap();
        take(&mut managed, 2, "Zé Treinador").unwrap();
        managed.dispatch(Command::AdvanceSeason).unwrap();
        let reloaded = GameSession::load(&example_pack_path(), &managed.to_save()).unwrap();
        assert_eq!(manager(&reloaded), manager(&managed));
        assert_eq!(reloaded.to_save(), managed.to_save());
        assert_eq!(reloaded.budgets, managed.budgets);
    }

    fn live_view(session: &GameSession, lines_from: usize) -> Option<MatchView> {
        match session.query(Query::LiveMatch { lines_from }) {
            QueryResult::LiveMatch(v) => v,
            other => panic!("resposta inesperada: {other:?}"),
        }
    }

    fn last_match(session: &GameSession) -> Option<MatchView> {
        match session.query(Query::LastMatch) {
            QueryResult::LastMatch(v) => v,
            other => panic!("resposta inesperada: {other:?}"),
        }
    }

    #[test]
    fn partida_ao_vivo_comeca_parada_no_minuto_zero_com_onze_banco_e_tres_trocas() {
        let mut session = managed(3);
        assert!(live_view(&session, 0).is_none());
        let receipt = session.dispatch(Command::StartMatch).unwrap();
        assert!(
            receipt.payroll_paid.cents() > 0,
            "pré-temporada no StartMatch"
        );
        assert_eq!(receipt.matches_played, 0);

        let view = live_view(&session, 0).unwrap();
        assert_eq!((view.minute, view.finished), (0, false));
        assert_eq!((view.home_goals, view.away_goals), (0, 0));
        assert_eq!(view.on_pitch.len(), 11);
        assert!(!view.bench.is_empty());
        assert_eq!(view.substitutions_left, 3);
        assert_eq!(view.total_lines, 1, "só o apito inicial");
        assert!(view.managed_side.is_some());
        let managed_name = &session.pack.club(domain::ClubId::new(3)).name;
        assert!(&view.home_name == managed_name || &view.away_name == managed_name);
    }

    #[test]
    fn play_minutes_avanca_e_a_paginacao_do_relato_so_traz_o_novo() {
        let mut session = managed(3);
        session.dispatch(Command::StartMatch).unwrap();
        session
            .dispatch(Command::PlayMinutes { minutes: 30 })
            .unwrap();
        let first = live_view(&session, 0).unwrap();
        assert_eq!(first.minute, 30);
        assert_eq!(first.lines.len(), first.total_lines);

        session
            .dispatch(Command::PlayMinutes { minutes: 30 })
            .unwrap();
        let next = live_view(&session, first.total_lines).unwrap();
        assert_eq!(next.minute, 60);
        assert_eq!(next.lines.len(), next.total_lines - first.total_lines);
        assert!(
            live_view(&session, next.total_lines)
                .unwrap()
                .lines
                .is_empty()
        );

        session
            .dispatch(Command::PlayMinutes { minutes: 255 })
            .unwrap();
        let end = live_view(&session, 0).unwrap();
        assert!(end.finished && end.minute == 90);
        assert_eq!(
            end.lines.last().unwrap().kind,
            MatchLineKind::FullTime,
            "termina com o apito final"
        );
    }

    #[test]
    fn advance_round_termina_a_partida_ao_vivo_e_conclui_a_rodada() {
        let mut session = managed(3);
        session.dispatch(Command::StartMatch).unwrap();
        session
            .dispatch(Command::PlayMinutes { minutes: 20 })
            .unwrap();
        let receipt = session.dispatch(Command::AdvanceRound).unwrap();
        assert_eq!(receipt.matches_played, 8, "as outras partidas saem aqui");
        assert!(live_view(&session, 0).is_none());
        let last = last_match(&session).unwrap();
        assert!(last.finished);
        assert_eq!(progress(&session).next_matchday, 1);
        assert_eq!(round_results(&session, None).unwrap().rows.len(), 8);
    }

    #[test]
    fn ao_vivo_sem_intervencao_e_identico_ao_modo_instantaneo() {
        let mut instant = managed(3);
        let mut live = managed(3);
        for _ in 0..14 {
            instant.dispatch(Command::AdvanceRound).unwrap();

            live.dispatch(Command::StartMatch).unwrap();
            for chunk in [1u8, 44, 9, 20] {
                live.dispatch(Command::PlayMinutes { minutes: chunk })
                    .unwrap();
            }
            live.dispatch(Command::AdvanceRound).unwrap();
            assert_eq!(last_match(&instant), last_match(&live));
        }
        assert_eq!(instant.budgets, live.budgets);
        assert_eq!(
            format!("{:?}", instant.history),
            format!("{:?}", live.history)
        );
    }

    #[test]
    fn modo_instantaneo_tambem_gera_o_relatorio_da_partida() {
        let mut session = managed(3);
        assert!(last_match(&session).is_none());
        session.dispatch(Command::AdvanceRound).unwrap();
        let report = last_match(&session).unwrap();
        assert!(report.finished);
        assert_eq!(report.total_lines, report.lines.len());
        assert_eq!(report.on_pitch.len(), 0, "só ao vivo");
        assert!(
            report
                .players
                .iter()
                .all(|p| (30..=100).contains(&p.rating_x10))
        );
        assert!(report.players.iter().filter(|p| p.started).count() >= 22);
        let goals: u32 = report.players.iter().map(|p| u32::from(p.goals)).sum();
        assert_eq!(goals, report.home_goals + report.away_goals);

        let observer = GameSession::new(&example_pack_path(), 42).unwrap();
        assert!(last_match(&observer).is_none());
    }

    #[test]
    fn substituicao_ao_vivo_troca_banco_e_campo_e_recusas_nao_entram_no_log() {
        let mut session = managed(3);
        assert!(matches!(
            session.dispatch(Command::Substitute {
                out: domain::PlayerId::new(0),
                incoming: domain::PlayerId::new(1)
            }),
            Err(AppError::NoLiveMatch)
        ));
        session.dispatch(Command::StartMatch).unwrap();
        session
            .dispatch(Command::PlayMinutes { minutes: 50 })
            .unwrap();
        let view = live_view(&session, 0).unwrap();
        let out = view.on_pitch[5].clone();
        let incoming = view
            .bench
            .iter()
            .find(|b| b.natural_position == out.slot_position)
            .unwrap()
            .clone();
        let log_before = session.to_save().command_log;

        assert!(matches!(
            session.dispatch(Command::Substitute {
                out: incoming.player.id,
                incoming: out.player.id
            }),
            Err(AppError::InvalidSubstitution(_))
        ));
        assert_eq!(session.to_save().command_log, log_before);

        session
            .dispatch(Command::Substitute {
                out: out.player.id,
                incoming: incoming.player.id,
            })
            .unwrap();
        let after = live_view(&session, 0).unwrap();
        assert_eq!(after.substitutions_left, 2);
        assert!(
            after
                .on_pitch
                .iter()
                .any(|p| p.player.id == incoming.player.id)
        );
        assert!(
            after
                .bench
                .iter()
                .all(|p| p.player.id != incoming.player.id)
        );
        let sub_line = after
            .lines
            .iter()
            .find(|l| l.kind == MatchLineKind::Substitution)
            .unwrap();
        assert_eq!(sub_line.minute, 50);
        assert_eq!(sub_line.player.as_ref().unwrap().id, incoming.player.id);
        assert_eq!(sub_line.other.as_ref().unwrap().id, out.player.id);
    }

    #[test]
    fn durante_a_partida_ao_vivo_formacao_e_escalacao_travam_mas_instrucoes_valem() {
        let mut session = managed(3);
        session.dispatch(Command::StartMatch).unwrap();
        assert!(matches!(
            session.dispatch(Command::StartMatch),
            Err(AppError::MatchInProgress)
        ));
        assert!(matches!(
            session.dispatch(Command::SetFormation {
                formation: Formation::F433
            }),
            Err(AppError::MatchInProgress)
        ));
        let lineup: Vec<_> = live_view(&session, 0)
            .unwrap()
            .on_pitch
            .iter()
            .map(|p| p.player.id)
            .collect();
        assert!(matches!(
            session.dispatch(Command::SetLineup {
                formation: Formation::F442,
                starters: lineup
            }),
            Err(AppError::MatchInProgress)
        ));
        session
            .dispatch(Command::SetInstructions {
                mentality: Mentality::Attacking,
                pressing: Pressing::Normal,
            })
            .unwrap();
        let QueryResult::Lineup(Some(info)) = session.query(Query::Lineup) else {
            panic!()
        };
        assert_eq!(info.mentality, Mentality::Attacking);
    }

    #[test]
    fn start_match_exige_tecnico_e_play_minutes_exige_partida() {
        let mut observer = GameSession::new(&example_pack_path(), 42).unwrap();
        assert!(matches!(
            observer.dispatch(Command::StartMatch),
            Err(AppError::NoClubTaken)
        ));
        let mut session = managed(3);
        assert!(matches!(
            session.dispatch(Command::PlayMinutes { minutes: 5 }),
            Err(AppError::NoLiveMatch)
        ));
        let log = session.to_save().command_log;
        let mut pos = 0;
        assert!(matches!(
            Command::decode(&log, &mut pos),
            Ok(Command::TakeClub { .. })
        ));
        assert_eq!(pos, log.len(), "recusas não entram no log: só o TakeClub");
    }

    #[test]
    fn advance_season_no_meio_de_uma_partida_ao_vivo_completa_tudo() {
        let mut session = managed(3);
        session.dispatch(Command::StartMatch).unwrap();
        session
            .dispatch(Command::PlayMinutes { minutes: 10 })
            .unwrap();
        let receipt = session.dispatch(Command::AdvanceSeason).unwrap();
        assert!(receipt.season_completed);
        assert_eq!(receipt.matches_played, 112);
        assert!(live_view(&session, 0).is_none());
        assert!(matches!(
            session.query(Query::CurrentSeason),
            QueryResult::CurrentSeason(1)
        ));
    }

    #[test]
    fn salvar_no_meio_da_partida_ao_vivo_reabre_no_mesmo_minuto_e_continua_igual() {
        let mut original = managed(3);
        original.dispatch(Command::AdvanceRound).unwrap();
        original.dispatch(Command::StartMatch).unwrap();
        original
            .dispatch(Command::PlayMinutes { minutes: 37 })
            .unwrap();
        let view = live_view(&original, 0).unwrap();
        let sub = (
            view.on_pitch[2].player.id,
            view.bench
                .iter()
                .find(|b| b.natural_position == view.on_pitch[2].slot_position)
                .unwrap()
                .player
                .id,
        );
        original
            .dispatch(Command::Substitute {
                out: sub.0,
                incoming: sub.1,
            })
            .unwrap();

        let mut reloaded = GameSession::load(&example_pack_path(), &original.to_save()).unwrap();
        assert_eq!(live_view(&reloaded, 0), live_view(&original, 0));

        original.dispatch(Command::AdvanceSeason).unwrap();
        reloaded.dispatch(Command::AdvanceSeason).unwrap();
        assert_eq!(last_match(&original), last_match(&reloaded));
        assert_eq!(original.budgets, reloaded.budgets);
    }

    fn window(session: &GameSession) -> TransferWindowInfo {
        match session.query(Query::TransferWindow) {
            QueryResult::TransferWindow(w) => w,
            other => panic!("resposta inesperada: {other:?}"),
        }
    }

    /// Sessão com técnico no clube 3, janela de verão aberta e caixa
    /// farto — o cenário base das negociações.
    fn market_ready() -> GameSession {
        let mut session = managed(3);
        session.dispatch(Command::BeginSeason).unwrap();
        session.budgets[3] = domain::Money::from_cents(1_000_000_000_000);
        session
    }

    /// Um jogador comprável (de outro clube, nada de intocável).
    fn buyable(session: &GameSession) -> domain::PlayerId {
        (0..session.roster.len())
            .find(|&i| {
                let p = &session.roster[i];
                p.club != domain::ClubId::new(3)
                    && session.loyalty[i] < ai::LOYALTY_REFUSAL_THRESHOLD
                    && world::seller_can_part(&session.roster, p.club, p.player)
                    && session.ambition[i] < ai::AMBITION_REFUSAL_THRESHOLD
            })
            .map(|i| domain::PlayerId::new(i as u32))
            .expect("pack de exemplo tem jogador comprável")
    }

    fn offer(
        session: &mut GameSession,
        player: domain::PlayerId,
        fee: i64,
        wage: u16,
    ) -> Result<world::OfferOutcome, AppError> {
        let receipt = session.dispatch(Command::MakeOffer {
            player,
            fee: domain::Money::from_cents(fee),
            wage_permille: wage,
            seasons: 3,
        })?;
        Ok(receipt.offer.expect("MakeOffer sempre responde"))
    }

    #[test]
    fn janela_so_abre_com_a_temporada_e_fecha_e_reabre_ao_longo_dela() {
        let mut session = managed(3);
        assert_eq!(
            window(&session),
            TransferWindowInfo {
                open: false,
                matchdays_until_change: None
            }
        );
        let p = buyable(&session);
        assert!(matches!(
            offer(&mut session, p, 1, 1000),
            Err(AppError::TransferWindowClosed)
        ));

        let receipt = session.dispatch(Command::BeginSeason).unwrap();
        assert!(
            receipt.payroll_paid.cents() > 0,
            "pré-temporada no BeginSeason"
        );
        assert_eq!(receipt.matches_played, 0);
        assert!(matches!(
            session.dispatch(Command::BeginSeason),
            Err(AppError::SeasonAlreadyBegun)
        ));
        assert!(window(&session).open);
        assert_eq!(window(&session).matchdays_until_change, Some(4));

        for _ in 0..4 {
            session.dispatch(Command::AdvanceRound).unwrap();
        }
        assert_eq!(
            window(&session),
            TransferWindowInfo {
                open: false,
                matchdays_until_change: Some(3)
            },
            "fechada; abre de novo no meio da temporada (rodada 7)"
        );
        for _ in 0..3 {
            session.dispatch(Command::AdvanceRound).unwrap();
        }
        assert_eq!(
            window(&session),
            TransferWindowInfo {
                open: true,
                matchdays_until_change: Some(2)
            }
        );
        for _ in 0..2 {
            session.dispatch(Command::AdvanceRound).unwrap();
        }
        assert_eq!(
            window(&session),
            TransferWindowInfo {
                open: false,
                matchdays_until_change: None
            }
        );
    }

    #[test]
    fn begin_season_nao_repete_a_pre_temporada_na_primeira_rodada() {
        let mut session = managed(3);
        session.dispatch(Command::BeginSeason).unwrap();
        let first = session.dispatch(Command::AdvanceRound).unwrap();
        assert_eq!(first.payroll_paid, domain::Money::ZERO);
        assert_eq!(first.matches_played, 8);
    }

    #[test]
    fn negociacao_em_etapas_ofensa_contraproposta_salario_e_fechado() {
        use world::{OfferOutcome as O, Refusal};
        let mut session = market_ready();
        let p = buyable(&session);
        let seller = session.roster[p.as_usize()].club;
        let squad_before = squad_of(&session, seller.index()).len();
        let (buyer_budget, seller_budget) =
            (session.budgets[3], session.budgets[seller.as_usize()]);

        assert_eq!(
            offer(&mut session, p, 1, 3000).unwrap(),
            O::ClubRefuses(Refusal::InsultingOffer)
        );
        let state = session.roster[p.as_usize()];
        let value = ai::market_value(
            state.ability.current(),
            session.contracts[p.as_usize()].seasons_remaining,
            state.age_years as u8,
            state.ability.potential(),
            session.club_reputation[seller.as_usize()],
        );
        let eighty_percent = world::asking_fee(value).cents() * 80 / 100;
        let O::ClubWantsMore { asking } = offer(&mut session, p, eighty_percent, 3000).unwrap()
        else {
            panic!("fee intermediária deveria gerar contraproposta")
        };
        assert_eq!(asking, world::asking_fee(value));
        // Fee aceita, mas salário baixo: o jogador pede mais.
        let O::PlayerWantsMore {
            asking_wage_permille,
        } = offer(&mut session, p, asking.cents(), 500).unwrap()
        else {
            panic!("salário baixo deveria gerar pedido do jogador")
        };
        assert!(asking_wage_permille >= 1000);
        assert_eq!(
            session.roster[p.as_usize()].club,
            seller,
            "nada mudou ainda"
        );
        assert_eq!(session.budgets[3], buyer_budget);

        let outcome = offer(&mut session, p, asking.cents(), asking_wage_permille).unwrap();
        assert_eq!(outcome, O::Accepted);
        assert_eq!(session.roster[p.as_usize()].club, domain::ClubId::new(3));
        assert_eq!(session.budgets[3], buyer_budget - asking);
        assert_eq!(session.budgets[seller.as_usize()], seller_budget + asking);
        let contract = session.contracts[p.as_usize()];
        assert_eq!(contract.seasons_remaining, 3);
        assert_eq!(contract.wage_permille, asking_wage_permille);
        assert_eq!(squad_of(&session, seller.index()).len(), squad_before - 1);
        assert_eq!(
            session.seasons_since_transfer[p.as_usize()],
            0,
            "adaptação recomeça"
        );

        let QueryResult::Player(Some(detail)) = session.query(Query::Player { player: p }) else {
            panic!()
        };
        assert_eq!(detail.row.club, domain::ClubId::new(3));
        assert_eq!(detail.row.wage_permille, asking_wage_permille);
        assert!(detail.row.salary_per_season.cents() > 0);

        // Comprar o mesmo jogador de novo é inválido: já é seu.
        assert!(matches!(
            offer(&mut session, p, asking.cents(), 3000),
            Err(AppError::InvalidOffer(_))
        ));
    }

    #[test]
    fn salario_negociado_pesa_na_folha_da_proxima_pre_temporada() {
        let mut session = market_ready();
        let p = buyable(&session);
        let accepted = world::OfferOutcome::Accepted;
        let QueryResult::Club(Some(before)) = session.query(Query::Club {
            club: domain::ClubId::new(3),
        }) else {
            panic!()
        };
        // Paga bem acima do pedido nos dois lados para fechar de primeira.
        let outcome = offer(&mut session, p, 500_000_000_000, 3000).unwrap();
        assert_eq!(outcome, accepted);
        let QueryResult::Club(Some(after)) = session.query(Query::Club {
            club: domain::ClubId::new(3),
        }) else {
            panic!()
        };
        assert_eq!(after.squad_size, before.squad_size + 1);
        let base = world::salary_per_season(session.contracts[p.as_usize()].signed_ability);
        assert_eq!(
            after.payroll_per_season.cents() - before.payroll_per_season.cents(),
            base.cents() * 3,
            "salário 3000‰ = 3x o base"
        );
    }

    #[test]
    fn recusas_do_clube_e_do_jogador_e_falta_de_caixa_nao_mudam_nada() {
        use world::{OfferOutcome as O, Refusal};
        let mut session = market_ready();
        let p = buyable(&session);
        let idx = p.as_usize();
        let seller = session.roster[idx].club;
        let snapshot = |s: &GameSession| (s.budgets.clone(), s.roster.clone());

        let state = snapshot(&session);
        session.loyalty[idx] = 100;
        assert_eq!(
            offer(&mut session, p, 500_000_000_000, 3000).unwrap(),
            O::ClubRefuses(Refusal::NotForSale)
        );
        session.loyalty[idx] = 0;

        session.budgets[3] = domain::Money::from_cents(10);
        assert_eq!(
            offer(&mut session, p, 500_000_000_000, 3000).unwrap(),
            O::CannotAfford
        );
        session.budgets[3] = domain::Money::from_cents(1_000_000_000_000);

        session.ambition[idx] = 100;
        session.club_reputation[3] = 5;
        session.club_reputation[seller.as_usize()] = 95;
        assert_eq!(
            offer(&mut session, p, 500_000_000_000, 3000).unwrap(),
            O::PlayerRefuses(Refusal::WantsBiggerClub)
        );
        // Só os ajustes manuais do teste mudaram; nenhuma recusa mexeu em caixa/elenco.
        let after = snapshot(&session);
        assert_eq!(after.1, state.1);
        assert_eq!(after.0[seller.as_usize()], state.0[seller.as_usize()]);
    }

    #[test]
    fn vendedor_com_elenco_no_limite_recusa_vender() {
        use world::{OfferOutcome as O, Refusal};
        let mut session = market_ready();
        let p = buyable(&session);
        let seller = session.roster[p.as_usize()].club;
        // Tira jogadores do vendedor até ficar com 14: não pode mais vender.
        let mut sellers_players: Vec<usize> = (0..session.roster.len())
            .filter(|&i| session.roster[i].club == seller && i != p.as_usize())
            .collect();
        sellers_players.sort_by_key(|&i| std::cmp::Reverse(i));
        let mut count = squad_of(&session, seller.index()).len();
        for i in sellers_players {
            if count <= 14 {
                break;
            }
            // devolve para um terceiro clube, preservando o mínimo por linha
            let other = domain::ClubId::new(if seller.index() == 15 { 14 } else { 15 });
            if session.roster[i].position != session.roster[p.as_usize()].position {
                session.roster[i].club = other;
                count -= 1;
            }
        }
        assert!(count <= 14, "montagem do cenário");
        assert_eq!(
            offer(&mut session, p, 500_000_000_000, 3000).unwrap(),
            O::ClubRefuses(Refusal::SquadLimits)
        );
    }

    #[test]
    fn propostas_mal_formadas_sao_erro_tipado_e_nao_entram_no_log() {
        let mut session = market_ready();
        let p = buyable(&session);
        let log = session.to_save().command_log;
        let own = squad_of(&session, 3)[0].id;
        for (player, wage, seasons) in [
            (own, 1000u16, 3u8),
            (domain::PlayerId::new(99_999), 1000, 3),
            (p, 1000, 0),
            (p, 1000, 6),
            (p, 100, 3),
            (p, 9000, 3),
        ] {
            let result = session.dispatch(Command::MakeOffer {
                player,
                fee: domain::Money::from_cents(1),
                wage_permille: wage,
                seasons,
            });
            assert!(
                matches!(result, Err(AppError::InvalidOffer(_))),
                "{player} {wage} {seasons}"
            );
        }
        assert_eq!(session.to_save().command_log, log);
    }

    #[test]
    fn nao_da_para_negociar_compra_durante_uma_partida_ao_vivo() {
        let mut session = market_ready();
        let p = buyable(&session);
        session.dispatch(Command::StartMatch).unwrap();
        assert!(matches!(
            offer(&mut session, p, 1, 1000),
            Err(AppError::MatchInProgress)
        ));
    }

    #[test]
    fn renovacao_exige_o_salario_pedido_e_vale_a_qualquer_momento() {
        use world::OfferOutcome as O;
        let mut session = managed(3);
        let own = squad_of(&session, 3)[2].id;
        let renew = |s: &mut GameSession, wage, seasons| {
            s.dispatch(Command::RenewContract {
                player: own,
                wage_permille: wage,
                seasons,
            })
            .map(|r| r.offer.unwrap())
        };
        let O::PlayerWantsMore {
            asking_wage_permille,
        } = renew(&mut session, 500, 4).unwrap()
        else {
            panic!("salário mínimo deveria ser pouco")
        };
        assert!((850..=1150).contains(&asking_wage_permille));
        assert_eq!(
            renew(&mut session, asking_wage_permille, 4).unwrap(),
            O::Accepted
        );
        let contract = session.contracts[own.as_usize()];
        assert_eq!(contract.seasons_remaining, 4);
        assert_eq!(contract.wage_permille, asking_wage_permille);
        assert_eq!(
            contract.signed_ability,
            session.roster[own.as_usize()].ability.current()
        );

        let stranger = squad_of(&session, 4)[0].id;
        assert!(matches!(
            session.dispatch(Command::RenewContract {
                player: stranger,
                wage_permille: 1000,
                seasons: 2
            }),
            Err(AppError::InvalidOffer(_))
        ));
    }

    #[test]
    fn negociacoes_sobrevivem_ao_save_e_load() {
        // O replay só tem comandos: sem ajustar o caixa direto, a oferta usa
        // o orçamento real do clube.
        let mut replayable = managed(3);
        replayable.dispatch(Command::BeginSeason).unwrap();
        let target = buyable(&replayable);
        let fee = replayable.budgets[3].cents();
        let _ = replayable.dispatch(Command::MakeOffer {
            player: target,
            fee: domain::Money::from_cents(fee),
            wage_permille: 3000,
            seasons: 5,
        });
        replayable.dispatch(Command::AdvanceSeason).unwrap();

        let reloaded = GameSession::load(&example_pack_path(), &replayable.to_save()).unwrap();
        assert_eq!(reloaded.budgets, replayable.budgets);
        assert_eq!(reloaded.contracts, replayable.contracts);
        assert_eq!(reloaded.to_save(), replayable.to_save());
    }

    fn incoming(session: &GameSession) -> Vec<IncomingOfferRow> {
        match session.query(Query::IncomingOffers) {
            QueryResult::IncomingOffers(rows) => rows,
            other => panic!("resposta inesperada: {other:?}"),
        }
    }

    /// Procura (por seed) uma sessão em que o clube 3 já tem proposta de pé
    /// logo depois de abrir a temporada ou nas primeiras rodadas da janela.
    fn session_with_incoming_offer() -> GameSession {
        for seed in 0..60u64 {
            let mut session = GameSession::new(&example_pack_path(), seed).unwrap();
            take(&mut session, 3, "Zé Treinador").unwrap();
            session.dispatch(Command::BeginSeason).unwrap();
            for _ in 0..3 {
                if !incoming(&session).is_empty() {
                    return session;
                }
                session.dispatch(Command::AdvanceRound).unwrap();
            }
            if !incoming(&session).is_empty() {
                return session;
            }
        }
        panic!("nenhuma seed de 0..60 gerou proposta (30% por rodada)");
    }

    #[test]
    fn sessao_observadora_nunca_recebe_proposta() {
        let mut session = GameSession::new(&example_pack_path(), 1).unwrap();
        session.dispatch(Command::BeginSeason).unwrap();
        for _ in 0..14 {
            session.dispatch(Command::AdvanceRound).unwrap();
            assert!(incoming(&session).is_empty());
        }
    }

    #[test]
    fn proposta_da_ia_traz_jogador_do_tecnico_comprador_valor_e_validade() {
        let session = session_with_incoming_offer();
        let offers = incoming(&session);
        let o = &offers[0];
        assert_eq!(
            session.roster[o.player.id.as_usize()].club,
            domain::ClubId::new(3)
        );
        assert_ne!(o.from, domain::ClubId::new(3));
        assert_eq!(o.from_name, session.pack.club(o.from).name);
        assert!(o.fee.cents() > 0 && o.market_value.cents() > 0);
        assert!((1..=2).contains(&o.expires_in_matchdays));
        let ratio = o.fee.cents() * 1000 / o.market_value.cents();
        assert!(
            (850..=1250).contains(&ratio),
            "valor entre 90% e 120% (±arredondamento): {ratio}"
        );
    }

    #[test]
    fn aceitar_proposta_vende_o_jogador_e_move_o_dinheiro() {
        let mut session = session_with_incoming_offer();
        let o = incoming(&session).remove(0);
        let (mine, theirs) = (session.budgets[3], session.budgets[o.from.as_usize()]);
        let squad_before = squad_of(&session, 3).len();

        let receipt = session
            .dispatch(Command::RespondToOffer {
                offer: o.id,
                accept: true,
            })
            .unwrap();
        assert_eq!(receipt.offer, Some(world::OfferOutcome::Accepted));
        assert_eq!(session.roster[o.player.id.as_usize()].club, o.from);
        assert_eq!(session.budgets[3], mine + o.fee);
        assert_eq!(session.budgets[o.from.as_usize()], theirs - o.fee);
        assert_eq!(squad_of(&session, 3).len(), squad_before - 1);
        assert!(
            incoming(&session)
                .iter()
                .all(|x| x.player.id != o.player.id)
        );
        assert_eq!(session.seasons_since_transfer[o.player.id.as_usize()], 0);
        // A proposta foi consumida.
        assert!(matches!(
            session.dispatch(Command::RespondToOffer {
                offer: o.id,
                accept: true
            }),
            Err(AppError::UnknownOffer)
        ));
    }

    #[test]
    fn recusar_proposta_so_a_descarta() {
        let mut session = session_with_incoming_offer();
        let o = incoming(&session).remove(0);
        let (budgets, roster) = (session.budgets.clone(), session.roster.clone());
        let receipt = session
            .dispatch(Command::RespondToOffer {
                offer: o.id,
                accept: false,
            })
            .unwrap();
        assert_eq!(receipt.offer, None);
        assert!(incoming(&session).iter().all(|x| x.id != o.id));
        assert_eq!(session.budgets, budgets);
        assert_eq!(session.roster, roster);
    }

    #[test]
    fn aceitar_recusa_se_deixa_o_elenco_no_minimo_ou_o_comprador_sem_caixa() {
        let mut session = session_with_incoming_offer();
        let o = incoming(&session).remove(0);
        let log = session.to_save().command_log;

        // Comprador sem caixa.
        let cash = session.budgets[o.from.as_usize()];
        session.budgets[o.from.as_usize()] = domain::Money::ZERO;
        assert!(matches!(
            session.dispatch(Command::RespondToOffer {
                offer: o.id,
                accept: true
            }),
            Err(AppError::OfferNoLongerValid)
        ));
        session.budgets[o.from.as_usize()] = cash;

        // Elenco no limite: leva jogadores (de outras linhas) para outro clube.
        let target_position = session.roster[o.player.id.as_usize()].position;
        let mut count = squad_of(&session, 3).len();
        for i in 0..session.roster.len() {
            if count <= 14 {
                break;
            }
            let p = &mut session.roster[i];
            if p.club == domain::ClubId::new(3)
                && p.position != target_position
                && p.player != o.player.id
            {
                p.club = domain::ClubId::new(15);
                count -= 1;
            }
        }
        assert!(matches!(
            session.dispatch(Command::RespondToOffer {
                offer: o.id,
                accept: true
            }),
            Err(AppError::SquadTooSmall)
        ));
        assert_eq!(
            session.to_save().command_log,
            log,
            "erros não entram no log"
        );
        assert!(
            incoming(&session).iter().any(|x| x.id == o.id),
            "a proposta segue de pé"
        );
    }

    #[test]
    fn proposta_expira_depois_de_duas_rodadas_e_nao_sobrevive_a_janela_fechada() {
        let mut session = session_with_incoming_offer();
        let id = incoming(&session)[0].id;
        // Joga até a janela de verão fechar: toda proposta some.
        for _ in 0..4 {
            session.dispatch(Command::AdvanceRound).unwrap();
        }
        assert!(!window(&session).open);
        assert!(incoming(&session).iter().all(|o| o.id != id));
        assert!(incoming(&session).is_empty());
    }

    #[test]
    fn propostas_aceitas_sobrevivem_ao_save_e_load() {
        let mut session = session_with_incoming_offer();
        let o = incoming(&session).remove(0);
        session
            .dispatch(Command::RespondToOffer {
                offer: o.id,
                accept: true,
            })
            .unwrap();
        session.dispatch(Command::AdvanceSeason).unwrap();
        let reloaded = GameSession::load(&example_pack_path(), &session.to_save()).unwrap();
        assert_eq!(reloaded.budgets, session.budgets);
        assert_eq!(reloaded.roster, session.roster);
        assert_eq!(reloaded.to_save(), session.to_save());
    }

    fn search(session: &GameSession, filter: PlayerFilter, offset: u32, limit: u16) -> SearchPage {
        match session.query(Query::Search {
            filter,
            offset,
            limit,
        }) {
            QueryResult::Search(page) => page,
            other => panic!("resposta inesperada: {other:?}"),
        }
    }

    #[test]
    fn busca_ordena_do_melhor_pro_pior_e_pagina_sem_repetir_nem_perder() {
        let session = GameSession::new(&example_pack_path(), 42).unwrap();
        let all = search(&session, PlayerFilter::default(), 0, 1000);
        assert_eq!(all.total, 256);
        assert_eq!(all.rows.len(), 50, "página limitada a MAX_SEARCH_PAGE");
        assert!(
            all.rows
                .windows(2)
                .all(|w| w[0].current_ability >= w[1].current_ability)
        );

        let mut seen = Vec::new();
        for page in 0..6 {
            let p = search(&session, PlayerFilter::default(), page * 50, 50);
            assert_eq!(p.total, 256);
            seen.extend(p.rows.iter().map(|r| r.id));
        }
        assert_eq!(seen.len(), 256);
        let mut unique = seen.clone();
        unique.sort();
        unique.dedup();
        assert_eq!(unique.len(), 256, "nenhum repetido entre páginas");
        assert!(
            search(&session, PlayerFilter::default(), 300, 10)
                .rows
                .is_empty()
        );
    }

    #[test]
    fn busca_filtra_por_posicao_habilidade_idade_clube_e_valor() {
        let session = GameSession::new(&example_pack_path(), 42).unwrap();
        let gks = search(
            &session,
            PlayerFilter {
                position: Some(domain::Position::Goalkeeper),
                ..PlayerFilter::default()
            },
            0,
            50,
        );
        assert_eq!(gks.total, 32, "2 goleiros por clube x 16");
        assert!(
            gks.rows
                .iter()
                .all(|r| r.position == domain::Position::Goalkeeper)
        );

        let good = search(
            &session,
            PlayerFilter {
                min_ability: 140,
                ..PlayerFilter::default()
            },
            0,
            50,
        );
        assert!(good.rows.iter().all(|r| r.current_ability >= 140));
        assert!(good.total < 256);

        let young = search(
            &session,
            PlayerFilter {
                max_age: Some(21),
                ..PlayerFilter::default()
            },
            0,
            50,
        );
        assert!(young.rows.iter().all(|r| r.age <= 21));

        let others = search(
            &session,
            PlayerFilter {
                not_from_club: Some(domain::ClubId::new(3)),
                ..PlayerFilter::default()
            },
            0,
            50,
        );
        assert_eq!(others.total, 256 - 16);
        assert!(others.rows.iter().all(|r| r.club != domain::ClubId::new(3)));

        let budget = domain::Money::from_cents(50_000_000);
        let cheap = search(
            &session,
            PlayerFilter {
                max_value: Some(budget),
                ..PlayerFilter::default()
            },
            0,
            50,
        );
        assert!(cheap.rows.iter().all(|r| r.market_value <= budget));
        assert!(cheap.total < 256);
    }

    #[test]
    fn busca_com_limite_zero_devolve_ao_menos_uma_linha() {
        let session = GameSession::new(&example_pack_path(), 42).unwrap();
        assert_eq!(
            search(&session, PlayerFilter::default(), 0, 0).rows.len(),
            1
        );
    }

    fn first_player_ids(session: &GameSession, club: u32, n: usize) -> Vec<domain::PlayerId> {
        squad_of(session, club)
            .iter()
            .take(n)
            .map(|p| p.id)
            .collect()
    }

    /// Onze válido para 4-4-2 do clube: o melhor onze automático.
    fn valid_lineup(session: &GameSession, club: u32) -> Vec<domain::PlayerId> {
        let QueryResult::Lineup(Some(info)) = session.query(Query::Lineup) else {
            panic!("sem escalação")
        };
        assert_eq!(info.slots.len(), 11);
        let _ = club;
        info.slots.iter().map(|s| s.player).collect()
    }

    fn managed(club: u32) -> GameSession {
        let mut session = GameSession::new(&example_pack_path(), 42).unwrap();
        take(&mut session, club, "Zé Treinador").unwrap();
        session
    }

    #[test]
    fn comandos_de_tecnico_exigem_um_clube_sob_comando() {
        let mut session = GameSession::new(&example_pack_path(), 42).unwrap();
        assert!(matches!(
            session.dispatch(Command::SetFormation {
                formation: Formation::F433
            }),
            Err(AppError::NoClubTaken)
        ));
        assert!(matches!(
            session.dispatch(Command::SetInstructions {
                mentality: Mentality::Attacking,
                pressing: Pressing::High
            }),
            Err(AppError::NoClubTaken)
        ));
        assert!(matches!(
            session.dispatch(Command::SetLineup {
                formation: Formation::F442,
                starters: Vec::new()
            }),
            Err(AppError::NoClubTaken)
        ));
        assert!(matches!(
            session.query(Query::Lineup),
            QueryResult::Lineup(None)
        ));
        assert!(session.to_save().command_log.is_empty());
    }

    #[test]
    fn query_lineup_padrao_e_o_melhor_onze_automatico_em_4_4_2() {
        let session = managed(3);
        let QueryResult::Lineup(Some(info)) = session.query(Query::Lineup) else {
            panic!("sem escalação")
        };
        assert_eq!(info.formation, Formation::F442);
        assert!(!info.manual);
        assert_eq!(info.mentality, Mentality::Balanced);
        assert_eq!(info.pressing, Pressing::Normal);
        assert_eq!(info.slots.len(), 11);
        assert!(info.adjustments.is_empty());
        assert!(
            info.slots
                .iter()
                .all(|s| s.slot_position == s.natural_position && s.fit_permille == 1000)
        );
        // 1 goleiro, 4 defensores, 4 meias, 2 atacantes.
        let count = |p| info.slots.iter().filter(|s| s.slot_position == p).count();
        assert_eq!(count(domain::Position::Goalkeeper), 1);
        assert_eq!(count(domain::Position::Defender), 4);
        assert_eq!(count(domain::Position::Midfielder), 4);
        assert_eq!(count(domain::Position::Forward), 2);
    }

    #[test]
    fn set_lineup_recusa_escalacoes_invalidas_com_motivo_e_sem_tocar_o_log() {
        let mut session = managed(3);
        let good = valid_lineup(&session, 3);
        let log_before = session.to_save().command_log;
        let lineup = |starters: Vec<domain::PlayerId>| Command::SetLineup {
            formation: Formation::F442,
            starters,
        };

        let too_few = good[..10].to_vec();
        assert!(matches!(
            session.dispatch(lineup(too_few)),
            Err(AppError::InvalidLineup(m)) if m.contains("11")
        ));

        let mut repeated = good.clone();
        repeated[10] = repeated[0];
        assert!(matches!(
            session.dispatch(lineup(repeated)),
            Err(AppError::InvalidLineup(m)) if m.contains("repetido")
        ));

        let mut foreign = good.clone();
        foreign[10] = first_player_ids(&session, 4, 1)[0];
        assert!(matches!(
            session.dispatch(lineup(foreign)),
            Err(AppError::InvalidLineup(m)) if m.contains("outro clube")
        ));

        let mut ghost = good.clone();
        ghost[10] = domain::PlayerId::new(99_999);
        assert!(matches!(
            session.dispatch(lineup(ghost)),
            Err(AppError::InvalidLineup(m)) if m.contains("inexistente")
        ));

        session.roster[good[2].as_usize()].injured = true;
        assert!(matches!(
            session.dispatch(lineup(good.clone())),
            Err(AppError::InvalidLineup(m)) if m.contains("lesionado")
        ));

        assert_eq!(
            session.to_save().command_log,
            log_before,
            "recusas não entram no log"
        );
    }

    #[test]
    fn set_lineup_valido_vira_escalacao_manual_e_set_formation_volta_ao_automatico() {
        let mut session = managed(3);
        let mut lineup = valid_lineup(&session, 3);
        lineup.swap(1, 2); // dois defensores trocam de lugar: ainda válido
        session
            .dispatch(Command::SetLineup {
                formation: Formation::F442,
                starters: lineup.clone(),
            })
            .unwrap();
        let QueryResult::Lineup(Some(info)) = session.query(Query::Lineup) else {
            panic!()
        };
        assert!(info.manual);
        assert_eq!(
            info.slots.iter().map(|s| s.player).collect::<Vec<_>>(),
            lineup
        );

        session
            .dispatch(Command::SetFormation {
                formation: Formation::F433,
            })
            .unwrap();
        let QueryResult::Lineup(Some(info)) = session.query(Query::Lineup) else {
            panic!()
        };
        assert!(!info.manual, "SetFormation descarta a escalação manual");
        assert_eq!(info.formation, Formation::F433);
        let forwards = info
            .slots
            .iter()
            .filter(|s| s.slot_position == domain::Position::Forward)
            .count();
        assert_eq!(forwards, 3);
    }

    #[test]
    fn jogador_fora_de_posicao_aparece_com_encaixe_menor_na_query() {
        let mut session = managed(3);
        let mut lineup = valid_lineup(&session, 3);
        lineup.swap(1, 5); // um defensor vai pro meio e um meia pra defesa
        session
            .dispatch(Command::SetLineup {
                formation: Formation::F442,
                starters: lineup,
            })
            .unwrap();
        let QueryResult::Lineup(Some(info)) = session.query(Query::Lineup) else {
            panic!()
        };
        let displaced: Vec<_> = info
            .slots
            .iter()
            .filter(|s| s.fit_permille < 1000)
            .collect();
        assert_eq!(displaced.len(), 2);
        assert!(displaced.iter().all(|s| s.fit_permille == 850));
    }

    #[test]
    fn titular_que_fica_indisponivel_e_trocado_e_o_recibo_conta_a_troca() {
        let mut session = managed(3);
        let lineup = valid_lineup(&session, 3);
        session
            .dispatch(Command::SetLineup {
                formation: Formation::F442,
                starters: lineup.clone(),
            })
            .unwrap();
        session.dispatch(Command::AdvanceRound).unwrap();
        // Depois da pré-temporada (que sorteia lesões), um titular se machuca.
        session.roster[lineup[4].as_usize()].injured = true;
        let QueryResult::Lineup(Some(info)) = session.query(Query::Lineup) else {
            panic!()
        };
        assert_eq!(info.adjustments.len(), 1);
        assert_eq!(info.adjustments[0].0, lineup[4]);
        assert!(info.slots.iter().all(|s| s.player != lineup[4]));

        let receipt = session.dispatch(Command::AdvanceRound).unwrap();
        assert_eq!(receipt.lineup_adjustments, 1);
    }

    #[test]
    fn instrucoes_ofensivas_mudam_de_verdade_o_mundo_e_o_replay_reproduz() {
        let mut observer = GameSession::new(&example_pack_path(), 42).unwrap();
        let mut tuned = managed(0);
        tuned
            .dispatch(Command::SetInstructions {
                mentality: Mentality::Attacking,
                pressing: Pressing::High,
            })
            .unwrap();
        tuned
            .dispatch(Command::SetFormation {
                formation: Formation::F433,
            })
            .unwrap();
        for _ in 0..3 {
            observer.dispatch(Command::AdvanceSeason).unwrap();
            tuned.dispatch(Command::AdvanceSeason).unwrap();
        }
        let tables = |s: &GameSession| format!("{:?}", s.history);
        assert_ne!(tables(&observer), tables(&tuned), "tática não é decorativa");

        let reloaded = GameSession::load(&example_pack_path(), &tuned.to_save()).unwrap();
        assert_eq!(tables(&reloaded), tables(&tuned));
        assert_eq!(reloaded.budgets, tuned.budgets);
    }

    #[test]
    fn escalacao_manual_no_meio_da_temporada_sobrevive_ao_save_e_load() {
        let mut session = managed(3);
        let lineup = valid_lineup(&session, 3);
        session.dispatch(Command::AdvanceRound).unwrap();
        session.dispatch(Command::AdvanceRound).unwrap();
        session
            .dispatch(Command::SetLineup {
                formation: Formation::F442,
                starters: lineup,
            })
            .unwrap();
        session
            .dispatch(Command::SetInstructions {
                mentality: Mentality::Defensive,
                pressing: Pressing::Normal,
            })
            .unwrap();
        session.dispatch(Command::AdvanceRound).unwrap();

        let mut reloaded = GameSession::load(&example_pack_path(), &session.to_save()).unwrap();
        let a = session.dispatch(Command::AdvanceSeason).unwrap();
        let b = reloaded.dispatch(Command::AdvanceSeason).unwrap();
        assert_eq!(format!("{a:?}"), format!("{b:?}"));
        assert_eq!(session.budgets, reloaded.budgets);
        assert_eq!(session.to_save(), reloaded.to_save());
    }

    fn all_clubs(session: &GameSession) -> Vec<domain::ClubId> {
        session.pack.clubs.iter().map(|c| c.id).collect()
    }

    fn squad_of(session: &GameSession, club: u32) -> Vec<PlayerRow> {
        match session.query(Query::Squad {
            club: domain::ClubId::new(club),
        }) {
            QueryResult::Squad(Some(rows)) => rows,
            other => panic!("resposta inesperada: {other:?}"),
        }
    }

    fn fixtures_of(session: &GameSession, club: u32) -> Option<FixtureList> {
        match session.query(Query::Fixtures {
            club: domain::ClubId::new(club),
        }) {
            QueryResult::Fixtures(f) => f,
            other => panic!("resposta inesperada: {other:?}"),
        }
    }

    fn round_results(session: &GameSession, matchday: Option<u32>) -> Option<RoundResults> {
        match session.query(Query::RoundResults { matchday }) {
            QueryResult::RoundResults(r) => r,
            other => panic!("resposta inesperada: {other:?}"),
        }
    }

    #[test]
    fn query_clubs_lista_todos_com_competicao_e_reputacao() {
        let session = GameSession::new(&example_pack_path(), 42).unwrap();
        let QueryResult::Clubs(rows) = session.query(Query::Clubs) else {
            panic!("resposta inesperada")
        };
        assert_eq!(rows.len(), 16);
        assert!(
            rows.iter()
                .all(|r| !r.name.is_empty() && r.reputation <= 100)
        );
        assert_eq!(rows[0].competition_name, "Primeira Divisão de Estrelônia");
        assert_eq!(rows[15].competition_name, "Segunda Divisão de Estrelônia");
        assert!(rows.windows(2).all(|w| w[0].id < w[1].id), "ordem de id");
    }

    #[test]
    fn query_club_traz_financas_estadio_e_folha_coerentes_com_o_estado() {
        let session = GameSession::new(&example_pack_path(), 42).unwrap();
        let club = domain::ClubId::new(4);
        let QueryResult::Club(Some(detail)) = session.query(Query::Club { club }) else {
            panic!("resposta inesperada")
        };
        assert_eq!(detail.id, club);
        assert_eq!(detail.budget, session.budgets[4]);
        assert_eq!(detail.debt, domain::Money::ZERO);
        assert_eq!(detail.stadium_capacity, session.stadium_capacities[4]);
        assert_eq!(detail.squad_size, 16);
        assert!(detail.payroll_per_season.cents() > 0);
        assert_eq!(detail.nation_name, "Estrelônia");

        let QueryResult::Club(none) = session.query(Query::Club {
            club: domain::ClubId::new(999),
        }) else {
            panic!("resposta inesperada")
        };
        assert!(none.is_none());
    }

    #[test]
    fn query_squad_ordena_por_posicao_e_habilidade_e_cobre_so_o_clube() {
        let session = GameSession::new(&example_pack_path(), 42).unwrap();
        let rows = squad_of(&session, 2);
        assert_eq!(rows.len(), 16);
        assert!(rows.iter().all(|r| r.club == domain::ClubId::new(2)));
        let order = |p: domain::Position| match p {
            domain::Position::Goalkeeper => 0,
            domain::Position::Defender => 1,
            domain::Position::Midfielder => 2,
            domain::Position::Forward => 3,
        };
        for pair in rows.windows(2) {
            let (a, b) = (&pair[0], &pair[1]);
            assert!(order(a.position) <= order(b.position));
            if a.position == b.position {
                assert!(a.current_ability >= b.current_ability);
            }
        }
        assert!(rows.iter().all(|r| r.market_value.cents() > 0));
        assert!(rows.iter().all(|r| !r.injured && !r.suspended));
        let QueryResult::Squad(none) = session.query(Query::Squad {
            club: domain::ClubId::new(999),
        }) else {
            panic!("resposta inesperada")
        };
        assert!(none.is_none());
    }

    #[test]
    fn query_player_traz_os_36_atributos_do_pack_e_nao_expoe_potencial() {
        let session = GameSession::new(&example_pack_path(), 42).unwrap();
        let player = domain::PlayerId::new(10);
        let QueryResult::Player(Some(detail)) = session.query(Query::Player { player }) else {
            panic!("resposta inesperada")
        };
        assert_eq!(detail.attributes.len(), 36);
        let pack_player = &session.pack.players[10];
        for av in &detail.attributes {
            assert_eq!(av.value, pack_player.attributes.get(av.attribute));
            assert!((1..=20).contains(&av.value));
        }
        assert_eq!(detail.row.id, player);
        assert_eq!(detail.club_name, session.pack.club(detail.row.club).name);
        assert_eq!(detail.row.current_ability, pack_player.ability.current());

        let QueryResult::Player(none) = session.query(Query::Player {
            player: domain::PlayerId::new(99_999),
        }) else {
            panic!("resposta inesperada")
        };
        assert!(none.is_none());
    }

    #[test]
    fn query_fixtures_so_existe_depois_de_a_temporada_comecar_e_sobrevive_ao_fim() {
        let mut session = GameSession::new(&example_pack_path(), 42).unwrap();
        assert!(
            fixtures_of(&session, 0).is_none(),
            "calendário só ao começar"
        );

        for _ in 0..3 {
            session.dispatch(Command::AdvanceRound).unwrap();
        }
        let list = fixtures_of(&session, 0).unwrap();
        assert_eq!(list.season_index, 0);
        assert_eq!(list.rows.len(), 14);
        assert_eq!(list.rows.iter().filter(|r| r.score.is_some()).count(), 3);
        assert!(list.rows.iter().all(|r| !r.home_name.is_empty()));

        session.dispatch(Command::AdvanceSeason).unwrap();
        let done = fixtures_of(&session, 0).unwrap();
        assert_eq!(done.season_index, 0, "entre temporadas, a última concluída");
        assert!(done.rows.iter().all(|r| r.score.is_some()));

        session.dispatch(Command::AdvanceRound).unwrap();
        let next = fixtures_of(&session, 0).unwrap();
        assert_eq!(next.season_index, 1, "a nova temporada substitui a antiga");
        assert_eq!(next.rows.iter().filter(|r| r.score.is_some()).count(), 1);
    }

    #[test]
    fn query_round_results_cobre_a_ultima_rodada_ou_uma_especifica() {
        let mut session = GameSession::new(&example_pack_path(), 42).unwrap();
        assert!(round_results(&session, None).is_none());

        session.dispatch(Command::AdvanceRound).unwrap();
        let first = round_results(&session, None).unwrap();
        assert_eq!((first.season_index, first.matchday), (0, 0));
        assert_eq!(first.rows.len(), 8);
        assert_eq!(round_results(&session, Some(0)), Some(first));
        assert!(
            round_results(&session, Some(1)).is_none(),
            "ainda não jogada"
        );

        session.dispatch(Command::AdvanceSeason).unwrap();
        assert_eq!(round_results(&session, None).unwrap().matchday, 13);
    }

    #[test]
    fn resultados_por_rodada_somam_os_gols_da_tabela_final() {
        let mut session = GameSession::new(&example_pack_path(), 9).unwrap();
        session.dispatch(Command::AdvanceSeason).unwrap();
        let from_rounds: u32 = (0..14)
            .flat_map(|m| round_results(&session, Some(m)).unwrap().rows)
            .map(|r| r.score.home_goals + r.score.away_goals)
            .sum();
        let from_tables: u32 = session
            .history
            .last()
            .unwrap()
            .tables
            .iter()
            .flat_map(|(_, t)| t.iter())
            .map(|row| row.goals_for)
            .sum();
        assert_eq!(from_rounds, from_tables);
    }

    #[test]
    fn suspensao_aparece_no_elenco_durante_a_temporada() {
        let mut session = GameSession::new(&example_pack_path(), 42).unwrap();
        let mut seen = false;
        for _ in 0..13 {
            session.dispatch(Command::AdvanceRound).unwrap();
            seen |= all_clubs(&session)
                .iter()
                .any(|c| squad_of(&session, c.index()).iter().any(|p| p.suspended));
        }
        assert!(
            seen,
            "com ~4% de expulsão por clube por rodada, alguém é suspenso"
        );
    }

    #[test]
    fn load_recusa_log_de_comandos_com_codigo_desconhecido() {
        let save = persist::SaveFile {
            pack_id: "example.two-tier".to_string(),
            pack_version: "0.1.0".to_string(),
            world_seed: 1,
            command_log: vec![2, 99],
        };
        let err = GameSession::load(&example_pack_path(), &save).unwrap_err();
        assert!(matches!(
            err,
            AppError::InvalidCommandLog {
                position: 1,
                tag: 99
            }
        ));
    }

    #[test]
    fn load_recusa_save_de_outro_pack() {
        let save = persist::SaveFile {
            pack_id: "outro.pack".to_string(),
            pack_version: "9.9.9".to_string(),
            world_seed: 1,
            command_log: Vec::new(),
        };
        let err = GameSession::load(&example_pack_path(), &save).unwrap_err();
        assert!(matches!(err, AppError::SavePackMismatch { .. }));
    }

    #[test]
    fn load_from_path_propaga_erro_tipado_de_persist_para_save_ausente() {
        let err = GameSession::load_from_path(
            &example_pack_path(),
            Path::new("/tmp/managerfc-save-que-nao-existe.cm0304save"),
        )
        .unwrap_err();
        assert!(matches!(err, AppError::Persist(_)));
    }
}

/// Testes de propriedade (`docs/08 §1`) — generaliza
/// `avancar_varias_temporadas_e_deterministico` para seeds arbitrárias.
#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;
    use std::path::PathBuf;

    fn example_pack_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/example-two-tier")
    }

    proptest! {
        #[test]
        fn dispatch_e_determinista_para_qualquer_seed(world_seed: u64) {
            let mut a = GameSession::new(&example_pack_path(), world_seed).unwrap();
            let mut b = GameSession::new(&example_pack_path(), world_seed).unwrap();

            let receipt_a = a.dispatch(Command::AdvanceSeason).unwrap();
            let receipt_b = b.dispatch(Command::AdvanceSeason).unwrap();
            prop_assert_eq!(receipt_a.matches_played, receipt_b.matches_played);
            prop_assert_eq!(receipt_a.movements, receipt_b.movements);

            let QueryResult::Competitions(comps) = a.query(Query::Competitions) else {
                unreachable!()
            };
            for comp in comps {
                prop_assert_eq!(
                    a.query(Query::Standings { competition: comp.id }),
                    b.query(Query::Standings { competition: comp.id })
                );
            }
        }

        /// Generaliza `load_a_partir_do_save_reproduz_a_mesma_tabela_por_replay`
        /// para seed e número de temporadas arbitrários (dentro de uma faixa
        /// pequena, já que cada temporada roda partidas de verdade).
        #[test]
        fn save_e_load_reproduzem_a_mesma_sessao_para_qualquer_seed_e_temporadas(
            world_seed: u64,
            seasons in 0u32..4,
        ) {
            let mut original = GameSession::new(&example_pack_path(), world_seed).unwrap();
            for _ in 0..seasons {
                original.dispatch(Command::AdvanceSeason).unwrap();
            }
            let save = original.to_save();
            let reloaded = GameSession::load(&example_pack_path(), &save).unwrap();

            prop_assert_eq!(
                reloaded.query(Query::CurrentSeason),
                original.query(Query::CurrentSeason)
            );
            let QueryResult::Competitions(comps) = original.query(Query::Competitions) else {
                unreachable!()
            };
            for comp in comps {
                prop_assert_eq!(
                    reloaded.query(Query::Standings { competition: comp.id }),
                    original.query(Query::Standings { competition: comp.id })
                );
            }
        }
    }
}
