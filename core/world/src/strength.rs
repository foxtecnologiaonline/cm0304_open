//! Força por clube — a partir do elenco de verdade quando o pack declara
//! jogadores (`pack::LoadedPack::players`, `docs/04 §2.1`), com um
//! substituto sintético para quando não declara (packs "só regras", ou
//! ainda sem `people/*.json` preenchido).
//!
//! A força sintética é sorteada uma vez, de forma determinística, e
//! permanece constante durante toda a sequência de temporadas simulada (o
//! "elenco" sintético não evolui) — suficiente para exercitar calendário,
//! tabela e promoção/rebaixamento de ponta a ponta mesmo sem elenco. A
//! força de elenco, ao contrário, é recalculada a cada chamada a partir do
//! CA médio dos **titulares** — `ai::select_starting_eleven` escolhe os 11
//! (`docs/07-roadmap.md` M1: "IA de escalação"), não o elenco inteiro,
//! porque um banco de reservas fracos não deveria arrastar pra baixo a
//! força de um time com um bom time titular. Determinística por
//! construção, sem RNG nenhum envolvido.

use domain::{ClubId, DeterministicRng, Fixed};
use engine::TeamStrength;
use pack::LoadedPack;

use crate::progression::PlayerState;

/// Piso e teto da força sintética — mesma ordem de grandeza usada nos
/// testes do `engine` (`Fixed::from_int(100)` como referência "média") e
/// próxima da escala de CA (0..=200, `docs/03 §4`), para que os dois modos
/// produzam partidas de granularidade comparável.
const MIN_SYNTHETIC_STRENGTH: i32 = 50;
const MAX_SYNTHETIC_STRENGTH: i32 = 150;

/// Gera uma força por clube, indexada pelo id denso do clube
/// (`ClubId::as_usize()`) — o mesmo layout SoA-friendly de `docs/02 §6.1`.
/// Clubes com elenco declarado usam [`strength_from_squad`]; os demais
/// caem no sorteio sintético descrito no doc do módulo.
#[must_use]
pub fn generate_strengths(pack: &LoadedPack, world_seed: u64) -> Vec<TeamStrength> {
    pack.clubs
        .iter()
        .map(|club| {
            strength_from_squad(pack, club.id)
                .unwrap_or_else(|| synthetic_strength(club.id, world_seed))
        })
        .collect()
}

/// Sorteia a força sintética de um clube — *fallback* quando
/// [`strength_from_squad`] não tem elenco pra trabalhar. `pub(crate)` porque
/// `crate::quality` reaproveita exatamente a mesma lógica para o lado
/// "força" de um [`engine::TeamMatchProfile`] sintético.
pub(crate) fn synthetic_strength(club: ClubId, world_seed: u64) -> TeamStrength {
    let mut rng = DeterministicRng::seeded(
        world_seed,
        "world.club_strength",
        u64::from(club.index()),
        0,
    );
    let span = (MAX_SYNTHETIC_STRENGTH - MIN_SYNTHETIC_STRENGTH + 1) as u32;
    let value = MIN_SYNTHETIC_STRENGTH + rng.below(span) as i32;
    TeamStrength::new(Fixed::from_int(value))
}

