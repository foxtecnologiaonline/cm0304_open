//! Motor v0.5 — "força relativa → finalização → placar"
//! (`docs/04-motor-de-partida.md §7`). Ainda não a camada de posse por zona
//! nem a camada tática completa do v1 (`docs/04 §2.1`/`§2.2`) — nenhuma
//! zona, nenhum jogador em campo de verdade, nenhuma tática — mas já é a
//! primeira fatia real da camada de evento (`docs/04 §2.3`): um gol não é
//! mais um sorteio direto, é uma finalização que o goleiro pode defender.
//!
//! ## O modelo
//!
//! Duas etapas por minuto, para cada lado:
//!
//! 1. **Finalização aconteceu?** Mesmo mecanismo do v0: um processo de
//!    Bernoulli por minuto que aproxima Poisson por *thinning*
//!    (`Binomial(90, λ/90) ≈ Poisson(λ)`), só que agora `λ` é o número
//!    esperado de **finalizações** por partida (alvo de `docs/04 §4.1`:
//!    12,5 finalizações/time, ~25 no total), não mais de gols — daí
//!    [`MatchEvent::Shot`].
//! 2. **A finalização virou gol?** Um segundo sorteio, independente,
//!    comparando a qualidade de finalização de quem chuta
//!    ([`FinishingQuality`]) com a qualidade do goleiro adversário
//!    ([`GoalkeepingQuality`]) — a primeira comparação ponderada
//!    atacante-vs-defensor do motor (`docs/04 §2.3`): `p_gol = base ×
//!    (finalização / goleiro)`, com `base` calibrado para que, em
//!    qualidade neutra, 25 finalizações × conversão ≈ 2,7 gols/partida (o
//!    mesmo alvo do v0). Nenhuma zona, ângulo, pressão ou clima entra
//!    ainda — isso é v2/v3 (`docs/04 §7`).
//!
//! `λ_mandante`/`λ_visitante` (finalizações esperadas) continuam vindo só
//! da força relativa dos dois times, exatamente como no v0:
//!
//! ```text
//! base_chutes = TOTAL_SHOTS_TARGET / (1 + HOME_ADVANTAGE)
//! λ_home = base_chutes × HOME_ADVANTAGE × (força_home / média(força_home, força_away))
//! λ_away = base_chutes ×                  (força_away / média(força_home, força_away))
//! ```
//!
//! Nenhuma das constantes abaixo é calibrada de verdade ainda — são pontos
//! de partida plausíveis, iguais em espírito ao v0, a ajustar com
//! `managerfc-cli calibrate` e eventualmente migrar para dado
//! (`packs/core/engine.toml`, RNF-23) em vez de ficar hard-coded aqui.

use domain::{DeterministicRng, Fixed};

use crate::event::{MatchEvent, Side};

/// Duração da partida — sem acréscimos (chegam com cartões/lesões).
const MINUTES: u8 = 90;

/// Piso de força — nunca zero, para a razão de força nunca dividir por zero
/// nem produzir um `λ` negativo.
const MIN_STRENGTH: Fixed = Fixed::from_ratio(1, 100);

/// Piso de qualidade de finalização/goleiro — mesma função de `MIN_STRENGTH`,
/// mas numa escala menor (atributos 1-20, `docs/03 §3.1`, não CA 0-200):
/// nunca zero, pra razão finalização/goleiro nunca dividir por zero.
const MIN_QUALITY: Fixed = Fixed::from_ratio(1, 10);

/// Teto de finalizações esperadas aplicado ao `λ` final, não à força de
/// entrada — evita que um pack com forças absurdamente desiguais produza
/// uma probabilidade por minuto maior que 100% (`docs/03 §7`, validação de
/// pack, é a primeira linha de defesa; isto é a segunda, dentro do motor).
const MAX_LAMBDA: Fixed = Fixed::from_int(40);

/// Total de finalizações esperado por partida quando as forças são iguais
/// — `docs/04 §4.1`: alvo de 12,5 finalizações/time, ×2 lados.
const TOTAL_SHOTS_TARGET: Fixed = Fixed::from_int(25);

/// Vantagem de mando: razão `λ_home / λ_away` quando as forças são iguais.
/// Recalibrado de 1,4 (valor herdado do v0) para 1,25: com a conversão em
/// duas etapas do v0.5, 1,4 deixava vitórias do mandante na borda/fora da
/// tolerância de `docs/04 §4.1` (44% ± 3pp) — `managerfc-cli calibrate
/// --check` media 47-48% em várias seeds com 50 temporadas. Com 1,25, 6
/// seeds testadas (1/7/42/100/999/55555 × 50 temporadas) ficam todas entre
/// 43-45%, dentro da tolerância sem exceção. `sim_version` (persist) foi
/// incrementado de 0 para 1 junto desta mudança (`docs/08 §3`).
const HOME_ADVANTAGE: Fixed = Fixed::from_ratio(5, 4);

/// Conversão de finalização em gol quando finalização e goleiro têm
/// qualidade igual — escolhido para que `TOTAL_SHOTS_TARGET ×
/// BASELINE_CONVERSION_PERMILLE ≈` o alvo de gols/partida do v0 (2,7):
/// `25 × 0,108 = 2,7`, exato. `docs/04 §4.1` também mira 10,5% de
/// conversão — 10,8% fica dentro da tolerância documentada (±1,5pp).
const BASELINE_CONVERSION_PERMILLE: i64 = 108;

