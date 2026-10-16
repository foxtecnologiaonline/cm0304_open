//! Progressão de jogador por idade, minutos jogados e profissionalismo —
//! `docs/03-modelo-de-dados.md §5.1`.
//!
//! Simplificação deliberada da fórmula completa (`docs/03 §5.1`:
//! `ΔCA_mês = f(idade, PA-CA, treino, minutos_jogados, profissionalismo,
//! ambição, qualidade_do_clube, moral)`): a maioria dessas variáveis ainda
//! não existe (sem treino, sem moral, sem qualidade de clube), e não há
//! calendário mensal ligado a `world` (`docs/02 §5` nota isso em vários
//! lugares). O que existe aqui são as três fatias calculáveis hoje:
//! **idade** (ajuste por temporada, sinal e magnitude puxados da tabela de
//! `docs/03 §5.1` — ganho alto aos 15-18, platô aos 24-28, declínio
//! acelerado aos 33+), **minutos jogados**, na única granularidade que
//! `world` tem pra isso — titular da temporada
//! (`crate::quality::starters_from_roster_all_clubs`, já calculado todo
//! `AdvanceSeason` pra condição física, `crate::condition`) ganha um
//! pequeno bônus de evolução sobre a curva de idade; reserva mantém a
//! curva pura — e **profissionalismo**, a primeira fatia de RF-JG-02
//! (atributos ocultos, `docs/01 §2.3`) a entrar aqui: sintético (0-100,
//! `generate_professionalism`, sorteado uma vez por carreira), soma um
//! pequeno bônus determinístico (0 a +2) proporcional a ele mesmo sobre
//! qualquer resultado da curva de idade — jogador mais profissional
//! evolui um pouco melhor e declina um pouco mais devagar, titular ou
//! reserva. `Ability::adjust_current` já garante `0 <= current <=
//! potential` (`docs/03 §5`), então o ajuste nunca precisa se preocupar
//! com estourar os limites por conta própria.
//!
//! O estado aqui é **separado** do pack (`pack::ResolvedPlayer` nunca
//! muda): [`PlayerState`] é o "CA de carreira" de uma sessão específica,
//! exatamente a distinção que `docs/03 §8` faz entre conteúdo do pack e
//! estado do save — hoje mora só em memória (`app::GameSession`), porque
//! `persist` ainda reconstrói uma sessão por replay em vez de guardar
//! estado por entidade (ver `persist::save`).

use std::collections::HashSet;

use domain::{Ability, ClubId, DeterministicRng, PlayerId, Position};
use pack::LoadedPack;

/// CA, idade e clube de um jogador dentro de uma carreira específica —
/// evolui a cada [`advance_season`] (CA/idade) ou transferência
/// (`crate::market`, `club`); o pack nunca muda. `position` é copiada do
/// pack e nunca muda por conta própria (não há treino de reconversão de
/// posição ainda) — mora aqui, não só em `pack::ResolvedPlayer`, pra que
/// `crate::strength`/`crate::market` não precisem voltar no pack só pra
/// saber onde alguém joga.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerState {
    pub player: PlayerId,
    /// Clube atual — começa como `pack::ResolvedPlayer::club`, muda com
    /// transferências (`crate::market::run_market_day`). É por isso que a
    /// força de um clube (`crate::strength::strength_from_roster`) agrupa
    /// jogadores por `roster[i].club`, nunca mais por `pack.players_of`.
    pub club: ClubId,
    pub position: Position,
    /// Idade em anos completos, calculada uma vez a partir de
    /// `LoadedPack::reference_year` e incrementada em 1 a cada temporada
    /// (aproximação de "uma temporada = um ano" — não há calendário mensal
    /// ligado a `world` ainda, ver o doc do módulo).
    pub age_years: i32,
    pub ability: Ability,
    /// `true` se o jogador está machucado nesta temporada
    /// (`crate::injuries::roll_injuries`) — uma fatia mínima de RF-JG-08
    /// (`docs/01 §2.3`, lesão completa com tipo/gravidade/recuperação é
    /// M3): aqui é só sim/não por temporada inteira, sem granularidade de
    /// dias (não há calendário mensal, ver o doc do módulo). Jogador
    /// machucado é excluído da escalação
    /// (`crate::strength::strength_from_roster`,
    /// `crate::quality::profile_from_roster`).
    pub injured: bool,
    /// Condição física de 0 a 100 — fatia mínima de RF-JG-07 (`docs/01
    /// §2.3`), recalculada a cada temporada por `crate::condition` a
    /// partir de quantas partidas de liga o jogador titularizou na
    /// temporada anterior (não é cumulativo entre temporadas: reseta a
    /// cada uma, não acumula desgaste ao longo da carreira — ver o doc de
    /// `crate::condition`). Começa em `crate::condition::FULL_CONDITION`
    /// (100, descansado). Entra como multiplicador na força efetiva de
    /// `crate::quality::build_profile`.
    pub condition: u8,
}

