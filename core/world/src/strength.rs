//! Força por clube — a partir do elenco de verdade quando o pack declara
//! jogadores (`pack::LoadedPack::players`, `docs/04 §2.1`), com um
//! substituto sintético para quando não declara (packs "só regras", ou
//! ainda sem `people/*.json` preenchido).
//!
//! A força sintética é sorteada uma vez, de forma determinística, e
//! permanece constante durante toda a sequência de temporadas simulada (o
//! "elenco" sintético não evolui) — suficiente para exercitar calendário,
//! tabela e promoção/rebaixamento de ponta a ponta mesmo sem elenco. A
//! força de elenco, ao contrário, é recalculada a cada chamada a partir dos
//! atributos declarados no pack — determinística por construção, sem RNG
//! nenhum envolvido.

use domain::{ClubId, DeterministicRng, Fixed};
use engine::TeamStrength;
use pack::LoadedPack;

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

/// Sorteia a força sintética de um clube — só usada como *fallback* quando
/// [`strength_from_squad`] não tem elenco pra trabalhar.
fn synthetic_strength(club: ClubId, world_seed: u64) -> TeamStrength {
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

/// Calcula a força de um clube a partir do CA (`Ability::current`) médio do
/// seu elenco — `None` se o clube não tem nenhum jogador declarado no pack.
///
/// Simplificação deliberada: a fórmula completa de força de time
/// (`docs/04 §2.1`) pesaria por posição, condição, moral e a tática do
/// treinador — nada disso existe ainda (tática entra no M3). Uma média
/// simples de CA já é suficiente para o motor v0 (que só compara duas
/// forças escalares, `docs/04 §7`) e não exige nenhum dado que o pack ainda
/// não declara. Só aritmética inteira (`ADR 0002`): soma de `u32` dividida
/// por contagem, sem ponto flutuante em nenhum passo.
#[must_use]
pub fn strength_from_squad(pack: &LoadedPack, club: ClubId) -> Option<TeamStrength> {
    let squad = pack.players_of(club);
    if squad.is_empty() {
        return None;
    }
    let total: u32 = squad.iter().map(|p| u32::from(p.ability.current())).sum();
    let average = total / squad.len() as u32;
    Some(TeamStrength::new(Fixed::from_int(average as i32)))
}

/// Busca a força de um clube pelo seu id denso — painel de acesso indexado,
/// igual ao resto do domínio (`docs/02 §6.1`).
#[must_use]
pub fn strength_of(strengths: &[TeamStrength], club: ClubId) -> TeamStrength {
    strengths[club.as_usize()]
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

    fn player_with_ca(club: ClubId, nation: domain::NationId, ca: u8) -> ResolvedPlayer {
        ResolvedPlayer {
            id: domain::PlayerId::new(0),
            external_id: "p".to_string(),
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
    fn strength_from_squad_e_a_media_exata_do_ca_do_elenco() {
        let mut p = pack_without_players(1);
        let club = p.clubs[0].id;
        let nation = p.nations[0].id;
        p.players = vec![
            player_with_ca(club, nation, 100),
            player_with_ca(club, nation, 120),
            player_with_ca(club, nation, 140),
        ];
        // (100 + 120 + 140) / 3 = 120, exato — escolhido de propósito para
        // não depender de arredondamento de divisão inteira.
        let strength = strength_from_squad(&p, club).unwrap();
        assert_eq!(strength.value(), Fixed::from_int(120));
    }

    #[test]
    fn generate_strengths_usa_elenco_quando_disponivel_e_ignora_a_seed() {
        let mut p = pack_without_players(1);
        let club = p.clubs[0].id;
        let nation = p.nations[0].id;
        p.players = vec![player_with_ca(club, nation, 90)];

        let a = generate_strengths(&p, 1);
        let b = generate_strengths(&p, 2);
        // Com elenco declarado, a força não usa RNG nenhum — seeds
        // diferentes têm que produzir exatamente o mesmo resultado, ao
        // contrário do caminho sintético (`caminho_sintetico_seeds_diferentes_dao_forcas_diferentes`).
        assert_eq!(a, b);
        assert_eq!(a[0].value(), Fixed::from_int(90));
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