/// Piso e teto da conversão de finalização em gol, aplicados **depois** da
/// razão finalização/goleiro — mesmo papel de `MAX_LAMBDA`: um pack com
/// qualidades extremamente desiguais não pode produzir "toda finalização
/// vira gol" nem "nenhuma finalização nunca converte".
const MIN_CONVERSION_PERMILLE: i64 = 10;
const MAX_CONVERSION_PERMILLE: i64 = 400;

/// Força relativa de um time, para efeito do motor (volume de
/// finalizações, não mais o placar direto desde o v0.5).
///
/// É um número positivo em escala arbitrária — o motor compara duas forças
/// entre si, nunca contra um valor absoluto. Calcular esse número a partir
/// de um elenco de verdade (atributos, moral, condição — `docs/04 §2.1`) é
/// trabalho de `world`/`ai`; o motor não sabe, e não precisa saber, de onde
/// o número veio.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TeamStrength(Fixed);

impl TeamStrength {
    /// Constrói a força, aplicando o piso de `MIN_STRENGTH` — nunca zero ou
    /// negativa, não importa o que o chamador passe.
    #[must_use]
    pub fn new(value: Fixed) -> Self {
        Self(value.max(MIN_STRENGTH))
    }

    #[must_use]
    pub fn value(self) -> Fixed {
        self.0
    }
}

/// Qualidade de finalização de um time — primeira vez que o motor usa
/// atributos individuais de jogador (`docs/03 §3.1`: finalização, frieza),
/// não só um agregado de CA. Escala esperada: a mesma dos atributos
/// visíveis, 1-20 (`docs/03 §4`), mas o tipo aceita qualquer `Fixed`
/// positivo — quem calcula (`world`) decide a fórmula exata.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FinishingQuality(Fixed);

impl FinishingQuality {
    #[must_use]
    pub fn new(value: Fixed) -> Self {
        Self(value.max(MIN_QUALITY))
    }

    #[must_use]
    pub fn value(self) -> Fixed {
        self.0
    }
}

/// Qualidade de goleiro de um time — mesma ideia de [`FinishingQuality`],
/// do lado defensivo (reflexos, posicionamento, jogo aéreo, `docs/04 §2.3`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GoalkeepingQuality(Fixed);

impl GoalkeepingQuality {
    #[must_use]
    pub fn new(value: Fixed) -> Self {
        Self(value.max(MIN_QUALITY))
    }

    #[must_use]
    pub fn value(self) -> Fixed {
        self.0
    }
}

/// Tudo que o motor precisa de **um** time para simular uma partida — o
/// agrupamento existe pra `simulate` não precisar de seis parâmetros soltos
/// e pra crescer (futuros modificadores táticos, condição, moral) sem
/// quebrar a assinatura de novo.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TeamMatchProfile {
    pub strength: TeamStrength,
    pub finishing: FinishingQuality,
    pub goalkeeping: GoalkeepingQuality,
}

/// Moduladores táticos de um time (RF-PA-02, `docs/01 §2.5`) — o único
/// ponto por onde formação, mentalidade e pressão chegam ao motor v0.5, que
/// ainda não tem zonas nem jogadores em campo (`docs/04 §7`): tática vira
/// **taxa de finalização**, não posicionamento. Valores em milésimos;
/// [`Tactics::NEUTRAL`] (1000/1000) não muda nada — `simulate` usa
/// exatamente esse valor, então o caminho sem tática continua byte a byte o
/// de antes.
///
/// Cada lado tem duas alavancas, e o par é o que cria o dilema tático:
/// o que o time **cria** (`shot_volume_permille`, multiplica as
/// finalizações do próprio time) e o que ele **cede**
/// (`concession_permille`, multiplica as finalizações do adversário).
/// Atacar mais tende a subir os dois; defender mais, a baixar os dois.
/// Nenhum valor é calibrado ainda (`docs/04 §4`): pontos de partida
/// plausíveis, a ajustar quando houver dado.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Tactics {
    pub shot_volume_permille: u32,
    pub concession_permille: u32,
}

impl Tactics {
    /// Sem tática: as taxas saem exatamente da força relativa dos times.
    pub const NEUTRAL: Tactics = Tactics {
        shot_volume_permille: 1000,
        concession_permille: 1000,
    };

    /// Piso e teto de cada alavanca — nenhuma combinação de instruções
    /// zera as finalizações de um lado nem as multiplica por mais que ~2.
    const MIN_PERMILLE: u32 = 500;
    const MAX_PERMILLE: u32 = 2000;

    /// Constrói limitando cada alavanca a `MIN_PERMILLE..=MAX_PERMILLE`.
    #[must_use]
    pub fn new(shot_volume_permille: u32, concession_permille: u32) -> Self {
        Self {
            shot_volume_permille: shot_volume_permille
                .clamp(Self::MIN_PERMILLE, Self::MAX_PERMILLE),
            concession_permille: concession_permille.clamp(Self::MIN_PERMILLE, Self::MAX_PERMILLE),
        }
    }

    fn is_neutral(self) -> bool {
        self == Self::NEUTRAL
    }
}

/// O que identifica *esta* partida para fins de derivação de RNG
/// (`docs/02 §5`): mundo + partida, nada mais. `fixture` deveria ser
/// `domain::FixtureId` assim que `world` existir para atribuí-lo; por ora
/// aceita qualquer `u64` estável (ex.: o índice denso do confronto no
/// calendário de exemplo usado pelos testes/CLI).
#[derive(Debug, Clone, Copy)]
pub struct MatchContext {
    pub world_seed: u64,
    pub fixture: u64,
}

