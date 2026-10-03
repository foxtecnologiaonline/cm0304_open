//! [`engine::TeamMatchProfile`] por clube — a primeira vez que `world`
//! calcula algo a partir dos **36 atributos visíveis** de um jogador
//! (`pack::ResolvedPlayer::attributes`, `docs/03 §3.1`), não só do CA
//! agregado que [`crate::strength`] já usava. Mesma filosofia de lá: elenco
//! de verdade quando o pack declara jogadores, sorteio sintético
//! determinístico quando não declara.
//!
//! A fórmula aqui é uma simplificação deliberada de `docs/04 §2.3`
//! (`p_gol = base × f(finalização, frieza, pé, ângulo, pressão) ÷
//! g(goleiro: reflexos, posicionamento, jogo aéreo)`): sem ângulo, pressão
//! ou pé (pedem zona e lado dominante, que não existem ainda), só a média
//! dos atributos que já existem e já fazem sentido sem eles.
//!
//! `strength` também é escalado pela condição física média dos titulares
//! (`crate::condition`) — um time desgastado rende um pouco menos que o
//! CA bruto sugeriria. `finishing`/`goalkeeping` não são escalados (fatia
//! mínima: fadiga afeta volume/força, não necessariamente técnica).

use std::collections::HashSet;

use domain::{Attribute, ClubId, DeterministicRng, Fixed, PlayerId, Position};
use engine::{FinishingQuality, GoalkeepingQuality, TeamMatchProfile};
use pack::{LoadedPack, ResolvedPlayer};

use crate::condition::FULL_CONDITION;
use crate::progression::PlayerState;
use crate::strength::synthetic_strength;

/// Valor "neutro" de qualidade — usado quando a formação titular não tem
/// nenhum goleiro ou nenhum atacante pra tirar a média de verdade (elenco
/// incompleto). É o centro da escala de atributos visíveis (1-20,
/// `docs/03 §4`), a mesma convenção usada nos testes de `engine::simulate`.
const NEUTRAL_QUALITY: i32 = 10;

/// Piso e teto da qualidade sintética — mesma faixa de `NEUTRAL_QUALITY`
/// ao redor, pra combinar com a escala real de atributos (1-20).
const MIN_SYNTHETIC_QUALITY: i32 = 5;
const MAX_SYNTHETIC_QUALITY: i32 = 15;

/// Gera um [`TeamMatchProfile`] por clube a partir do pack estático —
/// equivalente de [`crate::strength::generate_strengths`], mas com
/// finalização/goleiro reais também. Usado por `managerfc-cli calibrate`
/// (que não acompanha estado de carreira, só quer um instantâneo do pack).
#[must_use]
pub fn generate_match_profiles(pack: &LoadedPack, world_seed: u64) -> Vec<TeamMatchProfile> {
    pack.clubs
        .iter()
        .map(|club| {
            profile_from_squad(pack, club.id)
                .unwrap_or_else(|| synthetic_profile(club.id, world_seed))
        })
        .collect()
}

/// Igual a [`generate_match_profiles`], mas via o roster de carreira
/// (`crate::progression::PlayerState`) — a versão que `app::GameSession`
/// usa a cada temporada, depois de aplicar `crate::market::run_market_day`
/// e antes de `crate::progression::advance_season`.
#[must_use]
pub fn generate_match_profiles_from_roster(
    pack: &LoadedPack,
    roster: &[PlayerState],
    world_seed: u64,
) -> Vec<TeamMatchProfile> {
    pack.clubs
        .iter()
        .map(|club| {
            profile_from_roster(pack, roster, club.id)
                .unwrap_or_else(|| synthetic_profile(club.id, world_seed))
        })
        .collect()
}

fn synthetic_profile(club: ClubId, world_seed: u64) -> TeamMatchProfile {
    TeamMatchProfile {
        strength: synthetic_strength(club, world_seed),
        finishing: FinishingQuality::new(Fixed::from_int(synthetic_quality(
            club,
            world_seed,
            "world.synthetic_finishing",
        ))),
        goalkeeping: GoalkeepingQuality::new(Fixed::from_int(synthetic_quality(
            club,
            world_seed,
            "world.synthetic_goalkeeping",
        ))),
    }
}

fn synthetic_quality(club: ClubId, world_seed: u64, domain_tag: &str) -> i32 {
    let mut rng = DeterministicRng::seeded(world_seed, domain_tag, u64::from(club.index()), 0);
    let span = (MAX_SYNTHETIC_QUALITY - MIN_SYNTHETIC_QUALITY + 1) as u32;
    MIN_SYNTHETIC_QUALITY + rng.below(span) as i32
}