/// Constrói o estado inicial de carreira a partir do pack: um
/// [`PlayerState`] por `pack.players`, na mesma ordem (e portanto no mesmo
/// índice denso — `PlayerId::as_usize()` — que `pack.players`), com a
/// idade derivada de `birth` e `pack.reference_year`
/// (`pack::resolve` já garante `birth.year() < reference_year` na
/// validação, então `age_years` aqui é sempre positivo).
#[must_use]
pub fn initial_roster(pack: &LoadedPack) -> Vec<PlayerState> {
    pack.players
        .iter()
        .map(|p| PlayerState {
            player: p.id,
            club: p.club,
            position: p.position,
            age_years: pack.reference_year - p.birth.year(),
            ability: p.ability,
            injured: false,
            condition: crate::condition::FULL_CONDITION,
        })
        .collect()
}

/// Aplica uma temporada de envelhecimento + progressão a cada jogador do
/// `roster`, em ordem de `PlayerId` (determinístico, `ADR 0002`). Cada
/// jogador recebe um `Rng` próprio (`docs/02 §5`: "um `Rng` por subsistema
/// e por entidade-raiz") semeado por `(world_seed, "world.progression",
/// player_id, season_index)` — evoluir o jogador A não pode mudar o
/// resultado do jogador B, e a mesma temporada não pode repetir o ajuste
/// da anterior. `starters` é a escalação titular da temporada que acabou
/// de terminar (`crate::quality::starters_from_roster_all_clubs` — o mesmo
/// conjunto que `crate::condition::apply_season_fatigue` já usa): quem
/// titularizou ganha um pequeno bônus de evolução (fatia mínima de
/// "minutos jogados", RF-JG-05) sobre a curva de idade; reserva não.
/// `professionalism` tem que estar no mesmo índice denso que `roster`
/// (`generate_professionalism`, mesmo invariante de `crate::contract`) —
/// soma um bônus determinístico por cima, titular ou reserva.
pub fn advance_season(
    roster: &mut [PlayerState],
    starters: &HashSet<PlayerId>,
    professionalism: &[u8],
    world_seed: u64,
    season_index: u32,
) {
    for (state, &player_professionalism) in roster.iter_mut().zip(professionalism) {
        state.age_years += 1;
        let mut rng = DeterministicRng::seeded(
            world_seed,
            "world.progression",
            u64::from(state.player.index()),
            u64::from(season_index),
        );
        let is_starter = starters.contains(&state.player);
        let delta = delta_ca_for_age(state.age_years, is_starter, &mut rng)
            .saturating_add(professionalism_bonus(player_professionalism));
        state.ability = state.ability.adjust_current(delta);
    }
}

/// Piso e teto da propensão a profissionalismo sintética, em escala 0-100
/// — mesmo layout de `crate::reputation`/`crate::injuries`.
const MIN_SYNTHETIC_PROFESSIONALISM: u8 = 0;
const MAX_SYNTHETIC_PROFESSIONALISM: u8 = 100;

/// Gera o profissionalismo de cada jogador do pack, na mesma ordem (e
/// portanto no mesmo índice denso) que [`initial_roster`] — mesmo
/// invariante de indexação de `crate::contract::ContractState`: este
/// `Vec` tem que ficar sempre alinhado ao roster que o acompanha. Chamada
/// uma vez por carreira (`app::GameSession::new`): sem atributos ocultos
/// de verdade ainda (RF-JG-02 completo), o profissionalismo de um jogador
/// nunca muda depois de gerado.
#[must_use]
pub fn generate_professionalism(pack: &LoadedPack, world_seed: u64) -> Vec<u8> {
    pack.players
        .iter()
        .map(|p| synthetic_professionalism(p.id, world_seed))
        .collect()
}

