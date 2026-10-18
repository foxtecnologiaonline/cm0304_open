//! Adaptação — oitava e última fatia real de RF-JG-02 (atributos ocultos,
//! `docs/01 §2.3`): o quanto um jogador rende por baixo logo depois de uma
//! transferência, antes de "se acostumar" com o clube novo. Mora aqui, não
//! em `crate::market` (que já cuida de lealdade/ambição na decisão de
//! transferir) nem em `crate::progression` (que já cuida de CA/idade):
//! este módulo não decide **se** alguém transfere, só **quanto pior** um
//! recém-chegado rende em campo por um tempo — efeito que só o loop por
//! rodada de `crate::season` (seu único consumidor de verdade) consegue
//! expressar.
//!
//! [`generate_adaptation`] sorteia uma velocidade de adaptação sintética
//! (0-100) por jogador, uma vez por carreira — mais alta, adapta mais
//! rápido. Ao contrário da sujeira/jogo importante/consistência (que
//! pesam um sorteio ou balançam ao redor de 1x), isto é um **malus puro**:
//! [`apply_adaptation_penalty`] nunca aumenta a força, só reduz, e só para
//! quem foi transferido recentemente. Precisa de um estado de carreira à
//! parte — "há quantas temporadas este jogador está no clube atual" — que
//! nenhum atributo oculto anterior precisou (todos eram só "sorteado uma
//! vez, nunca muda"): [`initial_seasons_since_transfer`] começa todo
//! mundo "já assentado" (fora da janela de penalidade, já que o roster
//! inicial nunca foi "transferido" no sentido do jogo), [`mark_transferred`]
//! zera o contador de quem `crate::market::run_market_day` acabou de
//! mover, e [`advance_seasons_since_transfer`] incrementa todo mundo em 1
//! no fim da temporada — a mesma assimetria de "reset no evento, avança no
//! tick" de `crate::contract::advance_contracts`.
//!
//! Só existe no caminho de carreira (não há como "transferir" no caminho
//! estático de `managerfc-cli calibrate`/`bench`, que nunca chama
//! `crate::market::run_market_day`); copa (`crate::cup`) também não ganha
//! esse malus, mesma exclusão de sujeira/jogo importante/consistência (sem
//! granularidade de rodada lá).

use domain::{DeterministicRng, PlayerId};
use engine::TeamMatchProfile;
use pack::LoadedPack;

const MIN_SYNTHETIC_ADAPTATION: u8 = 0;
const MAX_SYNTHETIC_ADAPTATION: u8 = 100;

/// Gera a velocidade de adaptação de cada jogador do pack, na mesma ordem
/// (e portanto no mesmo índice denso) que
/// `crate::progression::initial_roster`. Chamada uma vez por carreira
/// (`app::GameSession::new`): sem atributos ocultos de verdade ainda
/// (RF-JG-02 completo — esta é a última fatia), a adaptação de um jogador
/// nunca muda depois de gerada.
#[must_use]
pub fn generate_adaptation(pack: &LoadedPack, world_seed: u64) -> Vec<u8> {
    pack.players
        .iter()
        .map(|p| synthetic_adaptation(p.id, world_seed))
        .collect()
}

fn synthetic_adaptation(player: PlayerId, world_seed: u64) -> u8 {
    let mut rng =
        DeterministicRng::seeded(world_seed, "world.adaptation", u64::from(player.index()), 0);
    let span = u32::from(MAX_SYNTHETIC_ADAPTATION - MIN_SYNTHETIC_ADAPTATION) + 1;
    MIN_SYNTHETIC_ADAPTATION + u8::try_from(rng.below(span)).expect("span cabe em u8 (0..=100)")
}

/// Quantas temporadas (contando a temporada da própria transferência como
/// `0`) um jogador ainda pode sofrer o malus de adaptação — a partir daqui
/// ([`ADAPTATION_WINDOW`] ou mais), [`apply_adaptation_penalty`] nunca
/// reduz nada. Não calibrado, só a direção certa: dura pouco (duas
/// temporadas) porque não há treino nem integração tática de verdade ainda
/// pra justificar um período mais longo.
const ADAPTATION_WINDOW: u8 = 2;

/// Estado inicial de "temporadas desde a última transferência" pro roster
/// inteiro de uma carreira nova — todo mundo começa **fora** da janela de
/// penalidade ([`ADAPTATION_WINDOW`]), porque o roster inicial do pack
/// nunca foi "transferido" no sentido de [`mark_transferred`] (chegou no
/// clube antes da carreira começar, não durante ela).
#[must_use]
pub fn initial_seasons_since_transfer(pack: &LoadedPack) -> Vec<u8> {
    vec![ADAPTATION_WINDOW; pack.players.len()]
}