/// Média de dois atributos (1-20) de um jogador, como `i32` — conveniência
/// pra não repetir `(get(a) + get(b)) / 2` em todo lugar.
fn average_two(attrs: domain::PlayerAttributes, a: Attribute, b: Attribute) -> i32 {
    (i32::from(attrs.get(a)) + i32::from(attrs.get(b))) / 2
}

fn average_three(attrs: domain::PlayerAttributes, a: Attribute, b: Attribute, c: Attribute) -> i32 {
    (i32::from(attrs.get(a)) + i32::from(attrs.get(b)) + i32::from(attrs.get(c))) / 3
}

/// Constrói o perfil a partir de uma lista de `(posição, CA, atributos,
/// condição)` — o núcleo comum entre [`profile_from_squad`] (atributos do
/// pack, CA do pack, sempre `FULL_CONDITION`: pack estático não tem estado
/// de carreira) e [`profile_from_roster`] (atributos do pack, CA e
/// condição do roster): os 36 atributos visíveis nunca evoluem na carreira
/// ainda (sem treino), só o CA e a condição evoluem (`crate::progression`,
/// `crate::condition`), então "de onde vêm esses dois" é a única diferença
/// real entre os dois caminhos.
fn build_profile(
    squad: &[(Position, u8, domain::PlayerAttributes, u8)],
) -> Option<TeamMatchProfile> {
    if squad.is_empty() {
        return None;
    }
    let ratings: Vec<ai::PlayerRating> = squad
        .iter()
        .enumerate()
        .map(|(idx, &(position, ca, _, _))| ai::PlayerRating {
            player: domain::PlayerId::new(idx as u32),
            position,
            current_ability: ca,
        })
        .collect();
    let starter_indices: Vec<usize> = ai::select_starting_eleven(&ratings)
        .iter()
        .map(|id| id.as_usize())
        .collect();

    // CA efetivo de cada titular já escalado pela própria condição, depois
    // a média de 11 — não a média de CA bruto escalada por uma condição de
    // time única, que trataria um titular cansado e um descansado como se
    // tivessem o mesmo desgaste.
    let total: u32 = starter_indices
        .iter()
        .map(|&i| u32::from(squad[i].1) * u32::from(squad[i].3) / u32::from(FULL_CONDITION))
        .sum();
    let strength = engine::TeamStrength::new(Fixed::from_int(
        (total / starter_indices.len() as u32) as i32,
    ));

    let goalkeeping = starter_indices
        .iter()
        .find(|&&i| squad[i].0 == Position::Goalkeeper)
        .map(|&i| {
            average_three(
                squad[i].2,
                Attribute::Reflexes,
                Attribute::Positioning,
                Attribute::AerialAbility,
            )
        })
        .unwrap_or(NEUTRAL_QUALITY);

    let forwards: Vec<usize> = starter_indices
        .iter()
        .copied()
        .filter(|&i| squad[i].0 == Position::Forward)
        .collect();
    let finishing = if forwards.is_empty() {
        NEUTRAL_QUALITY
    } else {
        let sum: i32 = forwards
            .iter()
            .map(|&i| average_two(squad[i].2, Attribute::Finishing, Attribute::Composure))
            .sum();
        sum / forwards.len() as i32
    };

    Some(TeamMatchProfile {
        strength,
        finishing: FinishingQuality::new(Fixed::from_int(finishing)),
        goalkeeping: GoalkeepingQuality::new(Fixed::from_int(goalkeeping)),
    })
}

/// A partir do pack estático (CA e atributos declarados, sem estado de
/// carreira) — `None` se o clube não tem nenhum jogador no pack.
#[must_use]
pub fn profile_from_squad(pack: &LoadedPack, club: ClubId) -> Option<TeamMatchProfile> {
    let squad: Vec<(Position, u8, domain::PlayerAttributes, u8)> = pack
        .players_of(club)
        .iter()
        .map(|p: &&ResolvedPlayer| {
            (
                p.position,
                p.ability.current(),
                p.attributes,
                FULL_CONDITION,
            )
        })
        .collect();
    build_profile(&squad)
}

/// A partir do roster de carreira: CA de [`PlayerState`] (evolui com
/// `crate::progression`, muda de clube com `crate::market`), atributos
/// visíveis ainda vêm do pack (nunca evoluem, ver o doc de [`build_profile`]).
/// Jogador machucado (`crate::injuries`) nunca entra no elenco considerado
/// — mesma exclusão de [`crate::strength::strength_from_roster`].
#[must_use]
pub fn profile_from_roster(
    pack: &LoadedPack,
    roster: &[PlayerState],
    club: ClubId,
) -> Option<TeamMatchProfile> {
    let squad: Vec<(Position, u8, domain::PlayerAttributes, u8)> = roster
        .iter()
        .filter(|p| p.club == club && !p.injured)
        .map(|p| {
            (
                p.position,
                p.ability.current(),
                pack.players[p.player.as_usize()].attributes,
                p.condition,
            )
        })
        .collect();
    build_profile(&squad)
}