fn synthetic_professionalism(player: PlayerId, world_seed: u64) -> u8 {
    let mut rng = DeterministicRng::seeded(
        world_seed,
        "world.professionalism",
        u64::from(player.index()),
        0,
    );
    let span = u32::from(MAX_SYNTHETIC_PROFESSIONALISM - MIN_SYNTHETIC_PROFESSIONALISM) + 1;
    MIN_SYNTHETIC_PROFESSIONALISM
        + u8::try_from(rng.below(span)).expect("span cabe em u8 (0..=100)")
}

/// Teto do bônus de evolução por profissionalismo — pequeno de propósito
/// (no máximo +2 num jogador que já pode variar de -15 a +12 por idade, e
/// +0 a +3 por titularidade): mais um tempero, não uma terceira fonte de
/// progressão do mesmo tamanho. Determinístico (função pura do
/// profissionalismo, sem `Rng`) — ao contrário do bônus de titular, não
/// precisa de sorteio pra variar: o profissionalismo em si já varia por
/// jogador.
const PROFESSIONALISM_BONUS_MAX: i16 = 2;

fn professionalism_bonus(professionalism: u8) -> i16 {
    i16::from(professionalism) * PROFESSIONALISM_BONUS_MAX / 100
}

/// Sorteia o ajuste de CA da temporada para um jogador de `age` anos —
/// faixas de `docs/03 §5.1`, convertidas de "por mês" para "por temporada"
/// (não há granularidade mensal ainda). O piso de cada faixa é sempre menor
/// que o teto, com sobreposição deliberada entre faixas vizinhas (ex.:
/// um jogador de 23 e um de 24 têm distribuições parecidas) — é uma curva
/// suave, não um degrau brusco no aniversário. `is_starter` soma o bônus de
/// [`starter_bonus`] por cima — nunca negativo, então titularizar nunca
/// piora a evolução de ninguém, só deixa de dar o extra pra quem fica no
/// banco.
fn delta_ca_for_age(age: i32, is_starter: bool, rng: &mut DeterministicRng) -> i16 {
    let (min, max): (i16, i16) = match age {
        i32::MIN..=18 => (0, 12), // 15-18: ganho alto, muito sensível (aqui: generoso)
        19..=23 => (-2, 8),       // 19-23: ganho moderado
        24..=28 => (-3, 4),       // 24-28: platô, ganho marginal
        29..=32 => (-8, 1),       // 29-32: declínio físico, mental compensa parcialmente
        _ => (-15, -2),           // 33+: declínio acelerado
    };
    let span = u32::try_from(i32::from(max) - i32::from(min) + 1)
        .expect("max >= min em todas as faixas de delta_ca_for_age");
    let base = min
        + i16::try_from(rng.below(span)).expect("span cabe em i16 para todas as faixas definidas");
    if is_starter {
        base.saturating_add(starter_bonus(rng))
    } else {
        base
    }
}

/// Piso e teto do bônus de evolução de quem titularizou a temporada —
/// pequeno de propósito (no máximo +3 num jogador que já pode variar de
/// -15 a +12 por idade): é um tempero na curva de idade, não uma segunda
/// fonte de progressão do mesmo tamanho. Sempre `>= 0`.
const STARTER_BONUS_MIN: i16 = 0;
const STARTER_BONUS_MAX: i16 = 3;

