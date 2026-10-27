//! Partida jogada **ao vivo** pelo clube do técnico (RF-PA-05/06/07,
//! `docs/01 §2.5`): a camada de jogadores sobre o motor minuto a minuto
//! ([`engine::LiveMatch`]).
//!
//! O motor v0.5 não tem jogadores em campo — só taxas de finalização e de
//! conversão por time (`docs/04 §7`). Este módulo **atribui** cada
//! finalização e cada gol a um jogador depois que o motor decidiu que eles
//! aconteceram, usando um RNG próprio (`"world.match.attribution"`), então
//! nada aqui pode mudar um placar: quem chuta, quem marca e quem dá o passe
//! são enfeites determinísticos sobre um resultado já decidido. O que **muda**
//! o resultado são as duas coisas que o técnico pode fazer durante o jogo:
//! substituir ([`LiveGame::substitute`]) e mudar mentalidade/pressão
//! ([`LiveGame::set_instructions`]), que trocam o perfil/tática do lado dele
//! do próximo minuto em diante.
//!
//! Só o clube do técnico humano pode mexer; o adversário joga o onze da IA
//! do começo ao fim. A saída é estruturada ([`MatchLine`]), nunca texto: o
//! "modo texto comentado" é a UI compondo frases localizadas a partir dela.
//!
//! Estatísticas honestas: finalizações, gols, assistências, minutos e **xG
//! do próprio modelo** (soma da probabilidade de gol de cada finalização).
//! Posse de bola, passes e desarmes não existem no motor atual e não são
//! inventados (RF-PA-07 completo pede zonas, M3).

use std::collections::{BTreeMap, HashSet};

use domain::{Attribute, ClubId, CompetitionId, DeterministicRng, Fixed, PlayerId, Position};
use engine::{
    FinishingQuality, GoalkeepingQuality, LiveMatch, MatchContext, MatchEvent, Side,
    TeamMatchProfile, TeamStrength,
};
use pack::LoadedPack;
use rules::{Fixture, Score};

use crate::progression::PlayerState;
use crate::season::{ExternalResult, PreparedFixture};
use crate::setup::{ClubSetup, Mentality, Pressing, ResolvedXi, profile_for, resolve_xi};

/// Substituições permitidas por partida, por time.
pub const MAX_SUBSTITUTIONS: u8 = 3;

/// O que uma linha do relato da partida descreve.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MatchLineKind {
    KickOff,
    Shot,
    Goal,
    Substitution,
    HalfTime,
    FullTime,
}

/// Uma linha do relato da partida, estruturada (a UI compõe o texto).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchLine {
    pub minute: u8,
    pub kind: MatchLineKind,
    /// De que lado é o lance; `None` em `KickOff`/`HalfTime`/`FullTime`.
    pub side: Option<Side>,
    /// `Shot`: quem chutou. `Goal`: quem marcou. `Substitution`: quem entrou.
    pub player: Option<PlayerId>,
    /// `Goal`: quem deu o passe, se houve. `Substitution`: quem saiu.
    pub other: Option<PlayerId>,
    /// Placar **depois** deste lance.
    pub home_goals: u32,
    pub away_goals: u32,
}

/// Desempenho de um jogador na partida.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlayerMatchStats {
    pub player: PlayerId,
    pub club: ClubId,
    pub side: Side,
    /// Posição natural do jogador.
    pub position: Position,
    pub started: bool,
    pub minutes: u8,
    pub shots: u8,
    pub goals: u8,
    pub assists: u8,
    /// Nota de 3,0 a 10,0, em décimos (`75` = 7,5).
    pub rating_x10: u8,
}

/// Retrato da partida num instante (ao vivo) ou no fim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatchReport {
    pub competition: CompetitionId,
    pub fixture: Fixture,
    pub season_index: u32,
    pub minute: u8,
    pub finished: bool,
    pub home: ClubId,
    pub away: ClubId,
    pub goals: (u32, u32),
    pub shots: (u32, u32),
    /// xG acumulado, em milésimos de gol.
    pub xg_milli: (u32, u32),
    pub lines: Vec<MatchLine>,
    /// Quem já entrou em campo, por id de jogador.
    pub players: Vec<PlayerMatchStats>,
}

/// Por que uma substituição foi recusada.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SubstitutionError {
    MatchFinished,
    /// Só o clube do técnico humano pode substituir.
    SideNotManaged,
    NoSubstitutionsLeft,
    PlayerNotOnPitch,
    PlayerNotOnBench,
}

