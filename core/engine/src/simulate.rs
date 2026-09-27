//! Motor v0 — "força relativa → placar" (`docs/04-motor-de-partida.md §7`,
//! primeira linha do plano evolutivo). Nenhuma zona, nenhuma posse, nenhum
//! jogador em campo — só o suficiente para o mundo (`world`, M1) ter um
//! resultado de partida determinístico para alimentar tabela, calendário e
//! IA de escalação/mercado. As camadas de posse e evento (`docs/04 §2.2`
//! e `§2.3`) chegam no v1/v2.
//!
//! ## O modelo
//!
//! O motor caminha os 90 minutos da partida (o mesmo "tick" de 1 minuto do
//! regime instantâneo, `docs/04 §3`) e, em cada minuto, sorteia
//! independentemente se cada time marca — um processo de Bernoulli por
//! minuto que aproxima um processo de Poisson por "thinning" (aproximação
//! padrão: `Binomial(90, λ/90) ≈ Poisson(λ)` para `λ` pequeno frente a 90).
//! A vantagem dessa abordagem sobre amostrar Poisson diretamente é que ela
//! reaproveita `DeterministicRng::chance_per_mille` — já testado — em vez de
//! precisar de uma segunda peça de infraestrutura (uma tabela de `exp(-λ)`)
//! só para o v0. E produz o próprio evento `Goal { minute, .. }` de graça,
//! em vez de ter que "inventar" um minuto depois de saber o placar.
//!
//! `λ_mandante`/`λ_visitante` (o número esperado de gols de cada lado na
//! partida inteira) vêm de duas constantes calibráveis e da força relativa
//! dos dois times:
//!
//! ```text
//! base = TOTAL_GOALS_TARGET / (1 + HOME_ADVANTAGE)
//! λ_home = base × HOME_ADVANTAGE × (força_home / média(força_home, força_away))
//! λ_away = base ×                  (força_away / média(força_home, força_away))
//! ```
//!
//! Quando as forças são iguais, `λ_home + λ_away = TOTAL_GOALS_TARGET` e
//! `λ_home / λ_away = HOME_ADVANTAGE` — o mandante marca mais na média, sem
//! mudar o total esperado de gols da partida. `TOTAL_GOALS_TARGET` e
//! `HOME_ADVANTAGE` são os alvos de `docs/04 §4.1` (2,7 gols/partida, ~44%
//! de vitória do mandante); os valores aqui são um ponto de partida
//! razoável, não uma calibração — isso é trabalho do `managerfc-cli
//! calibrate` (ainda não implementado, `docs/08 §4`) rodando muitas
//! temporadas e comparando com os alvos publicados. Quando esse comando
//! existir, os dois valores devem migrar para dado (`packs/core/engine.toml`,
//! RNF-23) em vez de ficar hard-coded aqui.

use domain::{DeterministicRng, Fixed};

use crate::event::{MatchEvent, Side};

/// Duração da partida no v0 — sem acréscimos (chegam com cartões/lesões).
const MINUTES: u8 = 90;

/// Piso de força — nunca zero, para a razão de força nunca dividir por zero
/// nem produzir um `λ` negativo.
const MIN_STRENGTH: Fixed = Fixed::from_ratio(1, 100);

/// Teto de força relativa aplicado ao `λ` final, não à força de entrada —
/// evita que um pack com forças absurdamente desiguais produza uma
/// probabilidade por minuto maior que 100% (`docs/03 §7`, validação de pack,
/// é a primeira linha de defesa; isto é a segunda, dentro do próprio motor).
const MAX_LAMBDA: Fixed = Fixed::from_int(8);

/// Total de gols esperado por partida quando as forças são iguais —
/// `docs/04 §4.1`: alvo de 2,7 gols/partida (±0,15 na calibração futura).
const TOTAL_GOALS_TARGET: Fixed = Fixed::from_ratio(27, 10);

/// Vantagem de mando: razão `λ_home / λ_away` quando as forças são iguais.
/// `docs/04 §4.1` mira ~44% de vitórias do mandante contra ~26% do
/// visitante (o resto empates); 1.4 é uma estimativa inicial de mercado
/// consistente com essa proporção, a ser ajustada por `calibrate`.
const HOME_ADVANTAGE: Fixed = Fixed::from_ratio(14, 10);