fn starter_bonus(rng: &mut DeterministicRng) -> i16 {
    let span = u32::try_from(STARTER_BONUS_MAX - STARTER_BONUS_MIN + 1)
        .expect("faixa do bônus de titular é pequena e fixa");
    STARTER_BONUS_MIN + i16::try_from(rng.below(span)).expect("span cabe em i16")
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{ClubId, CompetitionId, GameDate, NationId, Position};
    use pack::{ResolvedClub, ResolvedNation, ResolvedPlayer};

    fn pack_with_players(specs: &[(u8, i32)]) -> LoadedPack {
        // specs: (ability_current, birth_year); reference_year fixo em 2026.
        let nation = ResolvedNation {
            id: NationId::new(0),
            external_id: "ex".to_string(),
            name: "Exemplolândia".to_string(),
        };
        let club = ResolvedClub {
            id: ClubId::new(0),
            external_id: "ex.a".to_string(),
            name: "Clube A".to_string(),
            nation: nation.id,
            competition: CompetitionId::new(0),
            founded: None,
            stadium: None,
        };
        let players = specs
            .iter()
            .enumerate()
            .map(|(i, &(ca, birth_year))| ResolvedPlayer {
                id: PlayerId::new(i as u32),
                external_id: format!("ex.a.p{i:02}"),
                first_name: "Nome".to_string(),
                last_name: "Sobrenome".to_string(),
                birth: GameDate::from_ymd(birth_year, 6, 15),
                nation: nation.id,
                club: club.id,
                position: Position::Midfielder,
                attributes: domain::PlayerAttributes::default(),
                ability: Ability::new(ca, 200),
            })
            .collect();
        LoadedPack {
            reference_year: 2026,
            nations: vec![nation],
            competitions: vec![],
            clubs: vec![club],
            players,
        }
    }

    #[test]
    fn initial_roster_calcula_idade_a_partir_do_ano_de_referencia() {
        let pack = pack_with_players(&[(100, 2000), (100, 2008)]);
        let roster = initial_roster(&pack);
        assert_eq!(roster[0].age_years, 26);
        assert_eq!(roster[1].age_years, 18);
    }

    /// Profissionalismo neutro (0, sem bônus) — pros testes que não são
    /// sobre o efeito do profissionalismo em si.
    fn no_professionalism(n: usize) -> Vec<u8> {
        vec![0; n]
    }

    #[test]
    fn advance_season_incrementa_a_idade_de_todos_em_um_ano() {
        let pack = pack_with_players(&[(100, 2000), (100, 2008)]);
        let mut roster = initial_roster(&pack);
        advance_season(&mut roster, &HashSet::new(), &no_professionalism(2), 42, 0);
        assert_eq!(roster[0].age_years, 27);
        assert_eq!(roster[1].age_years, 19);
    }

    #[test]
    fn advance_season_e_deterministico_para_a_mesma_seed() {
        let pack = pack_with_players(&[(100, 2000), (80, 1990), (150, 2008)]);
        let mut a = initial_roster(&pack);
        let mut b = initial_roster(&pack);
        let starters = HashSet::from([PlayerId::new(0)]);
        let professionalism = no_professionalism(3);
        advance_season(&mut a, &starters, &professionalism, 7, 0);
        advance_season(&mut b, &starters, &professionalism, 7, 0);
        assert_eq!(a, b);
    }

    #[test]
    fn evoluir_um_jogador_nao_muda_o_resultado_de_outro() {
        // Dois elencos idênticos exceto o segundo jogador — o primeiro
        // jogador tem que evoluir exatamente igual nos dois casos, porque
        // cada jogador tem seu próprio `Rng` (docs/02 §5).
        let pack_a = pack_with_players(&[(100, 2000), (100, 2000)]);
        let pack_b = pack_with_players(&[(100, 2000), (50, 1990)]);
        let mut roster_a = initial_roster(&pack_a);
        let mut roster_b = initial_roster(&pack_b);
        // Jogador 0 titular nos dois casos — mantém o mesmo tratamento pra
        // isolar o que o teste quer provar (independência entre jogadores).
        let starters = HashSet::from([PlayerId::new(0)]);
        let professionalism = no_professionalism(2);
        advance_season(&mut roster_a, &starters, &professionalism, 42, 0);
        advance_season(&mut roster_b, &starters, &professionalism, 42, 0);
        assert_eq!(roster_a[0].ability, roster_b[0].ability);
    }

    #[test]
    fn jogadores_jovens_tendem_a_subir_e_veteranos_a_cair_em_media() {
        // Sanidade estatística grosseira da curva de docs/03 §5.1: um
        // jogador de 17 anos simulado por muitas "temporadas paralelas"
        // independentes (seeds diferentes) deveria, em média, ganhar CA; um
        // de 35 deveria, em média, perder. Não testa a curva exata — só que
        // a direção do efeito está certa, o que já pegaria um sinal
        // trocado por engano em `delta_ca_for_age`. Reserva sem
        // profissionalismo (sem bônus de titular nem de profissionalismo)
        // pra testar a curva de idade isolada, sem nenhum tempero por cima.
        let trials = 500u64;

        let young_total: i64 = (0..trials)
            .map(|seed| {
                let pack = pack_with_players(&[(100, 2009)]); // 17 anos
                let mut roster = initial_roster(&pack);
                advance_season(
                    &mut roster,
                    &HashSet::new(),
                    &no_professionalism(1),
                    seed,
                    0,
                );
                i64::from(roster[0].ability.current()) - 100
            })
            .sum();
        assert!(
            young_total > 0,
            "jogador jovem deveria ganhar CA em média (soma dos deltas: {young_total})"
        );

        let old_total: i64 = (0..trials)
            .map(|seed| {
                let pack = pack_with_players(&[(100, 1991)]); // 35 anos
                let mut roster = initial_roster(&pack);
                advance_season(
                    &mut roster,
                    &HashSet::new(),
                    &no_professionalism(1),
                    seed,
                    0,
                );
                i64::from(roster[0].ability.current()) - 100
            })
            .sum();
        assert!(
            old_total < 0,
            "jogador veterano deveria perder CA em média (soma dos deltas: {old_total})"
        );
    }

    #[test]
    fn titular_tende_a_evoluir_mais_que_reserva_em_media() {
        // Sanidade estatística do bônus de `starter_bonus`: o mesmo
        // jogador (mesma idade, mesmas seeds), só mudando titular/reserva,
        // deveria terminar em média com CA mais alto como titular — nunca
        // mais baixo, já que o bônus nunca é negativo.
        let trials = 500u64;
        let starters = HashSet::from([PlayerId::new(0)]);

        let starter_total: i64 = (0..trials)
            .map(|seed| {
                let pack = pack_with_players(&[(100, 2000)]); // 26 anos, platô
                let mut roster = initial_roster(&pack);
                advance_season(&mut roster, &starters, &no_professionalism(1), seed, 0);
                i64::from(roster[0].ability.current())
            })
            .sum();
        let reserve_total: i64 = (0..trials)
            .map(|seed| {
                let pack = pack_with_players(&[(100, 2000)]);
                let mut roster = initial_roster(&pack);
                advance_season(
                    &mut roster,
                    &HashSet::new(),
                    &no_professionalism(1),
                    seed,
                    0,
                );
                i64::from(roster[0].ability.current())
            })
            .sum();
        assert!(
            starter_total > reserve_total,
            "titular deveria evoluir melhor em média que reserva (titular: {starter_total}, \
             reserva: {reserve_total})"
        );
    }

    #[test]
    fn profissional_tende_a_evoluir_mais_que_sem_profissionalismo_em_media() {
        // Mesma sanidade estatística de `titular_tende_a_evoluir_mais...`,
        // agora pro profissionalismo: o mesmo jogador (mesma idade, mesmas
        // seeds), só mudando profissionalismo de 0 a 100, deveria terminar
        // em média com CA mais alto no caso profissional — nunca mais
        // baixo, já que o bônus nunca é negativo.
        let trials = 500u64;
        let starters = HashSet::new();

        let professional_total: i64 = (0..trials)
            .map(|seed| {
                let pack = pack_with_players(&[(100, 2000)]); // 26 anos, platô
                let mut roster = initial_roster(&pack);
                advance_season(&mut roster, &starters, &[100], seed, 0);
                i64::from(roster[0].ability.current())
            })
            .sum();
        let unprofessional_total: i64 = (0..trials)
            .map(|seed| {
                let pack = pack_with_players(&[(100, 2000)]);
                let mut roster = initial_roster(&pack);
                advance_season(&mut roster, &starters, &[0], seed, 0);
                i64::from(roster[0].ability.current())
            })
            .sum();
        assert!(
            professional_total > unprofessional_total,
            "profissional deveria evoluir melhor em média (profissional: {professional_total}, \
             sem profissionalismo: {unprofessional_total})"
        );
    }

    #[test]
    fn ability_nunca_ultrapassa_o_potencial_mesmo_apos_muitas_temporadas() {
        let pack = pack_with_players(&[(100, 2009)]); // jovem, tende a subir
        let mut roster = initial_roster(&pack);
        let starters = HashSet::from([PlayerId::new(0)]); // titular toda temporada, pior caso pro teto
        let professionalism = [100u8]; // máximo, pior caso pro teto
        for season in 0..20 {
            advance_season(&mut roster, &starters, &professionalism, 1, season);
            assert!(roster[0].ability.current() <= roster[0].ability.potential());
        }
    }
}