/// Titulares de `club` no `roster` — mesma seleção que [`profile_from_roster`]
/// faz internamente (maior CA por posição, `ai::select_starting_eleven`,
/// jogador machucado nunca candidato), mas devolvendo o [`PlayerId`] real
/// em vez do índice local que [`build_profile`] usa por dentro. É o que
/// `crate::condition::apply_season_fatigue` precisa pra saber quem de fato
/// jogou — `build_profile` não expõe isso, só o perfil agregado.
#[must_use]
pub fn starters_from_roster(roster: &[PlayerState], club: ClubId) -> Vec<PlayerId> {
    let squad: Vec<&PlayerState> = roster
        .iter()
        .filter(|p| p.club == club && !p.injured)
        .collect();
    let ratings: Vec<ai::PlayerRating> = squad
        .iter()
        .enumerate()
        .map(|(idx, p)| ai::PlayerRating {
            player: PlayerId::new(idx as u32),
            position: p.position,
            current_ability: p.ability.current(),
        })
        .collect();
    ai::select_starting_eleven(&ratings)
        .into_iter()
        .map(|local_id| squad[local_id.as_usize()].player)
        .collect()
}

/// [`starters_from_roster`] de todo clube do pack, achatado num único
/// conjunto — o que `app::GameSession` passa para
/// `crate::condition::apply_season_fatigue` no fim de cada temporada.
#[must_use]
pub fn starters_from_roster_all_clubs(
    pack: &LoadedPack,
    roster: &[PlayerState],
) -> HashSet<PlayerId> {
    pack.clubs
        .iter()
        .flat_map(|club| starters_from_roster(roster, club.id))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{Ability, CompetitionId, GameDate, NationId, PlayerAttributes, PlayerId};
    use pack::{ResolvedClub, ResolvedNation};
    use std::path::PathBuf;

    fn example_pack() -> LoadedPack {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/example-two-tier");
        pack::load_and_validate(&path).unwrap().pack
    }

    fn pack_with_players(specs: &[(Position, u8, PlayerAttributes)]) -> LoadedPack {
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
            .map(|(i, &(position, ca, attrs))| ResolvedPlayer {
                id: PlayerId::new(i as u32),
                external_id: format!("ex.a.p{i:02}"),
                first_name: "Nome".to_string(),
                last_name: "Sobrenome".to_string(),
                birth: GameDate::from_ymd(2000, 6, 15),
                nation: nation.id,
                club: club.id,
                position,
                attributes: attrs,
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

    fn attrs_with(pairs: &[(Attribute, u8)]) -> PlayerAttributes {
        let mut raw = [1u8; domain::attributes::N_ATTR];
        for &(attr, value) in pairs {
            raw[attr.index()] = value;
        }
        PlayerAttributes::from_raw(raw)
    }

    #[test]
    fn sem_elenco_devolve_none() {
        let p = pack_with_players(&[]);
        assert_eq!(profile_from_squad(&p, p.clubs[0].id), None);
    }

    #[test]
    fn goleiro_titular_determina_a_qualidade_de_goleiro() {
        let gk_attrs = attrs_with(&[
            (Attribute::Reflexes, 18),
            (Attribute::Positioning, 16),
            (Attribute::AerialAbility, 14),
        ]);
        let p = pack_with_players(&[(Position::Goalkeeper, 100, gk_attrs)]);
        let profile = profile_from_squad(&p, p.clubs[0].id).unwrap();
        // (18 + 16 + 14) / 3 = 16.
        assert_eq!(profile.goalkeeping.value(), Fixed::from_int(16));
    }

    #[test]
    fn atacante_titular_determina_a_qualidade_de_finalizacao() {
        let fw_attrs = attrs_with(&[(Attribute::Finishing, 18), (Attribute::Composure, 12)]);
        let p = pack_with_players(&[(Position::Forward, 100, fw_attrs)]);
        let profile = profile_from_squad(&p, p.clubs[0].id).unwrap();
        // (18 + 12) / 2 = 15.
        assert_eq!(profile.finishing.value(), Fixed::from_int(15));
    }

    #[test]
    fn sem_goleiro_titular_usa_qualidade_neutra() {
        let p = pack_with_players(&[(Position::Forward, 100, PlayerAttributes::default())]);
        let profile = profile_from_squad(&p, p.clubs[0].id).unwrap();
        assert_eq!(
            profile.goalkeeping.value(),
            Fixed::from_int(NEUTRAL_QUALITY)
        );
    }

    #[test]
    fn sem_atacante_titular_usa_qualidade_neutra() {
        let p = pack_with_players(&[(Position::Goalkeeper, 100, PlayerAttributes::default())]);
        let profile = profile_from_squad(&p, p.clubs[0].id).unwrap();
        assert_eq!(profile.finishing.value(), Fixed::from_int(NEUTRAL_QUALITY));
    }

    #[test]
    fn profile_from_roster_segue_o_clube_atual_e_o_ca_do_roster() {
        let fw_attrs = attrs_with(&[(Attribute::Finishing, 20), (Attribute::Composure, 20)]);
        let mut p = pack_with_players(&[(Position::Forward, 50, fw_attrs)]);
        p.clubs.push(ResolvedClub {
            id: ClubId::new(1),
            external_id: "ex.b".to_string(),
            name: "Clube B".to_string(),
            nation: p.nations[0].id,
            competition: CompetitionId::new(0),
            founded: None,
            stadium: None,
        });
        let mut roster = crate::progression::initial_roster(&p);
        assert_eq!(profile_from_roster(&p, &roster, ClubId::new(1)), None);

        roster[0].club = ClubId::new(1); // transferência simulada
        roster[0].ability = Ability::new(150, 200); // progressão simulada
        let profile = profile_from_roster(&p, &roster, ClubId::new(1)).unwrap();
        assert_eq!(profile.strength.value(), Fixed::from_int(150)); // CA do roster, não do pack
        assert_eq!(profile.finishing.value(), Fixed::from_int(20)); // atributos continuam vindo do pack
        assert_eq!(profile_from_roster(&p, &roster, ClubId::new(0)), None);
    }

    #[test]
    fn example_pack_produz_perfil_para_todo_clube() {
        let p = example_pack();
        for club in &p.clubs {
            let profile = profile_from_squad(&p, club.id);
            assert!(
                profile.is_some(),
                "clube '{}' sem perfil no pack de exemplo",
                club.external_id
            );
        }
    }

    #[test]
    fn generate_match_profiles_e_deterministico() {
        let p = example_pack();
        assert_eq!(
            generate_match_profiles(&p, 7),
            generate_match_profiles(&p, 7)
        );
    }

    #[test]
    fn condicao_reduzida_do_titular_reduz_a_forca_efetiva() {
        let p = pack_with_players(&[(Position::Midfielder, 100, PlayerAttributes::default())]);
        let mut roster = crate::progression::initial_roster(&p);
        let fresh = profile_from_roster(&p, &roster, p.clubs[0].id)
            .unwrap()
            .strength;

        roster[0].condition = 70;
        let tired = profile_from_roster(&p, &roster, p.clubs[0].id)
            .unwrap()
            .strength;

        assert!(
            tired.value() < fresh.value(),
            "titular com condição 70 deveria render menos que com condição 100"
        );
        // CA 100 × condição 70/100 = 70, exato.
        assert_eq!(tired.value(), Fixed::from_int(70));
    }

    #[test]
    fn starters_from_roster_bate_com_quem_profile_from_roster_de_fato_escalou() {
        // `profile_from_squad`'s teste de goleiro/atacante já prova que
        // `build_profile` escolhe por CA; este prova que
        // `starters_from_roster` (usada só por `crate::condition`) escolhe
        // exatamente os mesmos titulares, não uma seleção divergente.
        let p = pack_with_players(&[
            (Position::Goalkeeper, 150, PlayerAttributes::default()),
            (Position::Goalkeeper, 50, PlayerAttributes::default()), // reserva, CA menor
        ]);
        let roster = crate::progression::initial_roster(&p);
        let starters = starters_from_roster(&roster, p.clubs[0].id);
        assert_eq!(starters, vec![PlayerId::new(0)]); // só o de CA 150
    }

    #[test]
    fn starters_from_roster_ignora_jogador_machucado() {
        let p = pack_with_players(&[(Position::Midfielder, 100, PlayerAttributes::default())]);
        let mut roster = crate::progression::initial_roster(&p);
        roster[0].injured = true;
        assert!(starters_from_roster(&roster, p.clubs[0].id).is_empty());
    }

    #[test]
    fn starters_from_roster_all_clubs_cobre_todos_os_clubes_do_pack_de_exemplo() {
        let p = example_pack();
        let roster = crate::progression::initial_roster(&p);
        let starters = starters_from_roster_all_clubs(&p, &roster);
        // 16 clubes × 11 titulares = 176, sem repetição (jogadores são
        // exclusivos de um clube).
        assert_eq!(starters.len(), 16 * 11);
    }
}