/// Força relativa de um time, para efeito **só** do motor v0.
///
/// É um número positivo em escala arbitrária — o motor compara duas forças
/// entre si, nunca contra um valor absoluto. Calcular esse número a partir
/// de um elenco de verdade (atributos, moral, condição — `docs/04 §2.1`) é
/// trabalho de `world`/`ai`, que ainda não existem; o motor não sabe, e não
/// precisa saber, de onde o número veio.
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
/// completo (`KickOff` → `Goal`* → `FullTime`).
///
/// Determinístico: o mesmo `(home, away, ctx)` produz sempre a mesma
/// sequência de eventos, em qualquer plataforma (`ADR 0002`) — é o que os
/// testes de propriedade deste módulo verificam.
#[must_use]
pub fn simulate(home: TeamStrength, away: TeamStrength, ctx: MatchContext) -> Vec<MatchEvent> {
    let (lambda_home, lambda_away) = expected_goals(home, away);
    let per_minute_home = per_minute_permille(lambda_home);
    let per_minute_away = per_minute_permille(lambda_away);

    let mut rng = DeterministicRng::seeded(ctx.world_seed, "engine.v0.match", ctx.fixture, 0);
    let mut events = Vec::with_capacity(4); // a maioria das partidas tem poucos gols
    events.push(MatchEvent::KickOff);

    let mut home_goals = 0u32;
    let mut away_goals = 0u32;
    for minute in 1..=MINUTES {
        if rng.chance_per_mille(per_minute_home) {
            home_goals += 1;
            events.push(MatchEvent::Goal {
                minute,
                side: Side::Home,
            });
        }
        if rng.chance_per_mille(per_minute_away) {
            away_goals += 1;
            events.push(MatchEvent::Goal {
                minute,
                side: Side::Away,
            });
        }
    }

    events.push(MatchEvent::FullTime {
        home_goals,
        away_goals,
    });
    events
}

/// Calcula `(λ_home, λ_away)` a partir das duas forças — ver a fórmula no
/// doc do módulo.
fn expected_goals(home: TeamStrength, away: TeamStrength) -> (Fixed, Fixed) {
    let base = TOTAL_GOALS_TARGET / (Fixed::ONE + HOME_ADVANTAGE);
    let average = (home.0 + away.0) / Fixed::from_int(2);
    let ratio_home = home.0 / average;
    let ratio_away = away.0 / average;

    let lambda_home = (base * HOME_ADVANTAGE * ratio_home).min(MAX_LAMBDA);
    let lambda_away = (base * ratio_away).min(MAX_LAMBDA);
    (lambda_home, lambda_away)
}