/// Zera o contador de quem acabou de ser transferido
/// (`crate::market::run_market_day`) — a partir de agora esse jogador
/// entra na janela de penalidade de [`apply_adaptation_penalty`] de novo,
/// mesmo que já tivesse se adaptado a um clube anterior.
pub fn mark_transferred(seasons_since_transfer: &mut [u8], transferred: &[PlayerId]) {
    for &player in transferred {
        seasons_since_transfer[player.as_usize()] = 0;
    }
}

/// Avança "temporadas desde a última transferência" em uma unidade pro
/// roster inteiro — chamado uma vez no fim de cada temporada
/// (`app::GameSession::dispatch`), depois que o loop por rodada já leu o
/// valor desta temporada (mesma ordem de "reset no evento, avança no
/// tick" de `crate::contract::advance_contracts`). `saturating_add`
/// porque nada depende do valor exato uma vez fora da janela — só de
/// "é `>= ADAPTATION_WINDOW`" ([`apply_adaptation_penalty`]).
pub fn advance_seasons_since_transfer(seasons_since_transfer: &mut [u8]) {
    for value in seasons_since_transfer.iter_mut() {
        *value = value.saturating_add(1);
    }
}

/// Malus de força de um titular recém-chegado, em permilagem — `0` (fora
/// da janela, ou adaptação máxima) a [`MAX_ADAPTATION_PENALTY_PERMILLE`]
/// (acabou de chegar — `seasons_since_transfer == 0` — **e** adaptação
/// sintética mínima). Decai linearmente com as duas variáveis: mais
/// adaptado ou mais tempo no clube, menor o malus.
const MAX_ADAPTATION_PENALTY_PERMILLE: u32 = 150;

fn player_penalty_permille(adaptation: u8, seasons_since_transfer: u8) -> u32 {
    if seasons_since_transfer >= ADAPTATION_WINDOW {
        return 0;
    }
    let severity = u32::from(MAX_SYNTHETIC_ADAPTATION - adaptation); // 0..=100
    let remaining = u32::from(ADAPTATION_WINDOW - seasons_since_transfer); // 1..=ADAPTATION_WINDOW
    MAX_ADAPTATION_PENALTY_PERMILLE * severity * remaining
        / (u32::from(MAX_SYNTHETIC_ADAPTATION) * u32::from(ADAPTATION_WINDOW))
}

