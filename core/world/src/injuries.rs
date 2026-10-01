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

use domain::DeterministicRng;

use crate::progression::PlayerState;

/// Chance de um jogador se machucar numa temporada, em milésimos — um
/// número de bolso (6%), não calibrado contra nenhum alvo publicado (ver o
/// doc do módulo para o porquê).
const INJURY_CHANCE_PERMILLE: u32 = 60;

/// Sorteia lesão para cada jogador do `roster`, em ordem de `PlayerId`
/// (determinístico, `ADR 0002`). Substitui completamente o estado anterior
/// — um jogador machucado na temporada passada que não for sorteado de
/// novo está recuperado (`injured` volta a `false`), nunca acumula.
/// `Rng` por jogador por temporada (`docs/02 §5`), mesmo esquema de
/// `crate::progression::advance_season`: machucar o jogador A não muda o
/// sorteio do jogador B.
pub fn roll_injuries(roster: &mut [PlayerState], world_seed: u64, season_index: u32) {
    for state in roster.iter_mut() {
        let mut rng = DeterministicRng::seeded(
            world_seed,
            "world.injury",
            u64::from(state.player.index()),
            u64::from(season_index),
        );
        state.injured = rng.chance_per_mille(INJURY_CHANCE_PERMILLE);
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
        roll_injuries(&mut a, 42, 0);
        roll_injuries(&mut b, 42, 0);
        assert_eq!(a, b);
    }

    #[test]
    fn seeds_diferentes_produzem_sorteios_diferentes() {
        let pack = pack_with_n_players(20);
        let mut a = initial_roster(&pack);
        let mut b = initial_roster(&pack);
        roll_injuries(&mut a, 1, 0);
        roll_injuries(&mut b, 2, 0);
        assert_ne!(a, b);
    }

    #[test]
    fn em_um_elenco_grande_algum_jogador_se_machuca() {
        // Sanidade estatística grosseira: com 6% de chance e 200 jogadores,
        // esperar zero lesões seria uma coincidência de ~1 em 10^5 — se
        // isso falhar, é sinal de que o sorteio não está rolando de
        // verdade, não de azar.
        let pack = pack_with_n_players(200);
        let mut roster = initial_roster(&pack);
        roll_injuries(&mut roster, 7, 0);
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
        let world_seed = 99;

        let injured_season = (0..200u32).find(|&season| {
            let mut r = initial_roster(&pack);
            roll_injuries(&mut r, world_seed, season);
            r[0].injured
        });
        let injured_season =
            injured_season.expect("esperava pelo menos uma temporada machucada em 200 sorteios");

        let recovered = (injured_season + 1..injured_season + 200).any(|season| {
            let mut r = initial_roster(&pack);
            roll_injuries(&mut r, world_seed, season);
            !r[0].injured
        });
        assert!(
            recovered,
            "jogador deveria se recuperar em alguma temporada seguinte"
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
            roll_injuries(&mut a, world_seed, season_index);
            roll_injuries(&mut b, world_seed, season_index);
            prop_assert_eq!(a, b);
        }
    }
}