/// Testes de propriedade (`docs/08 §1`).
#[cfg(test)]
mod proptests {
    use super::*;
    use domain::{ClubId, CompetitionId, GameDate, NationId, Position};
    use pack::{ResolvedClub, ResolvedNation, ResolvedPlayer};
    use proptest::prelude::*;

    fn single_player_pack(ca: u8, potential: u8, birth_year: i32) -> LoadedPack {
        let nation = ResolvedNation {
            id: NationId::new(0),
            external_id: "ex".to_string(),
            name: "Exemplolândia".to_string(),
        };
        let club = ResolvedClub {
            id: ClubId::new(0),
            external_id: "ex.a".to_string(),
            name: "Clube A".to_string(),
            nation: nation.id,
            competition: CompetitionId::new(0),
            founded: None,
            stadium: None,
        };
        let player = ResolvedPlayer {
            id: PlayerId::new(0),
            external_id: "ex.a.p00".to_string(),
            first_name: "Nome".to_string(),
            last_name: "Sobrenome".to_string(),
            birth: GameDate::from_ymd(birth_year, 6, 15),
            nation: nation.id,
            club: club.id,
            position: Position::Midfielder,
            attributes: domain::PlayerAttributes::default(),
            ability: Ability::new(ca, potential),
        };
        LoadedPack {
            reference_year: 2026,
            nations: vec![nation],
            competitions: vec![],
            clubs: vec![club],
            players: vec![player],
        }
    }