#[derive(Debug)]
struct SideState {
    club: ClubId,
    /// Quem está em campo, na ordem dos espaços da formação.
    slots: Vec<(Position, PlayerId)>,
    /// Reservas disponíveis, melhores primeiro (posição, CA, id).
    bench: Vec<PlayerId>,
    subs_used: u8,
    /// `Some` só no clube do técnico humano.
    setup: Option<ClubSetup>,
    /// Perfil que o motor está usando (com moral/adaptação/consistência).
    profile: TeamMatchProfile,
    /// Perfil "cru" do onze atual (sem esses ajustes) — base da razão usada
    /// para reescalar `profile` quando o onze muda.
    raw: Option<TeamMatchProfile>,
}

/// Uma partida em andamento (ou terminada).
#[derive(Debug)]
pub struct LiveGame {
    competition: CompetitionId,
    fixture: Fixture,
    season_index: u32,
    world_seed: u64,
    attribution_key: u64,
    engine: LiveMatch,
    sides: [SideState; 2],
    lines: Vec<MatchLine>,
    stats: BTreeMap<PlayerId, PlayerStatsAcc>,
    processed_events: usize,
    last_shooter: Option<PlayerId>,
    halftime_done: bool,
    goals: (u32, u32),
}

#[derive(Debug, Clone)]
struct PlayerStatsAcc {
    club: ClubId,
    side: Side,
    position: Position,
    started: bool,
    entered: u8,
    left: Option<u8>,
    shots: u8,
    goals: u8,
    assists: u8,
}

const fn idx(side: Side) -> usize {
    match side {
        Side::Home => 0,
        Side::Away => 1,
    }
}

fn weighted_pick(rng: &mut DeterministicRng, weights: &[u32]) -> Option<usize> {
    let total: u32 = weights.iter().sum();
    if total == 0 {
        return None;
    }
    let mut roll = rng.below(total);
    for (i, &w) in weights.iter().enumerate() {
        if roll < w {
            return Some(i);
        }
        roll -= w;
    }
    None
}

/// Reescala `current` (perfil com moral/adaptação/consistência) pela razão
/// entre o perfil cru do onze novo e o do onze anterior — preserva os
/// ajustes multiplicativos da rodada sem precisar recalculá-los. Onze
/// igual (`new == old`) devolve `current` intacto, sem erro de arredondamento.
fn rescale(
    current: TeamMatchProfile,
    old: TeamMatchProfile,
    new: TeamMatchProfile,
) -> TeamMatchProfile {
    if old == new {
        return current;
    }
    let ratio = |cur: Fixed, old: Fixed, new: Fixed| cur * new / old;
    TeamMatchProfile {
        strength: TeamStrength::new(ratio(
            current.strength.value(),
            old.strength.value(),
            new.strength.value(),
        )),
        finishing: FinishingQuality::new(ratio(
            current.finishing.value(),
            old.finishing.value(),
            new.finishing.value(),
        )),
        goalkeeping: GoalkeepingQuality::new(ratio(
            current.goalkeeping.value(),
            old.goalkeeping.value(),
            new.goalkeeping.value(),
        )),
    }
}

fn position_order(p: Position) -> u8 {
    match p {
        Position::Goalkeeper => 0,
        Position::Defender => 1,
        Position::Midfielder => 2,
        Position::Forward => 3,
    }
}

impl LiveGame {
    /// Prepara a partida `prepared` (ver `SeasonRun::prepared_fixture_of`).
    /// `managed` é a escalação/instruções do clube do técnico humano, se ele
    /// joga esta partida; `unavailable` são os suspensos (para montar o
    /// banco). Nenhum minuto é jogado ainda.
    #[must_use]
    pub fn new(
        pack: &LoadedPack,
        roster: &[PlayerState],
        prepared: &PreparedFixture,
        managed: Option<&ClubSetup>,
        unavailable: &HashSet<PlayerId>,
        world_seed: u64,
    ) -> Self {
        let build = |side: Side| {
            let prep = match side {
                Side::Home => &prepared.home,
                Side::Away => &prepared.away,
            };
            let setup = managed.filter(|s| s.club == prep.club).cloned();
            let (slots, bench, raw) = match &setup {
                Some(setup) => {
                    let xi = resolve_xi(roster, setup, unavailable);
                    debug_assert_eq!(xi.players(), prep.starters);
                    let in_xi: HashSet<PlayerId> = xi.players().into_iter().collect();
                    let mut bench: Vec<&PlayerState> = roster
                        .iter()
                        .filter(|p| {
                            p.club == setup.club
                                && !p.injured
                                && !unavailable.contains(&p.player)
                                && !in_xi.contains(&p.player)
                        })
                        .collect();
                    bench.sort_by(|a, b| {
                        position_order(a.position)
                            .cmp(&position_order(b.position))
                            .then(b.ability.current().cmp(&a.ability.current()))
                            .then(a.player.cmp(&b.player))
                    });
                    let raw = profile_for(pack, roster, &xi, setup).map(|(p, _)| p);
                    (xi.slots, bench.iter().map(|p| p.player).collect(), raw)
                }
                None => (
                    prep.starters
                        .iter()
                        .map(|&id| (roster[id.as_usize()].position, id))
                        .collect(),
                    Vec::new(),
                    None,
                ),
            };
            SideState {
                club: prep.club,
                slots,
                bench,
                subs_used: 0,
                setup,
                profile: prep.profile,
                raw,
            }
        };
        let sides = [build(Side::Home), build(Side::Away)];

        let attribution_key = crate::fixture_key::pack(
            prepared.season_index,
            prepared.fixture.home,
            prepared.fixture.away,
            false,
        );
        let engine = LiveMatch::new(
            prepared.home.profile,
            prepared.away.profile,
            prepared.home.tactics,
            prepared.away.tactics,
            MatchContext {
                world_seed,
                fixture: attribution_key,
            },
        );

        let mut stats = BTreeMap::new();
        for (i, side) in [Side::Home, Side::Away].into_iter().enumerate() {
            for &(_, player) in &sides[i].slots {
                stats.insert(
                    player,
                    PlayerStatsAcc {
                        club: sides[i].club,
                        side,
                        position: roster[player.as_usize()].position,
                        started: true,
                        entered: 0,
                        left: None,
                        shots: 0,
                        goals: 0,
                        assists: 0,
                    },
                );
            }
        }

        let mut game = Self {
            competition: prepared.competition,
            fixture: prepared.fixture,
            season_index: prepared.season_index,
            world_seed,
            attribution_key,
            engine,
            sides,
            lines: Vec::new(),
            stats,
            processed_events: 0,
            last_shooter: None,
            halftime_done: false,
            goals: (0, 0),
        };
        game.process_new_events(pack);
        game
    }