/// Calcula a força de um clube a partir do CA (`Ability::current`) médio dos
/// seus **titulares** (`ai::select_starting_eleven`, ver o doc do módulo) —
/// `None` se o clube não tem nenhum jogador declarado no pack.
///
/// Simplificação deliberada: a fórmula completa de força de time
/// (`docs/04 §2.1`) pesaria por posição, condição, moral e a tática do
/// treinador — nada disso existe ainda (tática entra no M3). Uma média
/// simples de CA dos titulares já é suficiente para o motor v0 (que só
/// compara duas forças escalares, `docs/04 §7`) e não exige nenhum dado que
/// o pack ainda não declara. Só aritmética inteira (`ADR 0002`): soma de
/// `u32` dividida por contagem, sem ponto flutuante em nenhum passo.
#[must_use]
pub fn strength_from_squad(pack: &LoadedPack, club: ClubId) -> Option<TeamStrength> {
    let squad = pack.players_of(club);
    if squad.is_empty() {
        return None;
    }
    let ratings: Vec<ai::PlayerRating> = squad
        .iter()
        .map(|p| ai::PlayerRating {
            player: p.id,
            position: p.position,
            current_ability: p.ability.current(),
        })
        .collect();
    let starters = ai::select_starting_eleven(&ratings);
    // `PlayerId::as_usize()` indexa `pack.players` globalmente (a ordem em
    // que `pack::resolve` atribuiu os ids, não a posição dentro de
    // `squad`) — mesmo esquema de indexação que `strength_from_roster` usa
    // para `roster`, abaixo.
    average_ability(
        starters
            .iter()
            .map(|&id| pack.players[id.as_usize()].ability.current()),
    )
}

/// Média de CA como [`TeamStrength`] — `None` só se `values` estiver vazio
/// (não deveria acontecer quando chamado com o resultado de
/// `ai::select_starting_eleven` sobre um `squad` não-vazio, mas devolver
/// `Option` em vez de assumir isso mantém a função honesta sobre sua
/// própria pré-condição).
fn average_ability(values: impl Iterator<Item = u8>) -> Option<TeamStrength> {
    let (total, count) = values.fold((0u32, 0u32), |(total, count), v| {
        (total + u32::from(v), count + 1)
    });
    if count == 0 {
        return None;
    }
    Some(TeamStrength::new(Fixed::from_int((total / count) as i32)))
}

/// Busca a força de um clube pelo seu id denso — painel de acesso indexado,
/// igual ao resto do domínio (`docs/02 §6.1`).
#[must_use]
pub fn strength_of(strengths: &[TeamStrength], club: ClubId) -> TeamStrength {
    strengths[club.as_usize()]
}

/// Igual a [`strength_from_squad`], mas lê o CA **e o clube atual** de um
/// `roster` mutável (`crate::progression`) em vez do CA e do clube fixos
/// declarados no pack — o que faz a força de um clube refletir tanto a
/// progressão de jogadores quanto transferências (`crate::market`) ao
/// longo de uma carreira, não o instantâneo do pack no dia em que foi
/// carregado. Note que o elenco é agrupado por `roster[i].club`, **não**
/// por `pack.players_of(club)`: depois de uma transferência, os dois
/// divergem de propósito (o pack nunca muda; é assim que sabemos quem
/// pertence a quem *agora*). `pack` só entra aqui pra saber quantos
/// clubes existem no total (`generate_strengths_from_roster`), não pra
/// filtrar elenco.
#[must_use]
pub fn strength_from_roster(roster: &[PlayerState], club: ClubId) -> Option<TeamStrength> {
    let squad: Vec<ai::PlayerRating> = roster
        .iter()
        .filter(|p| p.club == club)
        .map(|p| ai::PlayerRating {
            player: p.player,
            position: p.position,
            current_ability: p.ability.current(),
        })
        .collect();
    if squad.is_empty() {
        return None;
    }
    let starters = ai::select_starting_eleven(&squad);
    average_ability(
        starters
            .iter()
            .map(|&id| roster[id.as_usize()].ability.current()),
    )
}

