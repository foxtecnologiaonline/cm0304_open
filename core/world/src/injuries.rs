//! Lesões simples por temporada — a fatia mínima de RF-JG-08
//! (`docs/01-requisitos.md §2.3`: lesão com tipo, gravidade, prazo de
//! recuperação e risco de recaída, marcada M3). Aqui não há nada disso: só
//! um sorteio por jogador por temporada, sim/não, sem duração interna à
//! temporada — "machucado" significa "fora da escalação a temporada
//! inteira" (mesma granularidade grosseira de `crate::progression`, que já
//! trata "uma temporada = um ano" como a menor unidade calculável sem
//! calendário mensal).
//!
//! Propositalmente **não** tenta bater com o alvo de `docs/04 §4.1`
//! ("lesões por 1.000 min", uma métrica por minuto de partida): essa
//! precisa de lesões geradas *durante* a simulação de uma partida
//! (motor v1/v2, `MatchEvent::Injury`), não por sorteio de pré-temporada.
//!
//! A chance em si varia por jogador conforme [`InjuryProneness`] —
//! primeira fatia real de RF-JG-02 (`docs/01 §2.3`: atributos ocultos,
//! "propensão a lesão" entre eles). Antes, todo jogador tinha a mesma
//! chance fixa (6%), decorativo em relação a quem é o jogador; agora um
//! jogador frágil se machuca mais que um resistente, em média. Sintética
//! (sorteada uma vez por carreira, igual orçamento/reputação/capacidade de
//! estádio) — o pack não declara propensão a lesão de verdade ainda.

use domain::{DeterministicRng, PlayerId};
use pack::LoadedPack;

use crate::progression::PlayerState;

/// Piso e teto da propensão a lesão sintética, em escala 0-100 (percentual,
/// não CA) — mesmo layout de `crate::reputation`.
const MIN_SYNTHETIC_PRONENESS: u8 = 0;
const MAX_SYNTHETIC_PRONENESS: u8 = 100;

/// Gera a propensão a lesão de cada jogador do pack, na mesma ordem (e
/// portanto no mesmo índice denso) que `crate::progression::initial_roster`
/// — mesmo invariante de indexação de `crate::contract::ContractState`:
/// este `Vec` tem que ficar sempre alinhado ao roster que o acompanha.
/// Chamada uma vez por carreira (`app::GameSession::new`): sem atributos
/// ocultos de verdade ainda (RF-JG-02 completo), a propensão de um
/// jogador nunca muda depois de gerada.
#[must_use]
pub fn generate_injury_proneness(pack: &LoadedPack, world_seed: u64) -> Vec<u8> {
    pack.players
        .iter()
        .map(|p| synthetic_proneness(p.id, world_seed))
        .collect()
}

fn synthetic_proneness(player: PlayerId, world_seed: u64) -> u8 {
    let mut rng = DeterministicRng::seeded(
        world_seed,
        "world.injury_proneness",
        u64::from(player.index()),
        0,
    );
    let span = u32::from(MAX_SYNTHETIC_PRONENESS - MIN_SYNTHETIC_PRONENESS) + 1;
    MIN_SYNTHETIC_PRONENESS + u8::try_from(rng.below(span)).expect("span cabe em u8 (0..=100)")
}

/// Piso e teto da chance de lesão por temporada, em milésimos, conforme a
/// propensão (0-100) — piso de 2% pra quem nunca se machuca, teto de 10%
/// pra quem é muito frágil; propensão média (50) dá 6%, a mesma chance
/// fixa de antes desta fatia (preserva a magnitude agregada, só deixa de
/// ser igual pra todo mundo). Não calibrado, só a direção certa.
const MIN_INJURY_CHANCE_PERMILLE: u32 = 20;
const MAX_INJURY_CHANCE_PERMILLE: u32 = 100;

fn injury_chance_permille(proneness: u8) -> u32 {
    MIN_INJURY_CHANCE_PERMILLE
        + u32::from(proneness) * (MAX_INJURY_CHANCE_PERMILLE - MIN_INJURY_CHANCE_PERMILLE) / 100
}

