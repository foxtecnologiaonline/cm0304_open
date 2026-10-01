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
/// Mesmo valor e mesmo papel do v0 — ver `docs/04 §4.1`.
const HOME_ADVANTAGE: Fixed = Fixed::from_ratio(14, 10);

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
    let (lambda_home, lambda_away) = expected_shots(home.strength, away.strength);
    let per_minute_home_shot = per_minute_permille(lambda_home);
    let per_minute_away_shot = per_minute_permille(lambda_away);
    let home_conversion = conversion_permille(home.finishing, away.goalkeeping);
    let away_conversion = conversion_permille(away.finishing, home.goalkeeping);

    let mut rng = DeterministicRng::seeded(ctx.world_seed, "engine.v0.match", ctx.fixture, 0);
    let mut events = Vec::with_capacity(8); // a maioria das partidas tem poucas finalizações convertidas
    events.push(MatchEvent::KickOff);

    let mut home_goals = 0u32;
    let mut away_goals = 0u32;
    for minute in 1..=MINUTES {
        if rng.chance_per_mille(per_minute_home_shot) {
            events.push(MatchEvent::Shot {
                minute,
                side: Side::Home,
            });
            if rng.chance_per_mille(home_conversion) {
                home_goals += 1;
                events.push(MatchEvent::Goal {
                    minute,
                    side: Side::Home,
                });
            }
        }
        if rng.chance_per_mille(per_minute_away_shot) {
            events.push(MatchEvent::Shot {
                minute,
                side: Side::Away,
            });
            if rng.chance_per_mille(away_conversion) {
                away_goals += 1;
                events.push(MatchEvent::Goal {
                    minute,
                    side: Side::Away,
                });
            }
        }
    }

    events.push(MatchEvent::FullTime {
        home_goals,
        away_goals,
    });
    events
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