/// Simula uma partida do início ao fim e devolve o fluxo de eventos
/// completo (`KickOff` → (`Shot` [`Goal`])* → `FullTime`).
///
/// Determinístico: o mesmo `(home, away, ctx)` produz sempre a mesma
/// sequência de eventos, em qualquer plataforma (`ADR 0002`) — é o que os
/// testes de propriedade deste módulo verificam. Cada finalização e cada
/// conversão são sorteios independentes da mesma sequência de RNG (um
/// `Rng` por partida, `docs/02 §5`), na ordem minuto a minuto, mandante
/// antes de visitante — mudar essa ordem muda o resultado, por isso é
/// parte do contrato de determinismo, não um detalhe de implementação.
#[must_use]
pub fn simulate(
    home: TeamMatchProfile,
    away: TeamMatchProfile,
    ctx: MatchContext,
) -> Vec<MatchEvent> {
    simulate_tactical(home, away, Tactics::NEUTRAL, Tactics::NEUTRAL, ctx)
}

/// [`simulate`] com [`Tactics`] de cada lado. Com as duas neutras é
/// exatamente [`simulate`] (mesma sequência de RNG, mesmos eventos).
/// As finalizações esperadas de cada lado saem de
/// `λ × volume_próprio × concessão_do_adversário`, ainda limitadas por
/// `MAX_LAMBDA`; a conversão em gol não muda (é qualidade, não tática).
#[must_use]
pub fn simulate_tactical(
    home: TeamMatchProfile,
    away: TeamMatchProfile,
    home_tactics: Tactics,
    away_tactics: Tactics,
    ctx: MatchContext,
) -> Vec<MatchEvent> {
    let mut live = LiveMatch::new(home, away, home_tactics, away_tactics, ctx);
    while !live.is_finished() {
        live.step();
    }
    live.into_events()
}

/// Taxas por minuto de uma partida, derivadas dos dois perfis e das duas
/// táticas — recalculadas quando algum deles muda no meio do jogo.
#[derive(Debug, Clone, Copy)]
struct Rates {
    /// Probabilidade de finalização por minuto, em milésimos: `[mandante, visitante]`.
    shot_per_minute: [u32; 2],
    /// Probabilidade de a finalização virar gol, em milésimos.
    conversion: [u32; 2],
}

fn rates(profiles: &[TeamMatchProfile; 2], tactics: &[Tactics; 2]) -> Rates {
    let [home, away] = *profiles;
    let (mut lambda_home, mut lambda_away) = expected_shots(home.strength, away.strength);
    if !tactics[0].is_neutral() || !tactics[1].is_neutral() {
        let scale = |lambda: Fixed, own: Tactics, other: Tactics| {
            let factor = Fixed::from_ratio(i64::from(own.shot_volume_permille), 1000)
                * Fixed::from_ratio(i64::from(other.concession_permille), 1000);
            (lambda * factor).min(MAX_LAMBDA)
        };
        lambda_home = scale(lambda_home, tactics[0], tactics[1]);
        lambda_away = scale(lambda_away, tactics[1], tactics[0]);
    }
    Rates {
        shot_per_minute: [
            per_minute_permille(lambda_home),
            per_minute_permille(lambda_away),
        ],
        conversion: [
            conversion_permille(home.finishing, away.goalkeeping),
            conversion_permille(away.finishing, home.goalkeeping),
        ],
    }
}

const fn side_index(side: Side) -> usize {
    match side {
        Side::Home => 0,
        Side::Away => 1,
    }
}

/// Uma partida jogada **minuto a minuto**: dá pontos de pausa para o
/// técnico mexer em tática ou time no meio do jogo (RF-PA-06) sem perder o
/// determinismo. Sem nenhuma intervenção, produz exatamente os mesmos
/// eventos que [`simulate_tactical`] — que, aliás, é só este laço rodado até
/// o fim (uma única fonte da verdade). A sequência de RNG por minuto é
/// parte do contrato: mandante antes de visitante, finalização antes da
/// conversão.
///
/// Trocar perfil/tática ([`LiveMatch::set_team`]) vale **do próximo minuto
/// em diante**; o passado não muda.
#[derive(Debug)]
pub struct LiveMatch {
    rng: DeterministicRng,
    profiles: [TeamMatchProfile; 2],
    tactics: [Tactics; 2],
    rates: Rates,
    /// Minutos já jogados (0 = antes do apito inicial).
    minute: u8,
    goals: [u32; 2],
    shots: [u32; 2],
    /// Soma, por lado, da probabilidade de gol de cada finalização, em
    /// milésimos de gol — o xG do próprio modelo (não uma estimativa
    /// externa): cada finalização vale `conversion` milésimos.
    xg_milli: [u32; 2],
    events: Vec<MatchEvent>,
}