/// Igual a [`generate_strengths`], mas via [`strength_from_roster`] — a
/// versão que `app::GameSession` usa a cada temporada, depois de aplicar
/// [`crate::progression::advance_season`] no `roster` da temporada
/// anterior. `generate_strengths` continua existindo à parte para quem só
/// quer o instantâneo do pack sem estado de carreira (`managerfc-cli
/// calibrate`, que não acompanha progressão).
#[must_use]
pub fn generate_strengths_from_roster(
    pack: &LoadedPack,
    roster: &[PlayerState],
    world_seed: u64,
) -> Vec<TeamStrength> {
    pack.clubs
        .iter()
        .map(|club| {
            strength_from_roster(roster, club.id)
                .unwrap_or_else(|| synthetic_strength(club.id, world_seed))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{Ability, CompetitionId, GameDate, NationId, Position};
    use pack::{ResolvedClub, ResolvedNation, ResolvedPlayer};
    use std::path::PathBuf;

    fn example_pack() -> LoadedPack {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/example-two-tier");
        pack::load_and_validate(&path).unwrap().pack
    }

    /// Um `LoadedPack` mínimo, montado à mão (sem tocar disco) com `n`
    /// clubes e **nenhum** jogador — para testar isoladamente o caminho
    /// sintético de `generate_strengths`, sem depender de o pack de exemplo
    /// do repositório continuar sem elenco (ele passa a ter um a partir
    /// deste incremento — ver `packs/core/example-two-tier/people/`).
    fn pack_without_players(n_clubs: u32) -> LoadedPack {
        let nation = ResolvedNation {
            id: NationId::new(0),
            external_id: "ex".to_string(),
            name: "Exemplolândia".to_string(),
        };
        let clubs = (0..n_clubs)
            .map(|i| ResolvedClub {
                id: ClubId::new(i),
                external_id: format!("ex.{i}"),
                name: format!("Clube {i}"),
                nation: nation.id,
                competition: CompetitionId::new(0),
                founded: None,
                stadium: None,
            })
            .collect();
        LoadedPack {
            reference_year: 2026,
            nations: vec![nation],
            competitions: vec![],
            clubs,
            players: vec![],
        }
    }

    #[test]
    fn caminho_sintetico_gera_uma_forca_por_clube_dentro_dos_limites() {
        let p = pack_without_players(8);
        let strengths = generate_strengths(&p, 42);
        assert_eq!(strengths.len(), p.clubs.len());
        for s in &strengths {
            assert!(s.value() >= Fixed::from_int(MIN_SYNTHETIC_STRENGTH));
            assert!(s.value() <= Fixed::from_int(MAX_SYNTHETIC_STRENGTH));
        }
    }

    #[test]
    fn caminho_sintetico_e_deterministico_para_a_mesma_seed() {
        let p = pack_without_players(8);
        let a = generate_strengths(&p, 7);
        let b = generate_strengths(&p, 7);
        assert_eq!(a, b);
    }

    #[test]
    fn caminho_sintetico_seeds_diferentes_dao_forcas_diferentes() {
        let p = pack_without_players(8);
        let a = generate_strengths(&p, 1);
        let b = generate_strengths(&p, 2);
        assert_ne!(a, b);
    }

    #[test]
    fn strength_from_squad_e_none_sem_elenco() {
        let p = pack_without_players(1);
        assert_eq!(strength_from_squad(&p, ClubId::new(0)), None);
    }

    fn player_with_ca(id: u32, club: ClubId, nation: domain::NationId, ca: u8) -> ResolvedPlayer {
        ResolvedPlayer {
            id: domain::PlayerId::new(id),
            external_id: format!("p{id}"),
            first_name: "Nome".to_string(),
            last_name: "Sobrenome".to_string(),
            birth: GameDate::from_ymd(2000, 1, 1),
            nation,
            club,
            position: Position::Midfielder,
            attributes: domain::PlayerAttributes::default(),
            ability: Ability::new(ca, ca + 10),
        }
    }

    #[test]
    fn strength_from_squad_e_a_media_exata_do_ca_do_elenco_quando_cabe_todo_na_formacao() {
        let mut p = pack_without_players(1);
        let club = p.clubs[0].id;
        let nation = p.nations[0].id;
        p.players = vec![
            player_with_ca(0, club, nation, 100),
            player_with_ca(1, club, nation, 120),
            player_with_ca(2, club, nation, 140),
        ];
        // Só 3 meio-campistas, a formação (`ai::FORMATION`) permite até 4 —
        // os 3 titularizam, então a média é do elenco inteiro mesmo.
        // (100 + 120 + 140) / 3 = 120, exato — escolhido de propósito para
        // não depender de arredondamento de divisão inteira.
        let strength = strength_from_squad(&p, club).unwrap();
        assert_eq!(strength.value(), Fixed::from_int(120));
    }

    #[test]
    fn strength_from_squad_ignora_reservas_fracos_fora_da_escalacao() {
        let mut p = pack_without_players(1);
        let club = p.clubs[0].id;
        let nation = p.nations[0].id;
        // 6 meio-campistas, mas a formação só escala 4 — os 2 piores (CA 10
        // e 20) deveriam ser ignorados no cálculo de força, não arrastar a
        // média pra baixo.
        p.players = vec![
            player_with_ca(0, club, nation, 100),
            player_with_ca(1, club, nation, 110),
            player_with_ca(2, club, nation, 120),
            player_with_ca(3, club, nation, 130),
            player_with_ca(4, club, nation, 20), // banco
            player_with_ca(5, club, nation, 10), // banco
        ];
        // Titulares: 100, 110, 120, 130 -> média 115.
        let strength = strength_from_squad(&p, club).unwrap();
        assert_eq!(strength.value(), Fixed::from_int(115));
    }

    #[test]
    fn generate_strengths_usa_elenco_quando_disponivel_e_ignora_a_seed() {
        let mut p = pack_without_players(1);
        let club = p.clubs[0].id;
        let nation = p.nations[0].id;
        p.players = vec![player_with_ca(0, club, nation, 90)];

        let a = generate_strengths(&p, 1);
        let b = generate_strengths(&p, 2);
        // Com elenco declarado, a força não usa RNG nenhum — seeds
        // diferentes têm que produzir exatamente o mesmo resultado, ao
        // contrário do caminho sintético (`caminho_sintetico_seeds_diferentes_dao_forcas_diferentes`).
        assert_eq!(a, b);
        assert_eq!(a[0].value(), Fixed::from_int(90));
    }

    #[test]
    fn strength_from_roster_segue_o_clube_atual_do_roster_nao_o_do_pack() {
        // Pack estático: um jogador de CA 200 pertence ao clube 0. Depois
        // de uma transferência simulada (mutar `roster[0].club` direto,
        // sem passar por `ai::market`), a força tem que "seguir" o
        // jogador — clube 0 fica sem elenco, clube 1 ganha o reforço —
        // exatamente o comportamento que `pack.players_of` sozinho jamais
        // capturaria (ele não sabe de transferência nenhuma).
        let mut p = pack_without_players(2);
        let club0 = p.clubs[0].id;
        let nation = p.nations[0].id;
        p.players = vec![player_with_ca(0, club0, nation, 200)];

        let mut roster = crate::progression::initial_roster(&p);
        assert_eq!(
            strength_from_roster(&roster, p.clubs[0].id),
            Some(TeamStrength::new(Fixed::from_int(200)))
        );
        assert_eq!(strength_from_roster(&roster, p.clubs[1].id), None);

        roster[0].club = p.clubs[1].id; // transferência simulada
        assert_eq!(strength_from_roster(&roster, p.clubs[0].id), None);
        assert_eq!(
            strength_from_roster(&roster, p.clubs[1].id),
            Some(TeamStrength::new(Fixed::from_int(200)))
        );
    }

    #[test]
    fn example_pack_carrega_com_elenco_real_e_usa_forca_de_elenco() {
        let p = example_pack();
        assert!(
            !p.players.is_empty(),
            "pack de exemplo deveria declarar jogadores em people/*.json"
        );
        for club in &p.clubs {
            assert!(
                strength_from_squad(&p, club.id).is_some(),
                "clube '{}' sem elenco no pack de exemplo",
                club.external_id
            );
        }
    }
}