/// Sorteia lesão para cada jogador do `roster`, em ordem de `PlayerId`
/// (determinístico, `ADR 0002`). Substitui completamente o estado anterior
/// — um jogador machucado na temporada passada que não for sorteado de
/// novo está recuperado (`injured` volta a `false`), nunca acumula.
/// `proneness` tem que estar no mesmo índice denso que `roster`
/// (`generate_injury_proneness`, ver o doc da função). `Rng` por jogador
/// por temporada (`docs/02 §5`), mesmo esquema de
/// `crate::progression::advance_season`: machucar o jogador A não muda o
/// sorteio do jogador B.
pub fn roll_injuries(
    roster: &mut [PlayerState],
    proneness: &[u8],
    world_seed: u64,
    season_index: u32,
) {
    for (state, &player_proneness) in roster.iter_mut().zip(proneness) {
        let mut rng = DeterministicRng::seeded(
            world_seed,
            "world.injury",
            u64::from(state.player.index()),
            u64::from(season_index),
        );
        state.injured = rng.chance_per_mille(injury_chance_permille(player_proneness));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::progression::initial_roster;
    use domain::{Ability, ClubId, CompetitionId, GameDate, NationId, Position};
    use pack::{LoadedPack, ResolvedClub, ResolvedNation, ResolvedPlayer};

    fn pack_with_n_players(n: u32) -> LoadedPack {
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
        let players = (0..n)
            .map(|i| ResolvedPlayer {
                id: domain::PlayerId::new(i),
                external_id: format!("ex.a.p{i:02}"),
                first_name: "Nome".to_string(),
                last_name: "Sobrenome".to_string(),
                birth: GameDate::from_ymd(2000, 6, 15),
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

    /// Propensão "média" fixa (50 -> 6%, mesma chance fixa de antes desta
    /// fatia) — pros testes que não são sobre o efeito da propensão em si.
    fn mid_proneness(n: u32) -> Vec<u8> {
        vec![50; n as usize]
    }

    #[test]
    fn ninguem_comeca_machucado() {
        let pack = pack_with_n_players(5);
        let roster = initial_roster(&pack);
        assert!(roster.iter().all(|p| !p.injured));
    }

    #[test]
    fn e_deterministico_para_a_mesma_seed() {
        let pack = pack_with_n_players(20);
        let mut a = initial_roster(&pack);
        let mut b = initial_roster(&pack);
        let proneness = mid_proneness(20);
        roll_injuries(&mut a, &proneness, 42, 0);
        roll_injuries(&mut b, &proneness, 42, 0);
        assert_eq!(a, b);
    }

    #[test]
    fn seeds_diferentes_produzem_sorteios_diferentes() {
        let pack = pack_with_n_players(20);
        let mut a = initial_roster(&pack);
        let mut b = initial_roster(&pack);
        let proneness = mid_proneness(20);
        roll_injuries(&mut a, &proneness, 1, 0);
        roll_injuries(&mut b, &proneness, 2, 0);
        assert_ne!(a, b);
    }

    #[test]
    fn em_um_elenco_grande_algum_jogador_se_machuca() {
        // Sanidade estatística grosseira: com 6% de chance média e 200
        // jogadores, esperar zero lesões seria uma coincidência de ~1 em
        // 10^5 — se isso falhar, é sinal de que o sorteio não está
        // rolando de verdade, não de azar.
        let pack = pack_with_n_players(200);
        let mut roster = initial_roster(&pack);
        let proneness = mid_proneness(200);
        roll_injuries(&mut roster, &proneness, 7, 0);
        assert!(roster.iter().any(|p| p.injured));
    }

    #[test]
    fn jogador_recupera_se_nao_for_sorteado_de_novo() {
        // Varre temporadas até achar uma em que o jogador 0 se machuca, e
        // então até achar uma seguinte em que não — prova que `injured` é
        // recalculado do zero a cada chamada, nunca um contador que só
        // soma (um jogador eternamente machucado seria o sintoma de
        // `roll_injuries` só ligar `injured`, nunca desligar).
        let pack = pack_with_n_players(1);
        let proneness = mid_proneness(1);
        let world_seed = 99;

        let injured_season = (0..200u32).find(|&season| {
            let mut r = initial_roster(&pack);
            roll_injuries(&mut r, &proneness, world_seed, season);
            r[0].injured
        });
        let injured_season =
            injured_season.expect("esperava pelo menos uma temporada machucada em 200 sorteios");

        let recovered = (injured_season + 1..injured_season + 200).any(|season| {
            let mut r = initial_roster(&pack);
            roll_injuries(&mut r, &proneness, world_seed, season);
            !r[0].injured
        });
        assert!(
            recovered,
            "jogador deveria se recuperar em alguma temporada seguinte"
        );
    }

    #[test]
    fn gera_uma_propensao_por_jogador_dentro_dos_limites() {
        let pack = pack_with_n_players(50);
        let proneness = generate_injury_proneness(&pack, 42);
        assert_eq!(proneness.len(), pack.players.len());
        for &p in &proneness {
            assert!(p <= MAX_SYNTHETIC_PRONENESS);
        }
    }

    #[test]
    fn propensao_e_deterministica_para_a_mesma_seed() {
        let pack = pack_with_n_players(20);
        assert_eq!(
            generate_injury_proneness(&pack, 7),
            generate_injury_proneness(&pack, 7)
        );
    }

    #[test]
    fn jogador_fragil_se_machuca_mais_que_resistente_em_media() {
        // Sanidade estatística: o mesmo jogador (mesma seed de carreira),
        // só variando propensão de 0 (nunca) a 100 (muito frágil), deveria
        // se machucar com muito mais frequência no caso frágil, ao longo
        // de muitas temporadas independentes.
        let pack = pack_with_n_players(1);
        let seasons = 0..1000u32;

        let fragile_proneness = vec![100u8];
        let fragile_injuries = seasons
            .clone()
            .filter(|&season| {
                let mut r = initial_roster(&pack);
                roll_injuries(&mut r, &fragile_proneness, 1, season);
                r[0].injured
            })
            .count();

        let resistant_proneness = vec![0u8];
        let resistant_injuries = seasons
            .filter(|&season| {
                let mut r = initial_roster(&pack);
                roll_injuries(&mut r, &resistant_proneness, 1, season);
                r[0].injured
            })
            .count();

        assert!(
            fragile_injuries > resistant_injuries,
            "frágil deveria se machucar mais (frágil: {fragile_injuries}, resistente: {resistant_injuries})"
        );
    }
}

/// Testes de propriedade (`docs/08 §1`).
#[cfg(test)]
mod proptests {
    use super::*;
    use crate::progression::initial_roster;
    use domain::{Ability, ClubId, CompetitionId, GameDate, NationId, Position};
    use pack::{LoadedPack, ResolvedClub, ResolvedNation, ResolvedPlayer};
    use proptest::prelude::*;

    fn pack_with_n_players(n: u32) -> LoadedPack {
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
        let players = (0..n)
            .map(|i| ResolvedPlayer {
                id: domain::PlayerId::new(i),
                external_id: format!("ex.a.p{i:02}"),
                first_name: "Nome".to_string(),
                last_name: "Sobrenome".to_string(),
                birth: GameDate::from_ymd(2000, 6, 15),
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

    proptest! {
        /// `roll_injuries` é puro: mesma entrada, mesmo resultado, para
        /// qualquer seed e temporada, não só os casos de exemplo.
        #[test]
        fn e_puro(world_seed: u64, season_index: u32, n_players in 1u32..30) {
            let pack = pack_with_n_players(n_players);
            let mut a = initial_roster(&pack);
            let mut b = initial_roster(&pack);
            let proneness = generate_injury_proneness(&pack, world_seed);
            roll_injuries(&mut a, &proneness, world_seed, season_index);
            roll_injuries(&mut b, &proneness, world_seed, season_index);
            prop_assert_eq!(a, b);
        }
    }
}