impl LiveMatch {
    #[must_use]
    pub fn new(
        home: TeamMatchProfile,
        away: TeamMatchProfile,
        home_tactics: Tactics,
        away_tactics: Tactics,
        ctx: MatchContext,
    ) -> Self {
        let profiles = [home, away];
        let tactics = [home_tactics, away_tactics];
        let mut events = Vec::with_capacity(8); // a maioria das partidas tem poucas finalizações convertidas
        events.push(MatchEvent::KickOff);
        Self {
            rng: DeterministicRng::seeded(ctx.world_seed, "engine.v0.match", ctx.fixture, 0),
            rates: rates(&profiles, &tactics),
            profiles,
            tactics,
            minute: 0,
            goals: [0, 0],
            shots: [0, 0],
            xg_milli: [0, 0],
            events,
        }
    }

    /// Minutos já jogados, `0..=90`.
    #[must_use]
    pub fn minute(&self) -> u8 {
        self.minute
    }

    #[must_use]
    pub fn is_finished(&self) -> bool {
        self.minute >= MINUTES
    }

    /// `(gols do mandante, gols do visitante)` até agora.
    #[must_use]
    pub fn score(&self) -> (u32, u32) {
        (self.goals[0], self.goals[1])
    }

    /// `(finalizações do mandante, do visitante)` até agora.
    #[must_use]
    pub fn shots(&self) -> (u32, u32) {
        (self.shots[0], self.shots[1])
    }

    /// xG acumulado `(mandante, visitante)`, em milésimos de gol.
    #[must_use]
    pub fn xg_milli(&self) -> (u32, u32) {
        (self.xg_milli[0], self.xg_milli[1])
    }

    /// Todos os eventos até agora, na ordem (`KickOff` primeiro).
    #[must_use]
    pub fn events(&self) -> &[MatchEvent] {
        &self.events
    }

    #[must_use]
    pub fn into_events(self) -> Vec<MatchEvent> {
        self.events
    }

    /// Troca o perfil e a tática de um lado, do próximo minuto em diante
    /// (substituição, mudança de instruções). Recalcula as taxas dos dois
    /// lados, já que `λ` depende da força relativa.
    pub fn set_team(&mut self, side: Side, profile: TeamMatchProfile, tactics: Tactics) {
        let i = side_index(side);
        self.profiles[i] = profile;
        self.tactics[i] = tactics;
        self.rates = rates(&self.profiles, &self.tactics);
    }

    /// Joga o próximo minuto; devolve quantos eventos novos surgiram. No-op
    /// (devolve 0) depois do apito final. No minuto 90 acrescenta `FullTime`.
    pub fn step(&mut self) -> usize {
        if self.is_finished() {
            return 0;
        }
        let before = self.events.len();
        self.minute += 1;
        let minute = self.minute;
        for side in [Side::Home, Side::Away] {
            let i = side_index(side);
            if self.rng.chance_per_mille(self.rates.shot_per_minute[i]) {
                self.shots[i] += 1;
                self.xg_milli[i] += self.rates.conversion[i];
                self.events.push(MatchEvent::Shot { minute, side });
                if self.rng.chance_per_mille(self.rates.conversion[i]) {
                    self.goals[i] += 1;
                    self.events.push(MatchEvent::Goal { minute, side });
                }
            }
        }
        if minute == MINUTES {
            self.events.push(MatchEvent::FullTime {
                home_goals: self.goals[0],
                away_goals: self.goals[1],
            });
        }
        self.events.len() - before
    }

    /// Joga até `minutes` minutos (menos, se a partida acabar antes);
    /// devolve quantos minutos foram de fato jogados.
    pub fn step_minutes(&mut self, minutes: u8) -> u8 {
        let start = self.minute;
        for _ in 0..minutes {
            if self.is_finished() {
                break;
            }
            self.step();
        }
        self.minute - start
    }
}

/// Calcula `(λ_home, λ_away)` — finalizações esperadas na partida inteira
/// — a partir das duas forças. Mesma fórmula do v0 (`expected_goals`),
/// só que agora o alvo é `TOTAL_SHOTS_TARGET`, não gols diretos — ver a
/// fórmula no doc do módulo.
fn expected_shots(home: TeamStrength, away: TeamStrength) -> (Fixed, Fixed) {
    let base = TOTAL_SHOTS_TARGET / (Fixed::ONE + HOME_ADVANTAGE);
    let average = (home.0 + away.0) / Fixed::from_int(2);
    let ratio_home = home.0 / average;
    let ratio_away = away.0 / average;

    let lambda_home = (base * HOME_ADVANTAGE * ratio_home).min(MAX_LAMBDA);
    let lambda_away = (base * ratio_away).min(MAX_LAMBDA);
    (lambda_home, lambda_away)
}

/// Converte `λ` (finalizações esperadas na partida inteira) em
/// probabilidade por minuto, em milésimos — a escala que
/// `DeterministicRng::chance_per_mille` espera. Arredonda para o mais
/// próximo (não trunca): `round(λ×1000/90)`.
fn per_minute_permille(lambda: Fixed) -> u32 {
    let milli = i64::from(lambda.raw_milli().max(0));
    let minutes = i64::from(MINUTES);
    (((milli + minutes / 2) / minutes) as u32).min(1000)
}