    #[must_use]
    pub fn minute(&self) -> u8 {
        self.engine.minute()
    }

    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.engine.is_finished()
    }

    #[must_use]
    pub fn score(&self) -> (u32, u32) {
        self.goals
    }

    #[must_use]
    pub fn fixture(&self) -> Fixture {
        self.fixture
    }

    /// Qual lado o técnico humano comanda nesta partida, se algum.
    #[must_use]
    pub fn managed_side(&self) -> Option<Side> {
        [Side::Home, Side::Away]
            .into_iter()
            .find(|&s| self.sides[idx(s)].setup.is_some())
    }

    /// Quem está em campo de `side`, na ordem dos espaços.
    #[must_use]
    pub fn on_pitch(&self, side: Side) -> &[(Position, PlayerId)] {
        &self.sides[idx(side)].slots
    }

    /// Reservas que ainda podem entrar, melhores primeiro.
    #[must_use]
    pub fn bench(&self, side: Side) -> &[PlayerId] {
        &self.sides[idx(side)].bench
    }

    #[must_use]
    pub fn substitutions_left(&self, side: Side) -> u8 {
        if self.sides[idx(side)].setup.is_some() {
            MAX_SUBSTITUTIONS - self.sides[idx(side)].subs_used
        } else {
            0
        }
    }

    /// Mentalidade/pressão em vigor do lado do técnico humano.
    #[must_use]
    pub fn setup_of(&self, side: Side) -> Option<&ClubSetup> {
        self.sides[idx(side)].setup.as_ref()
    }

    /// Joga até `minutes` minutos (menos, se a partida acabar antes);
    /// devolve quantos foram jogados. A atribuição é feita minuto a minuto,
    /// então o tamanho do passo nunca muda o relato.
    pub fn advance(&mut self, pack: &LoadedPack, minutes: u8) -> u8 {
        let start = self.engine.minute();
        for _ in 0..minutes {
            if self.engine.is_finished() {
                break;
            }
            self.engine.step();
            self.process_new_events(pack);
            if self.engine.minute() == 45 && !self.halftime_done {
                self.halftime_done = true;
                self.push_line(45, MatchLineKind::HalfTime, None, None, None);
            }
        }
        self.engine.minute() - start
    }

    /// Joga o que falta até o apito final.
    pub fn play_to_end(&mut self, pack: &LoadedPack) {
        let left = 90 - self.engine.minute();
        self.advance(pack, left);
    }

    /// Resultado para `SeasonRun::commit_matchday`. Só faz sentido com a
    /// partida terminada.
    #[must_use]
    pub fn external_result(&self) -> ExternalResult {
        let (home_goals, away_goals) = self.engine.score();
        ExternalResult {
            fixture: self.fixture,
            score: Score {
                home_goals,
                away_goals,
            },
            shots: self.engine.shots(),
        }
    }

    /// Substitui `out` (em campo) por `incoming` (no banco) do lado `side`,
    /// valendo do próximo minuto em diante. O reserva assume o espaço do
    /// titular — fora da posição natural, rende menos
    /// ([`crate::position_fit_permille`]).
    pub fn substitute(
        &mut self,
        pack: &LoadedPack,
        roster: &[PlayerState],
        side: Side,
        out: PlayerId,
        incoming: PlayerId,
    ) -> Result<(), SubstitutionError> {
        if self.engine.is_finished() {
            return Err(SubstitutionError::MatchFinished);
        }
        let i = idx(side);
        if self.sides[i].setup.is_none() {
            return Err(SubstitutionError::SideNotManaged);
        }
        if self.sides[i].subs_used >= MAX_SUBSTITUTIONS {
            return Err(SubstitutionError::NoSubstitutionsLeft);
        }
        let Some(slot) = self.sides[i].slots.iter().position(|&(_, p)| p == out) else {
            return Err(SubstitutionError::PlayerNotOnPitch);
        };
        let Some(bench_pos) = self.sides[i].bench.iter().position(|&p| p == incoming) else {
            return Err(SubstitutionError::PlayerNotOnBench);
        };

        let minute = self.engine.minute();
        self.sides[i].slots[slot].1 = incoming;
        self.sides[i].bench.remove(bench_pos);
        self.sides[i].subs_used += 1;
        if let Some(acc) = self.stats.get_mut(&out) {
            acc.left = Some(minute);
        }
        self.stats.insert(
            incoming,
            PlayerStatsAcc {
                club: self.sides[i].club,
                side,
                position: roster[incoming.as_usize()].position,
                started: false,
                entered: minute,
                left: None,
                shots: 0,
                goals: 0,
                assists: 0,
            },
        );
        self.push_line(
            minute,
            MatchLineKind::Substitution,
            Some(side),
            Some(incoming),
            Some(out),
        );
        self.refresh_side(pack, roster, side, true);
        Ok(())
    }

    /// Muda mentalidade/pressão do lado do técnico humano, do próximo minuto
    /// em diante. `false` se o lado não é dele ou a partida acabou.
    pub fn set_instructions(
        &mut self,
        pack: &LoadedPack,
        roster: &[PlayerState],
        side: Side,
        mentality: Mentality,
        pressing: Pressing,
    ) -> bool {
        let i = idx(side);
        if self.engine.is_finished() {
            return false;
        }
        let Some(setup) = self.sides[i].setup.as_mut() else {
            return false;
        };
        setup.mentality = mentality;
        setup.pressing = pressing;
        self.refresh_side(pack, roster, side, false);
        true
    }

    /// Recalcula perfil (se o onze mudou) e tática do lado `side` e entrega
    /// ao motor.
    fn refresh_side(
        &mut self,
        pack: &LoadedPack,
        roster: &[PlayerState],
        side: Side,
        lineup_changed: bool,
    ) {
        let i = idx(side);
        let Some(setup) = self.sides[i].setup.clone() else {
            return;
        };
        let xi = ResolvedXi {
            slots: self.sides[i].slots.clone(),
            adjustments: Vec::new(),
        };
        let Some((raw_now, tactics)) = profile_for(pack, roster, &xi, &setup) else {
            return;
        };
        if lineup_changed {
            if let Some(old_raw) = self.sides[i].raw {
                self.sides[i].profile = rescale(self.sides[i].profile, old_raw, raw_now);
            }
            self.sides[i].raw = Some(raw_now);
        }
        self.engine.set_team(side, self.sides[i].profile, tactics);
    }

    fn push_line(
        &mut self,
        minute: u8,
        kind: MatchLineKind,
        side: Option<Side>,
        player: Option<PlayerId>,
        other: Option<PlayerId>,
    ) {
        self.lines.push(MatchLine {
            minute,
            kind,
            side,
            player,
            other,
            home_goals: self.goals.0,
            away_goals: self.goals.1,
        });
    }

    /// Converte os eventos novos do motor em linhas atribuídas a jogadores.
    fn process_new_events(&mut self, pack: &LoadedPack) {
        while self.processed_events < self.engine.events().len() {
            let event_index = self.processed_events;
            let event = self.engine.events()[event_index];
            self.processed_events += 1;
            match event {
                MatchEvent::KickOff => self.push_line(0, MatchLineKind::KickOff, None, None, None),
                MatchEvent::Shot { minute, side } => {
                    let shooter = self.pick_shooter(pack, side, event_index);
                    if let Some(acc) = shooter.and_then(|p| self.stats.get_mut(&p)) {
                        acc.shots = acc.shots.saturating_add(1);
                    }
                    self.last_shooter = shooter;
                    self.push_line(minute, MatchLineKind::Shot, Some(side), shooter, None);
                }
                MatchEvent::Goal { minute, side } => {
                    let scorer = self.last_shooter.take();
                    let assist = scorer.and_then(|s| self.pick_assist(pack, side, s, event_index));
                    match side {
                        Side::Home => self.goals.0 += 1,
                        Side::Away => self.goals.1 += 1,
                    }
                    if let Some(acc) = scorer.and_then(|s| self.stats.get_mut(&s)) {
                        acc.goals = acc.goals.saturating_add(1);
                    }
                    if let Some(acc) = assist.and_then(|a| self.stats.get_mut(&a)) {
                        acc.assists = acc.assists.saturating_add(1);
                    }
                    self.push_line(minute, MatchLineKind::Goal, Some(side), scorer, assist);
                }
                MatchEvent::FullTime { .. } => {
                    self.push_line(90, MatchLineKind::FullTime, None, None, None);
                }
            }
        }
    }

    fn attribution_rng(&self, domain_tag: &str, event_index: usize) -> DeterministicRng {
        DeterministicRng::seeded(
            self.world_seed,
            domain_tag,
            self.attribution_key,
            event_index as u64,
        )
    }

    fn pick_shooter(&self, pack: &LoadedPack, side: Side, event_index: usize) -> Option<PlayerId> {
        let slots = &self.sides[idx(side)].slots;
        let weights: Vec<u32> = slots
            .iter()
            .map(|&(slot, player)| {
                let position_weight = match slot {
                    Position::Goalkeeper => 0,
                    Position::Defender => 1,
                    Position::Midfielder => 3,
                    Position::Forward => 6,
                };
                let finishing = u32::from(
                    pack.players[player.as_usize()]
                        .attributes
                        .get(Attribute::Finishing),
                );
                position_weight * (finishing + 2)
            })
            .collect();
        let mut rng = self.attribution_rng("world.match.shooter", event_index);
        weighted_pick(&mut rng, &weights).map(|i| slots[i].1)
    }

    fn pick_assist(
        &self,
        pack: &LoadedPack,
        side: Side,
        scorer: PlayerId,
        event_index: usize,
    ) -> Option<PlayerId> {
        let mut rng = self.attribution_rng("world.match.assist", event_index);
        // Nem todo gol tem assistência (cobrança, rebote, jogada individual).
        if !rng.chance_per_mille(650) {
            return None;
        }
        let slots = &self.sides[idx(side)].slots;
        let weights: Vec<u32> = slots
            .iter()
            .map(|&(slot, player)| {
                if player == scorer {
                    return 0;
                }
                let position_weight = match slot {
                    Position::Goalkeeper => 0,
                    Position::Defender => 1,
                    Position::Midfielder => 4,
                    Position::Forward => 3,
                };
                let attrs = pack.players[player.as_usize()].attributes;
                let creativity = (u32::from(attrs.get(Attribute::Passing))
                    + u32::from(attrs.get(Attribute::Vision)))
                    / 2;
                position_weight * (creativity + 2)
            })
            .collect();
        weighted_pick(&mut rng, &weights).map(|i| slots[i].1)
    }

    /// Retrato da partida agora. As notas só são definitivas depois do apito
    /// final, mas são calculadas com o que já aconteceu.
    #[must_use]
    pub fn report(&self) -> MatchReport {
        let now = self.engine.minute();
        let (home_goals, away_goals) = self.goals;
        let players = self
            .stats
            .iter()
            .map(|(&player, acc)| {
                let minutes = acc.left.unwrap_or(now).saturating_sub(acc.entered);
                let (own, opp) = match acc.side {
                    Side::Home => (home_goals, away_goals),
                    Side::Away => (away_goals, home_goals),
                };
                PlayerMatchStats {
                    player,
                    club: acc.club,
                    side: acc.side,
                    position: acc.position,
                    started: acc.started,
                    minutes,
                    shots: acc.shots,
                    goals: acc.goals,
                    assists: acc.assists,
                    rating_x10: rating_x10(acc, minutes, own, opp),
                }
            })
            .collect();
        MatchReport {
            competition: self.competition,
            fixture: self.fixture,
            season_index: self.season_index,
            minute: now,
            finished: self.engine.is_finished(),
            home: self.sides[0].club,
            away: self.sides[1].club,
            goals: self.goals,
            shots: self.engine.shots(),
            xg_milli: self.engine.xg_milli(),
            lines: self.lines.clone(),
            players,
        }
    }

    /// Linhas do relato a partir do índice `from` (para a UI só buscar o que
    /// é novo).
    #[must_use]
    pub fn lines_from(&self, from: usize) -> &[MatchLine] {
        self.lines.get(from..).unwrap_or(&[])
    }
}