/// Aplica o malus de adaptação à força de `profile` — a média do malus
/// individual ([`player_penalty_permille`]) de cada titular em `starters`,
/// não só de quem está na janela (titular já assentado conta `0`, dilui a
/// média). `adaptation` e `seasons_since_transfer` têm que estar no mesmo
/// índice denso que o roster inteiro (não só `starters`). Time sem titular
/// elegível (`starters` vazio) devolve `profile` sem mudança — nada pra
/// tirar média.
#[must_use]
pub fn apply_adaptation_penalty(
    profile: TeamMatchProfile,
    starters: &[PlayerId],
    adaptation: &[u8],
    seasons_since_transfer: &[u8],
) -> TeamMatchProfile {
    if starters.is_empty() {
        return profile;
    }
    let total: u32 = starters
        .iter()
        .map(|&p| {
            player_penalty_permille(
                adaptation[p.as_usize()],
                seasons_since_transfer[p.as_usize()],
            )
        })
        .sum();
    let average_penalty = total / starters.len() as u32;
    if average_penalty == 0 {
        return profile;
    }
    let multiplier = domain::Fixed::from_ratio(i64::from(1000 - average_penalty), 1000);
    TeamMatchProfile {
        strength: engine::TeamStrength::new(profile.strength.value() * multiplier),
        ..profile
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{Ability, ClubId, CompetitionId, GameDate, NationId, Position};
    use engine::{FinishingQuality, GoalkeepingQuality, TeamStrength};
    use pack::{ResolvedClub, ResolvedNation, ResolvedPlayer};

    fn example_pack() -> LoadedPack {
        let nation = ResolvedNation {
            id: NationId::new(0),
            external_id: "ex".to_string(),
            name: "Ex".to_string(),
        };
        let club = ResolvedClub {
            id: ClubId::new(0),
            external_id: "ex.a".to_string(),
            name: "A".to_string(),
            nation: nation.id,
            competition: CompetitionId::new(0),
            founded: None,
            stadium: None,
        };
        let players = (0..10)
            .map(|i| ResolvedPlayer {
                id: PlayerId::new(i),
                external_id: format!("ex.a.p{i:02}"),
                first_name: "N".to_string(),
                last_name: "S".to_string(),
                birth: GameDate::from_ymd(2000, 1, 1),
                nation: nation.id,
                club: club.id,
                position: Position::Midfielder,
                attributes: domain::PlayerAttributes::default(),
                ability: Ability::new(100, 150),
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

    fn profile(strength: i32) -> TeamMatchProfile {
        TeamMatchProfile {
            strength: TeamStrength::new(domain::Fixed::from_int(strength)),
            finishing: FinishingQuality::new(domain::Fixed::from_int(10)),
            goalkeeping: GoalkeepingQuality::new(domain::Fixed::from_int(10)),
        }
    }

    #[test]
    fn generate_adaptation_e_deterministica_para_a_mesma_seed() {
        let p = example_pack();
        assert_eq!(generate_adaptation(&p, 7), generate_adaptation(&p, 7));
    }

    #[test]
    fn roster_novo_comeca_fora_da_janela_de_penalidade() {
        let p = example_pack();
        let seasons = initial_seasons_since_transfer(&p);
        assert!(seasons.iter().all(|&s| s >= ADAPTATION_WINDOW));
    }

    #[test]
    fn mark_transferred_zera_so_quem_foi_transferido() {
        let mut seasons = vec![ADAPTATION_WINDOW; 3];
        mark_transferred(&mut seasons, &[PlayerId::new(1)]);
        assert_eq!(seasons, vec![ADAPTATION_WINDOW, 0, ADAPTATION_WINDOW]);
    }

    #[test]
    fn advance_seasons_since_transfer_incrementa_todo_mundo() {
        let mut seasons = vec![0u8, 1, ADAPTATION_WINDOW];
        advance_seasons_since_transfer(&mut seasons);
        assert_eq!(seasons, vec![1, 2, ADAPTATION_WINDOW + 1]);
    }

    #[test]
    fn fora_da_janela_nunca_tem_malus() {
        let starters = vec![PlayerId::new(0)];
        let adaptation = vec![0u8]; // adaptação mínima, mas...
        let seasons_since_transfer = vec![ADAPTATION_WINDOW]; // ...já fora da janela
        let base = profile(300);
        let result =
            apply_adaptation_penalty(base, &starters, &adaptation, &seasons_since_transfer);
        assert_eq!(result.strength.value(), base.strength.value());
    }

    #[test]
    fn recem_chegado_com_adaptacao_minima_sofre_o_malus_maximo() {
        let starters = vec![PlayerId::new(0)];
        let adaptation = vec![0u8];
        let seasons_since_transfer = vec![0u8];
        let base = profile(300);
        let result =
            apply_adaptation_penalty(base, &starters, &adaptation, &seasons_since_transfer);
        // 300 × (1 - 0,150) = 255.
        assert_eq!(result.strength.value(), domain::Fixed::from_int(255));
    }

    #[test]
    fn adaptacao_maxima_nunca_tem_malus_mesmo_recem_chegado() {
        let starters = vec![PlayerId::new(0)];
        let adaptation = vec![100u8];
        let seasons_since_transfer = vec![0u8];
        let base = profile(300);
        let result =
            apply_adaptation_penalty(base, &starters, &adaptation, &seasons_since_transfer);
        assert_eq!(result.strength.value(), base.strength.value());
    }

    #[test]
    fn malus_diminui_conforme_passam_as_temporadas() {
        let starters = vec![PlayerId::new(0)];
        let adaptation = vec![0u8];
        let base = profile(300);

        let fresh = apply_adaptation_penalty(base, &starters, &adaptation, &[0u8]);
        let one_season_in = apply_adaptation_penalty(base, &starters, &adaptation, &[1u8]);
        let settled = apply_adaptation_penalty(base, &starters, &adaptation, &[ADAPTATION_WINDOW]);

        assert!(fresh.strength.value() < one_season_in.strength.value());
        assert!(one_season_in.strength.value() < settled.strength.value());
        assert_eq!(settled.strength.value(), base.strength.value());
    }

    #[test]
    fn titular_ja_assentado_dilui_a_media_do_time() {
        // Dois titulares: um recém-chegado com adaptação mínima (malus
        // máximo isolado), outro já assentado (malus 0) — a média do time
        // tem que ficar entre os dois extremos, não no malus máximo puro.
        let starters = vec![PlayerId::new(0), PlayerId::new(1)];
        let adaptation = vec![0u8, 0u8];
        let seasons_since_transfer = vec![0u8, ADAPTATION_WINDOW];
        let base = profile(300);

        let isolated = apply_adaptation_penalty(base, &starters[..1], &adaptation, &[0u8]);
        let diluted =
            apply_adaptation_penalty(base, &starters, &adaptation, &seasons_since_transfer);

        assert!(diluted.strength.value() > isolated.strength.value());
        assert!(diluted.strength.value() < base.strength.value());
    }
}