/// Probabilidade de uma finalização virar gol, em milésimos — razão entre
/// a qualidade de finalização de quem chuta e a qualidade do goleiro
/// adversário, escalada por `BASELINE_CONVERSION_PERMILLE` e limitada a
/// `MIN_CONVERSION_PERMILLE..=MAX_CONVERSION_PERMILLE`.
fn conversion_permille(finishing: FinishingQuality, goalkeeping: GoalkeepingQuality) -> u32 {
    let ratio = finishing.0 / goalkeeping.0;
    let scaled = Fixed::from_int(BASELINE_CONVERSION_PERMILLE as i32) * ratio;
    let rounded = (i64::from(scaled.raw_milli()) + 500) / 1000;
    rounded.clamp(MIN_CONVERSION_PERMILLE, MAX_CONVERSION_PERMILLE) as u32
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::{score, shots};

    fn ctx(fixture: u64) -> MatchContext {
        MatchContext {
            world_seed: 42,
            fixture,
        }
    }

    fn neutral_quality() -> (FinishingQuality, GoalkeepingQuality) {
        (
            FinishingQuality::new(Fixed::from_int(10)),
            GoalkeepingQuality::new(Fixed::from_int(10)),
        )
    }

    fn profile(strength: i32) -> TeamMatchProfile {
        let (finishing, goalkeeping) = neutral_quality();
        TeamMatchProfile {
            strength: TeamStrength::new(Fixed::from_int(strength)),
            finishing,
            goalkeeping,
        }
    }

    fn equal_profiles() -> (TeamMatchProfile, TeamMatchProfile) {
        (profile(100), profile(100))
    }

    #[test]
    fn partida_ao_vivo_sem_intervencao_e_identica_a_simulate_completa() {
        let (home, away) = (profile(130), profile(85));
        let tactics = [Tactics::NEUTRAL, Tactics::new(1120, 1150)];
        for fixture in 0..300 {
            let whole = simulate_tactical(home, away, tactics[0], tactics[1], ctx(fixture));
            let mut live = LiveMatch::new(home, away, tactics[0], tactics[1], ctx(fixture));
            // Em fatias irregulares: o tamanho do passo não pode importar.
            for chunk in [1u8, 7, 30, 2, 50, 10] {
                live.step_minutes(chunk);
            }
            assert!(live.is_finished());
            assert_eq!(live.events(), whole.as_slice(), "fixture {fixture}");
        }
    }

    #[test]
    fn partida_ao_vivo_acumula_placar_finalizacoes_e_xg_coerentes_com_os_eventos() {
        let (home, away) = equal_profiles();
        let mut live = LiveMatch::new(home, away, Tactics::NEUTRAL, Tactics::NEUTRAL, ctx(11));
        assert_eq!(live.minute(), 0);
        assert_eq!(live.events(), &[MatchEvent::KickOff]);
        while !live.is_finished() {
            live.step();
        }
        assert_eq!(live.minute(), 90);
        assert_eq!(live.score(), score(live.events()));
        assert_eq!(live.shots(), shots(live.events()));
        // xG = Σ conversão por finalização; com qualidade neutra, 108‰ cada.
        let (hs, as_) = live.shots();
        assert_eq!(live.xg_milli(), (hs * 108, as_ * 108));
        // Depois do apito final, nada muda.
        assert_eq!(live.step(), 0);
        assert_eq!(live.step_minutes(10), 0);
        assert!(matches!(
            live.events().last(),
            Some(MatchEvent::FullTime { .. })
        ));
    }

    #[test]
    fn trocar_o_time_no_meio_so_afeta_os_minutos_seguintes() {
        let (home, away) = equal_profiles();
        for fixture in 0..100 {
            let mut plain =
                LiveMatch::new(home, away, Tactics::NEUTRAL, Tactics::NEUTRAL, ctx(fixture));
            let mut changed =
                LiveMatch::new(home, away, Tactics::NEUTRAL, Tactics::NEUTRAL, ctx(fixture));
            plain.step_minutes(45);
            changed.step_minutes(45);
            assert_eq!(plain.events(), changed.events());
            changed.set_team(Side::Home, profile(160), Tactics::new(1300, 1000));
            plain.step_minutes(45);
            changed.step_minutes(45);
            // O primeiro tempo é idêntico, mesmo depois de a partida acabar.
            let first_half = |m: &LiveMatch| {
                m.events()
                    .iter()
                    .filter(|e| match e {
                        MatchEvent::Shot { minute, .. } | MatchEvent::Goal { minute, .. } => {
                            *minute <= 45
                        }
                        _ => true,
                    })
                    .count()
            };
            assert_eq!(first_half(&plain), first_half(&changed));
        }
    }

    #[test]
    fn reforcar_o_time_no_intervalo_gera_mais_finalizacoes_no_segundo_tempo() {
        let (home, away) = equal_profiles();
        let (mut base, mut boosted) = (0u32, 0u32);
        for fixture in 0..1500 {
            let mut a =
                LiveMatch::new(home, away, Tactics::NEUTRAL, Tactics::NEUTRAL, ctx(fixture));
            let mut b =
                LiveMatch::new(home, away, Tactics::NEUTRAL, Tactics::NEUTRAL, ctx(fixture));
            a.step_minutes(45);
            b.step_minutes(45);
            let (before_a, _) = a.shots();
            let (before_b, _) = b.shots();
            b.set_team(Side::Home, profile(200), Tactics::NEUTRAL);
            a.step_minutes(45);
            b.step_minutes(45);
            base += a.shots().0 - before_a;
            boosted += b.shots().0 - before_b;
        }
        assert!(boosted > base + base / 5, "{boosted} vs {base}");
    }

    #[test]
    fn tatica_neutra_e_identica_a_simulate_sem_tatica() {
        let (home, away) = (profile(120), profile(90));
        for fixture in 0..200 {
            assert_eq!(
                simulate_tactical(home, away, Tactics::NEUTRAL, Tactics::NEUTRAL, ctx(fixture)),
                simulate(home, away, ctx(fixture))
            );
        }
    }

    /// Média de (finalizações próprias, finalizações do adversário,
    /// saldo de gols) do mandante em `n` partidas entre perfis iguais.
    fn averages(home_tactics: Tactics, away_tactics: Tactics, n: u64) -> (f64, f64, f64) {
        let (home, away) = equal_profiles();
        let (mut own, mut conceded, mut diff) = (0u64, 0u64, 0i64);
        for fixture in 0..n {
            let events = simulate_tactical(home, away, home_tactics, away_tactics, ctx(fixture));
            let (hs, as_) = shots(&events);
            let (hg, ag) = score(&events);
            own += u64::from(hs);
            conceded += u64::from(as_);
            diff += i64::from(hg) - i64::from(ag);
        }
        (
            own as f64 / n as f64,
            conceded as f64 / n as f64,
            diff as f64 / n as f64,
        )
    }

    #[test]
    fn volume_proprio_maior_gera_mais_finalizacoes_proprias_e_nao_mexe_nas_do_adversario() {
        let (base_own, base_conceded, _) = averages(Tactics::NEUTRAL, Tactics::NEUTRAL, 3000);
        let (own, conceded, _) = averages(Tactics::new(1300, 1000), Tactics::NEUTRAL, 3000);
        assert!(own > base_own * 1.15, "{own} vs {base_own}");
        assert!((conceded - base_conceded).abs() < base_conceded * 0.06);
    }

    #[test]
    fn concessao_maior_do_proprio_time_gera_mais_finalizacoes_do_adversario() {
        let (_, base_conceded, _) = averages(Tactics::NEUTRAL, Tactics::NEUTRAL, 3000);
        let (_, conceded, _) = averages(Tactics::new(1000, 1300), Tactics::NEUTRAL, 3000);
        assert!(
            conceded > base_conceded * 1.15,
            "{conceded} vs {base_conceded}"
        );
    }

    #[test]
    fn tatica_e_determinista_para_a_mesma_entrada() {
        let (home, away) = equal_profiles();
        let t = Tactics::new(1120, 1150);
        assert_eq!(
            simulate_tactical(home, away, t, Tactics::NEUTRAL, ctx(7)),
            simulate_tactical(home, away, t, Tactics::NEUTRAL, ctx(7))
        );
    }

    #[test]
    fn alavancas_sao_limitadas_a_faixa_segura() {
        let t = Tactics::new(0, 100_000);
        assert_eq!(t.shot_volume_permille, 500);
        assert_eq!(t.concession_permille, 2000);
        // Mesmo no extremo, o λ continua sob MAX_LAMBDA e a partida roda.
        let (home, away) = equal_profiles();
        let events = simulate_tactical(home, away, t, t, ctx(1));
        assert!(matches!(events.last(), Some(MatchEvent::FullTime { .. })));
    }

    #[test]
    fn partida_comeca_com_kickoff_e_termina_com_fulltime_coerente() {
        let (home, away) = equal_profiles();
        let events = simulate(home, away, ctx(1));
        assert_eq!(events.first(), Some(&MatchEvent::KickOff));
        let (home_goals, away_goals) = score(&events);
        match events.last() {
            Some(MatchEvent::FullTime {
                home_goals: h,
                away_goals: a,
            }) => {
                assert_eq!(*h, home_goals);
                assert_eq!(*a, away_goals);
            }
            other => panic!("último evento deveria ser FullTime, veio {other:?}"),
        }
    }

    #[test]
    fn todo_gol_e_precedido_por_uma_finalizacao_no_mesmo_minuto_e_lado() {
        let (home, away) = equal_profiles();
        let events = simulate(home, away, ctx(3));
        for (i, event) in events.iter().enumerate() {
            if let MatchEvent::Goal { minute, side } = event {
                assert_eq!(
                    events[i - 1],
                    MatchEvent::Shot {
                        minute: *minute,
                        side: *side
                    },
                    "gol no minuto {minute} ({side:?}) sem a finalização correspondente logo antes"
                );
            }
        }
    }

    #[test]
    fn numero_de_finalizacoes_e_sempre_maior_ou_igual_ao_numero_de_gols() {
        let (home, away) = equal_profiles();
        let events = simulate(home, away, ctx(9));
        let (shots_home, shots_away) = shots(&events);
        let (goals_home, goals_away) = score(&events);
        assert!(shots_home >= goals_home);
        assert!(shots_away >= goals_away);
    }

    #[test]
    fn mesmo_contexto_produz_a_mesma_partida_sempre() {
        let (home, away) = equal_profiles();
        let a = simulate(home, away, ctx(7));
        let b = simulate(home, away, ctx(7));
        assert_eq!(a, b);
    }

    #[test]
    fn fixtures_diferentes_produzem_partidas_independentes() {
        let (home, away) = equal_profiles();
        let a = simulate(home, away, ctx(1));
        let b = simulate(home, away, ctx(2));
        assert_ne!(
            a, b,
            "duas partidas com fixtures diferentes vieram idênticas — RNG correlacionado?"
        );
    }

    #[test]
    fn forcas_iguais_preservam_o_total_e_a_vantagem_de_mando() {
        // Com forças iguais, λ_home ≠ λ_away — é exatamente aí que a
        // vantagem de mando entra. As duas invariantes que valem são outras:
        // a soma bate com o alvo de finalizações/partida, e a razão entre os
        // dois bate com HOME_ADVANTAGE.
        let (home, away) = equal_profiles();
        let (lambda_home, lambda_away) = expected_shots(home.strength, away.strength);

        let sum = lambda_home + lambda_away;
        let sum_diff = (sum - TOTAL_SHOTS_TARGET).abs();
        assert!(
            sum_diff.raw_milli() <= 2,
            "soma dos λ ({sum}) deveria ficar perto do alvo ({TOTAL_SHOTS_TARGET})"
        );

        assert!(
            lambda_home > lambda_away,
            "λ_home ({lambda_home}) deveria ser maior que λ_away ({lambda_away}) — vantagem de mando"
        );
        let ratio = lambda_home / lambda_away;
        let ratio_diff = (ratio - HOME_ADVANTAGE).abs();
        assert!(
            ratio_diff.raw_milli() <= 5,
            "razão λ_home/λ_away ({ratio}) deveria ficar perto de HOME_ADVANTAGE ({HOME_ADVANTAGE})"
        );
    }

    #[test]
    fn melhor_finalizador_contra_pior_goleiro_converte_mais_que_o_contrario() {
        // A comparação ponderada atacante-vs-defensor de verdade
        // (`docs/04 §2.3`): mesma força de time (mesmo volume de
        // finalizações), mas o mandante tem finalização muito melhor e o
        // visitante tem o goleiro muito pior — o mandante tem que marcar
        // muito mais, não só empatar no volume de chutes.
        let strong_finishing = FinishingQuality::new(Fixed::from_int(19));
        let weak_finishing = FinishingQuality::new(Fixed::from_int(5));
        let strong_keeper = GoalkeepingQuality::new(Fixed::from_int(19));
        let weak_keeper = GoalkeepingQuality::new(Fixed::from_int(5));

        let home = TeamMatchProfile {
            strength: TeamStrength::new(Fixed::from_int(100)),
            finishing: strong_finishing,
            goalkeeping: strong_keeper,
        };
        let away = TeamMatchProfile {
            strength: TeamStrength::new(Fixed::from_int(100)),
            finishing: weak_finishing,
            goalkeeping: weak_keeper,
        };

        let trials = 300u64;
        let mut home_goals_total = 0u32;
        let mut away_goals_total = 0u32;
        for i in 0..trials {
            let events = simulate(home, away, ctx(2000 + i));
            let (h, a) = score(&events);
            home_goals_total += h;
            away_goals_total += a;
        }
        assert!(
            home_goals_total > away_goals_total * 2,
            "mandante com finalização forte vs. goleiro fraco do visitante deveria \
             marcar muito mais ({home_goals_total} vs {away_goals_total})"
        );
    }

    #[test]
    fn time_muito_mais_forte_vence_a_grande_maioria_das_simulacoes() {
        // A verificação mais barata de que "força relativa" de fato importa
        // — o embrião do que vira o torneio anti-exploit completo no v2/v3
        // (docs/04 §4.3, docs/08 §5).
        let strong = profile(500);
        let weak = profile(20);

        let trials = 500;
        let home_wins = (0..trials)
            .filter(|&i| {
                let events = simulate(strong, weak, ctx(1000 + i));
                let (h, a) = score(&events);
                h > a
            })
            .count();

        let win_rate_percent = home_wins * 100 / trials as usize;
        assert!(
            win_rate_percent >= 90,
            "time muito mais forte só venceu {win_rate_percent}% das {trials} simulações"
        );
    }

    #[test]
    fn media_de_gols_com_forcas_iguais_fica_perto_do_alvo_de_calibracao() {
        // Sanidade estatística grosseira contra o alvo de docs/04 §4.1 (2,7
        // gols/partida) — não é a suíte de calibração de verdade (docs/08
        // §4), só evita que o motor fique grosseiramente errado sem que
        // nenhum teste perceba.
        let (home, away) = equal_profiles();
        let trials = 2000u32;
        let total_goals: u32 = (0..trials)
            .map(|i| {
                let events = simulate(home, away, ctx(u64::from(i)));
                let (h, a) = score(&events);
                h + a
            })
            .sum();
        let average_milli = u64::from(total_goals) * 1000 / u64::from(trials);
        // Alvo 2700 (2,7); folga generosa (±500, ~18%) — a composição de
        // duas etapas de Bernoulli (finalização × conversão) tem mais
        // variância que o sorteio direto do v0, então a tolerância aqui é
        // um pouco maior que a do v0 original. A calibração fina fica para
        // o comando dedicado.
        assert!(
            (2200..=3200).contains(&average_milli),
            "média observada {average_milli}/1000 gols por partida, esperado ~2700 (±500)"
        );
    }

    #[test]
    fn media_de_finalizacoes_com_forcas_iguais_fica_perto_do_alvo_de_calibracao() {
        // Mesma sanidade grosseira, agora para o alvo de finalizações de
        // docs/04 §4.1 (12,5/time, 25 no total) — métrica que o v0 não
        // produzia (não existia `Shot`).
        let (home, away) = equal_profiles();
        let trials = 2000u32;
        let total_shots: u32 = (0..trials)
            .map(|i| {
                let events = simulate(home, away, ctx(u64::from(i) + 1_000_000));
                let (h, a) = shots(&events);
                h + a
            })
            .sum();
        let average = total_shots / trials;
        assert!(
            (18..=32).contains(&average),
            "média observada {average} finalizações por partida, esperado ~25 (±~25%)"
        );
    }
}