    proptest! {
        /// Para qualquer CA/PA/idade/seed válidos, `advance_season` nunca
        /// deixa `current` sair de `0..=potential` — generaliza
        /// `ability_nunca_ultrapassa_o_potencial_mesmo_apos_muitas_temporadas`
        /// para entradas arbitrárias em vez de um único exemplo fixo.
        #[test]
        fn ability_sempre_dentro_dos_limites_para_qualquer_entrada(
            ca in 0u8..=200,
            potential_extra in 0u8..=100,
            birth_year in 1970i32..2008,
            is_starter: bool,
            professionalism: u8,
            world_seed: u64,
            season_index: u32,
        ) {
            let potential = ca.saturating_add(potential_extra).min(domain::attributes::ABILITY_MAX);
            let pack = single_player_pack(ca.min(potential), potential, birth_year);
            let mut roster = initial_roster(&pack);
            let starters = if is_starter { HashSet::from([PlayerId::new(0)]) } else { HashSet::new() };
            advance_season(&mut roster, &starters, &[professionalism], world_seed, season_index);
            prop_assert!(roster[0].ability.current() <= roster[0].ability.potential());
        }

        /// `advance_season` é uma função pura do seu input — mesma seed,
        /// mesmo roster de entrada, sempre o mesmo resultado.
        #[test]
        fn advance_season_e_puro(
            ca in 0u8..=200,
            birth_year in 1970i32..2008,
            is_starter: bool,
            professionalism: u8,
            world_seed: u64,
            season_index: u32,
        ) {
            let pack = single_player_pack(ca, 200, birth_year);
            let mut a = initial_roster(&pack);
            let mut b = initial_roster(&pack);
            let starters = if is_starter { HashSet::from([PlayerId::new(0)]) } else { HashSet::new() };
            advance_season(&mut a, &starters, &[professionalism], world_seed, season_index);
            advance_season(&mut b, &starters, &[professionalism], world_seed, season_index);
            prop_assert_eq!(a, b);
        }
    }
}