/// Nota de 3,0 a 10,0 (em décimos): base 6,0; gol +1,5; assistência +0,8;
/// cada finalização +0,1 (até 3); vitória +0,3, derrota -0,3; defensor ou
/// goleiro com 60+ minutos: +0,5 sem sofrer gol, -0,5 se sofrer 3 ou mais.
/// Quem jogou menos de 20 minutos só recebe os lances (gol, assistência).
fn rating_x10(acc: &PlayerStatsAcc, minutes: u8, own_goals: u32, opp_goals: u32) -> u8 {
    let mut rating: i32 = 60;
    rating += 15 * i32::from(acc.goals);
    rating += 8 * i32::from(acc.assists);
    rating += i32::from(acc.shots.min(3));
    if minutes >= 20 {
        rating += match own_goals.cmp(&opp_goals) {
            std::cmp::Ordering::Greater => 3,
            std::cmp::Ordering::Less => -3,
            std::cmp::Ordering::Equal => 0,
        };
        if minutes >= 60 && matches!(acc.position, Position::Goalkeeper | Position::Defender) {
            if opp_goals == 0 {
                rating += 5;
            } else if opp_goals >= 3 {
                rating -= 5;
            }
        }
    }
    rating.clamp(30, 100) as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::season::{CareerRound, SeasonRun};
    use std::path::PathBuf;

    struct World {
        pack: LoadedPack,
        roster: Vec<PlayerState>,
    }

    fn world() -> World {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/example-two-tier");
        let pack = pack::load_and_validate(&path).unwrap().pack;
        let roster = crate::progression::initial_roster(&pack);
        World { pack, roster }
    }

    /// Prepara a primeira rodada e devolve a partida do clube 0.
    fn prepared(w: &World, setups: &[ClubSetup]) -> PreparedFixture {
        let membership: Vec<CompetitionId> = w.pack.clubs.iter().map(|c| c.competition).collect();
        let mut budgets = vec![domain::Money::ZERO; w.pack.clubs.len()];
        let n_clubs = w.pack.clubs.len();
        let n_players = w.pack.players.len();
        let caps = vec![20_000u32; n_clubs];
        let rep = vec![50u8; n_clubs];
        let hidden = vec![50u8; n_players];
        let sst = vec![5u8; n_players];
        let career = CareerRound {
            roster: &w.roster,
            budgets: &mut budgets,
            stadium_capacities: &caps,
            club_reputation: &rep,
            dirtiness: &hidden,
            big_game_mentality: &hidden,
            consistency: &hidden,
            adaptation: &hidden,
            seasons_since_transfer: &sst,
            setups,
        };
        let mut run = SeasonRun::start(&w.pack, &membership, 0);
        run.begin_matchday(&w.pack, &career, 42);
        run.prepared_fixture_of(ClubId::new(0)).unwrap()
    }

    fn managed_setup() -> ClubSetup {
        ClubSetup::new(ClubId::new(0))
    }

    fn game(w: &World, setups: &[ClubSetup]) -> (LiveGame, PreparedFixture) {
        let prep = prepared(w, setups);
        let g = LiveGame::new(
            &w.pack,
            &w.roster,
            &prep,
            setups.first(),
            &HashSet::new(),
            42,
        );
        (g, prep)
    }

    #[test]
    fn sem_intervencao_o_resultado_e_o_do_motor_para_a_mesma_partida() {
        let w = world();
        let (mut g, prep) = game(&w, &[managed_setup()]);
        g.play_to_end(&w.pack);
        assert!(g.is_finished());
        let direct = engine::simulate_tactical(
            prep.home.profile,
            prep.away.profile,
            prep.home.tactics,
            prep.away.tactics,
            MatchContext {
                world_seed: 42,
                fixture: crate::fixture_key::pack(0, prep.fixture.home, prep.fixture.away, false),
            },
        );
        assert_eq!(g.score(), engine::score(&direct));
        assert_eq!(g.external_result().shots, engine::shots(&direct));
    }

    #[test]
    fn passo_a_passo_e_tudo_de_uma_vez_geram_o_mesmo_relato() {
        let w = world();
        let (mut whole, _) = game(&w, &[managed_setup()]);
        whole.play_to_end(&w.pack);
        let (mut stepped, _) = game(&w, &[managed_setup()]);
        for chunk in [1u8, 44, 3, 17, 25] {
            stepped.advance(&w.pack, chunk);
        }
        assert_eq!(whole.report(), stepped.report());
    }

    #[test]
    fn atribuicao_e_coerente_com_placar_e_finalizacoes() {
        let w = world();
        let (mut g, _) = game(&w, &[managed_setup()]);
        g.play_to_end(&w.pack);
        let report = g.report();
        let count = |kind| report.lines.iter().filter(|l| l.kind == kind).count() as u32;
        assert_eq!(count(MatchLineKind::Goal), report.goals.0 + report.goals.1);
        assert_eq!(count(MatchLineKind::Shot), report.shots.0 + report.shots.1);
        assert_eq!(count(MatchLineKind::KickOff), 1);
        assert_eq!(count(MatchLineKind::HalfTime), 1);
        assert_eq!(count(MatchLineKind::FullTime), 1);
        let last = report.lines.last().unwrap();
        assert_eq!(last.kind, MatchLineKind::FullTime);
        assert_eq!((last.home_goals, last.away_goals), report.goals);

        let goals: u32 = report.players.iter().map(|p| u32::from(p.goals)).sum();
        assert_eq!(goals, report.goals.0 + report.goals.1);
        let shots: u32 = report.players.iter().map(|p| u32::from(p.shots)).sum();
        assert_eq!(shots, report.shots.0 + report.shots.1);
        for line in report
            .lines
            .iter()
            .filter(|l| l.kind == MatchLineKind::Goal)
        {
            assert!(line.player.is_some(), "todo gol tem autor");
            assert_ne!(line.player, line.other, "ninguém assiste a si mesmo");
        }
    }

    #[test]
    fn goleiro_nunca_chuta_e_autor_e_do_time_certo() {
        let w = world();
        let (mut g, prep) = game(&w, &[managed_setup()]);
        g.play_to_end(&w.pack);
        let report = g.report();
        for line in report
            .lines
            .iter()
            .filter(|l| l.kind == MatchLineKind::Shot)
        {
            let shooter = line.player.unwrap();
            let state = &w.roster[shooter.as_usize()];
            assert_ne!(state.position, Position::Goalkeeper);
            let expected_club = match line.side.unwrap() {
                Side::Home => prep.home.club,
                Side::Away => prep.away.club,
            };
            assert_eq!(state.club, expected_club);
        }
    }

    #[test]
    fn relato_e_deterministico_para_a_mesma_entrada() {
        let w = world();
        let (mut a, _) = game(&w, &[managed_setup()]);
        let (mut b, _) = game(&w, &[managed_setup()]);
        a.play_to_end(&w.pack);
        b.play_to_end(&w.pack);
        assert_eq!(a.report(), b.report());
    }

    #[test]
    fn substituicao_valida_troca_o_espaco_registra_a_linha_e_conta_minutos() {
        let w = world();
        let (mut g, _) = game(&w, &[managed_setup()]);
        let side = g.managed_side().unwrap();
        g.advance(&w.pack, 60);
        let (slot_pos, out) = g.on_pitch(side)[5];
        let incoming = *g
            .bench(side)
            .iter()
            .find(|&&p| w.roster[p.as_usize()].position == slot_pos)
            .unwrap();
        g.substitute(&w.pack, &w.roster, side, out, incoming)
            .unwrap();

        assert!(g.on_pitch(side).iter().any(|&(_, p)| p == incoming));
        assert!(g.on_pitch(side).iter().all(|&(_, p)| p != out));
        assert!(!g.bench(side).contains(&incoming));
        assert_eq!(g.substitutions_left(side), 2);
        let line = g
            .report()
            .lines
            .into_iter()
            .find(|l| l.kind == MatchLineKind::Substitution)
            .unwrap();
        assert_eq!(
            (line.minute, line.player, line.other),
            (60, Some(incoming), Some(out))
        );

        g.play_to_end(&w.pack);
        let report = g.report();
        let minutes = |id| {
            report
                .players
                .iter()
                .find(|p| p.player == id)
                .unwrap()
                .minutes
        };
        assert_eq!(minutes(out), 60);
        assert_eq!(minutes(incoming), 30);
        assert!(
            !report
                .players
                .iter()
                .find(|p| p.player == incoming)
                .unwrap()
                .started
        );
    }

    #[test]
    fn substituicao_invalida_e_recusada_com_motivo() {
        let w = world();
        let (mut g, _) = game(&w, &[managed_setup()]);
        let side = g.managed_side().unwrap();
        let other = if side == Side::Home {
            Side::Away
        } else {
            Side::Home
        };
        let (_, on) = g.on_pitch(side)[3];
        let (_, on2) = g.on_pitch(side)[4];
        let bench = g.bench(side).to_vec();

        assert_eq!(
            g.substitute(&w.pack, &w.roster, other, on, bench[0]),
            Err(SubstitutionError::SideNotManaged)
        );
        assert_eq!(
            g.substitute(&w.pack, &w.roster, side, bench[0], bench[1]),
            Err(SubstitutionError::PlayerNotOnPitch)
        );
        assert_eq!(
            g.substitute(&w.pack, &w.roster, side, on, on2),
            Err(SubstitutionError::PlayerNotOnBench)
        );
        for (k, &b) in bench.iter().take(3).enumerate() {
            let (_, out) = g.on_pitch(side)[k + 1];
            g.substitute(&w.pack, &w.roster, side, out, b).unwrap();
        }
        let (_, out) = g.on_pitch(side)[7];
        assert_eq!(
            g.substitute(&w.pack, &w.roster, side, out, bench[3]),
            Err(SubstitutionError::NoSubstitutionsLeft)
        );
        g.play_to_end(&w.pack);
        assert_eq!(
            g.substitute(&w.pack, &w.roster, side, out, bench[3]),
            Err(SubstitutionError::MatchFinished)
        );
    }

    #[test]
    fn instrucoes_ofensivas_ao_vivo_geram_mais_finalizacoes_no_agregado() {
        let w = world();
        let (mut base_shots, mut tuned_shots) = (0u32, 0u32);
        for season in 0..60u32 {
            let setups = [managed_setup()];
            let mut prep = prepared(&w, &setups);
            prep.season_index = season; // chaves de RNG diferentes
            let make = |prep: &PreparedFixture| {
                LiveGame::new(
                    &w.pack,
                    &w.roster,
                    prep,
                    setups.first(),
                    &HashSet::new(),
                    42,
                )
            };
            let mut plain = make(&prep);
            let mut tuned = make(&prep);
            let side = tuned.managed_side().unwrap();
            assert!(tuned.set_instructions(
                &w.pack,
                &w.roster,
                side,
                Mentality::Attacking,
                Pressing::Normal
            ));
            plain.play_to_end(&w.pack);
            tuned.play_to_end(&w.pack);
            let pick = |g: &LiveGame| match side {
                Side::Home => g.report().shots.0,
                Side::Away => g.report().shots.1,
            };
            base_shots += pick(&plain);
            tuned_shots += pick(&tuned);
        }
        assert!(tuned_shots > base_shots, "{tuned_shots} vs {base_shots}");
    }

    #[test]
    fn instrucoes_no_adversario_da_ia_sao_recusadas() {
        let w = world();
        let (mut g, _) = game(&w, &[managed_setup()]);
        let side = g.managed_side().unwrap();
        let other = if side == Side::Home {
            Side::Away
        } else {
            Side::Home
        };
        assert!(!g.set_instructions(
            &w.pack,
            &w.roster,
            other,
            Mentality::Attacking,
            Pressing::High
        ));
        g.play_to_end(&w.pack);
        assert!(!g.set_instructions(
            &w.pack,
            &w.roster,
            side,
            Mentality::Attacking,
            Pressing::High
        ));
    }

    #[test]
    fn partida_sem_clube_do_tecnico_nao_tem_banco_nem_substituicao() {
        let w = world();
        let prep = prepared(&w, &[]);
        let g = LiveGame::new(&w.pack, &w.roster, &prep, None, &HashSet::new(), 42);
        assert!(g.managed_side().is_none());
        assert_eq!(g.substitutions_left(Side::Home), 0);
        assert!(g.bench(Side::Home).is_empty());
    }

    #[test]
    fn rescalar_com_onze_igual_nao_muda_nada_e_com_onze_melhor_aumenta() {
        let mk = |s, f, g| TeamMatchProfile {
            strength: TeamStrength::new(Fixed::from_int(s)),
            finishing: FinishingQuality::new(Fixed::from_int(f)),
            goalkeeping: GoalkeepingQuality::new(Fixed::from_int(g)),
        };
        let current = mk(100, 10, 10);
        let old = mk(100, 10, 10);
        assert_eq!(rescale(current, old, old), current);
        let better = rescale(current, old, mk(120, 12, 12));
        assert_eq!(better.strength.value(), Fixed::from_int(120));
        assert!(better.finishing.value() > current.finishing.value());
    }

    #[test]
    fn notas_ficam_na_faixa_e_premiam_gol_e_clean_sheet() {
        let acc = |goals, assists, position| PlayerStatsAcc {
            club: ClubId::new(0),
            side: Side::Home,
            position,
            started: true,
            entered: 0,
            left: None,
            shots: 0,
            goals,
            assists,
        };
        let neutral = rating_x10(&acc(0, 0, Position::Midfielder), 90, 1, 1);
        assert_eq!(neutral, 60);
        assert!(rating_x10(&acc(1, 0, Position::Forward), 90, 1, 1) > neutral);
        assert!(rating_x10(&acc(0, 1, Position::Midfielder), 90, 1, 1) > neutral);
        assert!(
            rating_x10(&acc(0, 0, Position::Defender), 90, 1, 0)
                > rating_x10(&acc(0, 0, Position::Midfielder), 90, 1, 0)
        );
        assert!(rating_x10(&acc(0, 0, Position::Goalkeeper), 90, 0, 3) < neutral);
        assert_eq!(rating_x10(&acc(9, 9, Position::Forward), 90, 9, 0), 100);
        assert_eq!(rating_x10(&acc(0, 0, Position::Goalkeeper), 90, 0, 9), 52);
        // Quem entrou aos 80 não herda nota de resultado.
        assert_eq!(rating_x10(&acc(0, 0, Position::Forward), 10, 3, 0), 60);
    }
}