/// Testes de propriedade (`docs/08 §1`) — o mesmo papel que já pagou dividendos
/// em `domain` (dois bugs reais achados por generalizar exemplos fixos):
/// aqui generalizam o determinismo e os limites de `per_minute_permille`/
/// `conversion_permille` para entradas arbitrárias, não só os poucos casos
/// escolhidos à mão acima.
#[cfg(test)]
mod proptests {
    use super::*;
    use crate::event::score;
    use proptest::prelude::*;

    fn arbitrary_profile(strength_raw: i32, quality_raw: i32) -> TeamMatchProfile {
        TeamMatchProfile {
            strength: TeamStrength::new(Fixed::from_milli(strength_raw)),
            finishing: FinishingQuality::new(Fixed::from_milli(quality_raw)),
            goalkeeping: GoalkeepingQuality::new(Fixed::from_milli(quality_raw)),
        }
    }

    proptest! {
        /// `simulate` é uma função pura do seu input — mesmo perfil, mesmo
        /// contexto, mesmos eventos, para qualquer combinação, não só o par
        /// fixo usado em `mesmo_contexto_produz_a_mesma_partida_sempre`.
        #[test]
        fn simulate_e_determinista_para_qualquer_entrada(
            home_raw in 1_000i32..1_000_000i32,
            away_raw in 1_000i32..1_000_000i32,
            quality_raw in 100i32..20_000i32,
            world_seed: u64,
            fixture: u64,
        ) {
            let home = arbitrary_profile(home_raw, quality_raw);
            let away = arbitrary_profile(away_raw, quality_raw);
            let ctx = MatchContext { world_seed, fixture };

            let a = simulate(home, away, ctx);
            let b = simulate(home, away, ctx);
            prop_assert_eq!(a, b);
        }

        /// A lista de eventos sempre começa com `KickOff`, sempre termina com
        /// `FullTime`, e o placar do `FullTime` sempre bate com `score()` —
        /// para qualquer perfil e qualquer seed, não só os casos de exemplo.
        #[test]
        fn eventos_tem_forma_valida_para_qualquer_entrada(
            home_raw in 1_000i32..1_000_000i32,
            away_raw in 1_000i32..1_000_000i32,
            quality_raw in 100i32..20_000i32,
            world_seed: u64,
            fixture: u64,
        ) {
            let home = arbitrary_profile(home_raw, quality_raw);
            let away = arbitrary_profile(away_raw, quality_raw);
            let events = simulate(home, away, MatchContext { world_seed, fixture });

            prop_assert_eq!(events.first(), Some(&MatchEvent::KickOff));
            let (home_goals, away_goals) = score(&events);
            match events.last() {
                Some(MatchEvent::FullTime { home_goals: h, away_goals: a }) => {
                    prop_assert_eq!(*h, home_goals);
                    prop_assert_eq!(*a, away_goals);
                }
                other => prop_assert!(false, "último evento não é FullTime: {other:?}"),
            }
        }

        /// `per_minute_permille` nunca estoura 1000‰ (100%) nem para um `λ`
        /// bem maior que qualquer coisa que `expected_shots` produziria na
        /// prática — importa porque `chance_per_mille` trata `>= 1000` como
        /// "sempre", então um estouro silencioso viraria "finalização
        /// garantida todo minuto" em vez de um erro visível.
        #[test]
        fn per_minute_permille_nunca_passa_de_mil(raw in 0i32..1_000_000i32) {
            let value = per_minute_permille(Fixed::from_milli(raw));
            prop_assert!(value <= 1000);
        }

        /// `conversion_permille` nunca sai de `MIN_CONVERSION_PERMILLE..=MAX_CONVERSION_PERMILLE`,
        /// para qualquer combinação de qualidades positivas — mesma garantia
        /// de `per_minute_permille`, do lado da conversão.
        #[test]
        fn conversion_permille_sempre_dentro_dos_limites(
            finishing_raw in 1i32..1_000_000i32,
            keeper_raw in 1i32..1_000_000i32,
        ) {
            let finishing = FinishingQuality::new(Fixed::from_milli(finishing_raw));
            let keeper = GoalkeepingQuality::new(Fixed::from_milli(keeper_raw));
            let value = conversion_permille(finishing, keeper);
            prop_assert!(i64::from(value) >= MIN_CONVERSION_PERMILLE);
            prop_assert!(i64::from(value) <= MAX_CONVERSION_PERMILLE);
        }

        /// `TeamStrength::new`/`FinishingQuality::new`/`GoalkeepingQuality::new`
        /// nunca deixam passar um valor não-positivo — para qualquer `raw`,
        /// inclusive negativo, o piso segura.
        #[test]
        fn pisos_de_qualidade_nunca_sao_furados(raw: i32) {
            let strength = TeamStrength::new(Fixed::from_milli(raw));
            let finishing = FinishingQuality::new(Fixed::from_milli(raw));
            let goalkeeping = GoalkeepingQuality::new(Fixed::from_milli(raw));
            prop_assert!(strength.value() >= MIN_STRENGTH);
            prop_assert!(finishing.value() >= MIN_QUALITY);
            prop_assert!(goalkeeping.value() >= MIN_QUALITY);
        }
    }
}