/// Converte `λ` (gols esperados na partida inteira) em probabilidade por
/// minuto, em milésimos — a escala que `DeterministicRng::chance_per_mille`
/// espera. Arredonda para o mais próximo (não trunca): `round(λ×1000/90)`.
fn per_minute_permille(lambda: Fixed) -> u32 {
    let milli = i64::from(lambda.raw_milli().max(0));
    let minutes = i64::from(MINUTES);
    (((milli + minutes / 2) / minutes) as u32).min(1000)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::event::score;

    fn ctx(fixture: u64) -> MatchContext {
        MatchContext {
            world_seed: 42,
            fixture,
        }
    }

    fn equal_strengths() -> (TeamStrength, TeamStrength) {
        let s = TeamStrength::new(Fixed::from_int(100));
        (s, s)
    }

    #[test]
    fn partida_comeca_com_kickoff_e_termina_com_fulltime_coerente() {
        let (home, away) = equal_strengths();
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
    fn mesmo_contexto_produz_a_mesma_partida_sempre() {
        let (home, away) = equal_strengths();
        let a = simulate(home, away, ctx(7));
        let b = simulate(home, away, ctx(7));
        assert_eq!(a, b);
    }

    #[test]
    fn fixtures_diferentes_produzem_partidas_independentes() {
        let (home, away) = equal_strengths();
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
        // a soma bate com o alvo de gols/partida, e a razão entre os dois
        // bate com HOME_ADVANTAGE.
        let (home, away) = equal_strengths();
        let (lambda_home, lambda_away) = expected_goals(home, away);

        let sum = lambda_home + lambda_away;
        let sum_diff = (sum - TOTAL_GOALS_TARGET).abs();
        assert!(
            sum_diff.raw_milli() <= 2,
            "soma dos λ ({sum}) deveria ficar perto do alvo ({TOTAL_GOALS_TARGET})"
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
    fn time_muito_mais_forte_vence_a_grande_maioria_das_simulacoes() {
        // A verificação mais barata de que "força relativa" de fato importa
        // no v0 — o embrião do que vira o torneio anti-exploit completo no
        // v2/v3 (docs/04 §4.3, docs/08 §5).
        let strong = TeamStrength::new(Fixed::from_int(500));
        let weak = TeamStrength::new(Fixed::from_int(20));

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
        // Sanidade estatística grosseira do v0 contra o alvo de docs/04 §4.1
        // (2,7 gols/partida). Não é a suíte de calibração de verdade
        // (docs/08 §4) — essa vem com `managerfc-cli calibrate` no M1/M2,
        // rodando muito mais temporadas e comparando várias métricas. Isto
        // aqui só evita que o v0 fique grosseiramente errado sem que
        // nenhum teste perceba.
        let (home, away) = equal_strengths();
        let trials = 2000u32;
        let total_goals: u32 = (0..trials)
            .map(|i| {
                let events = simulate(home, away, ctx(u64::from(i)));
                let (h, a) = score(&events);
                h + a
            })
            .sum();
        let average_milli = u64::from(total_goals) * 1000 / u64::from(trials);
        // Alvo 2700 (2,7); folga generosa (±400, ~15%) porque a aproximação
        // por thinning binomial(90, λ/90) e a amostra de 2000 partidas não
        // precisam bater a três casas — só não podem estar grosseiramente
        // fora. A calibração fina fica para o comando dedicado.
        assert!(
            (2300..=3100).contains(&average_milli),
            "média observada {average_milli}/1000 gols por partida, esperado ~2700 (±400)"
        );
    }
}

/// Testes de propriedade (`docs/08 §1`) — o mesmo papel que já pagou dividendos
/// em `domain` (dois bugs reais achados por generalizar exemplos fixos):
/// aqui generalizam o determinismo e os limites de `per_minute_permille`
/// para entradas arbitrárias, não só os poucos casos escolhidos à mão acima.
#[cfg(test)]
mod proptests {
    use super::*;
    use crate::event::score;
    use proptest::prelude::*;

    proptest! {
        /// `simulate` é uma função pura do seu input — mesma força, mesmo
        /// contexto, mesmos eventos, para qualquer combinação de forças e
        /// seeds, não só o par fixo usado em `mesmo_contexto_produz_a_mesma_partida_sempre`.
        #[test]
        fn simulate_e_determinista_para_qualquer_entrada(
            home_raw in 1_000i32..1_000_000i32,
            away_raw in 1_000i32..1_000_000i32,
            world_seed: u64,
            fixture: u64,
        ) {
            let home = TeamStrength::new(Fixed::from_milli(home_raw));
            let away = TeamStrength::new(Fixed::from_milli(away_raw));
            let ctx = MatchContext { world_seed, fixture };

            let a = simulate(home, away, ctx);
            let b = simulate(home, away, ctx);
            prop_assert_eq!(a, b);
        }

        /// A lista de eventos sempre começa com `KickOff`, sempre termina com
        /// `FullTime`, e o placar do `FullTime` sempre bate com `score()` —
        /// para qualquer força e qualquer seed, não só os casos de exemplo.
        #[test]
        fn eventos_tem_forma_valida_para_qualquer_entrada(
            home_raw in 1_000i32..1_000_000i32,
            away_raw in 1_000i32..1_000_000i32,
            world_seed: u64,
            fixture: u64,
        ) {
            let home = TeamStrength::new(Fixed::from_milli(home_raw));
            let away = TeamStrength::new(Fixed::from_milli(away_raw));
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
        /// bem maior que qualquer coisa que `expected_goals` produziria na
        /// prática — importa porque `chance_per_mille` trata `>= 1000` como
        /// "sempre", então um estouro silencioso viraria "gol garantido todo
        /// minuto" em vez de um erro visível.
        #[test]
        fn per_minute_permille_nunca_passa_de_mil(raw in 0i32..1_000_000i32) {
            let value = per_minute_permille(Fixed::from_milli(raw));
            prop_assert!(value <= 1000);
        }

        /// `TeamStrength::new` nunca deixa passar um valor não-positivo —
        /// para qualquer `raw`, inclusive negativo, o piso segura.
        #[test]
        fn team_strength_nunca_fica_abaixo_do_piso(raw: i32) {
            let strength = TeamStrength::new(Fixed::from_milli(raw));
            prop_assert!(strength.value() >= MIN_STRENGTH);
        }
    }
}
