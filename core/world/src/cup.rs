//! Copa — mata-mata de fase única (`pack::Format::Knockout`,
//! `docs/03-modelo-de-dados.md §7`).
//!
//! Cada rodada é pareada por [`rules::pair_round`] a partir dos vencedores
//! da rodada anterior; a primeira rodada parte da lista de participantes
//! (ver [`cup_participants`]) na ordem de `ClubId` — sem cerimônia de
//! sorteio (`rules::bracket` documenta por quê). Participantes vêm de
//! **todos os clubes do país** da competição
//! (`pack::ResolvedCompetition::nation`), nunca de uma inscrição própria
//! como em `RoundRobin`: nenhum clube aponta `competition` para uma copa
//! (um clube só tem uma competição "de origem", a liga).
//!
//! Empate no tempo normal é decidido por um sorteio de moeda determinístico
//! e sem viés de força (50/50) em vez de simular prorrogação e pênaltis
//! evento a evento — pênaltis de verdade já são próximos de um sorteio, e
//! modelar isso de verdade pediria um subsistema novo do motor só para uma
//! fração pequena dos jogos. Fatia mínima documentada, no mesmo espírito de
//! `world::injuries` para RF-JG-08.
//!
//! Partidas de copa **não** entram nas contagens agregadas de
//! `SeasonResult` (`matches_played`/`total_goals`/`total_shots`/etc,
//! `docs/04 §4.1`) — esses alvos de calibração são de liga ("futebol
//! europeu de primeira divisão"), e somar jogos de copa ali enviesaria a
//! calibração sem ninguém perceber (foi exatamente esse tipo de
//! contaminação silenciosa que `managerfc-cli calibrate --check` existe
//! para detectar, `docs/04 §4.2`).

use domain::{ClubId, CompetitionId, DeterministicRng};
use engine::{MatchContext, TeamMatchProfile};
use pack::{LoadedPack, ResolvedCompetition};

/// Uma partida de mata-mata já disputada — sempre com um vencedor (nunca
/// fica empatada, ver doc do módulo).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CupMatch {
    pub round: u32,
    pub home: ClubId,
    pub away: ClubId,
    pub home_goals: u32,
    pub away_goals: u32,
    /// `true` se `home_goals == away_goals` no tempo normal e o sorteio de
    /// moeda decidiu quem avança.
    pub decided_by_coin_toss: bool,
    pub winner: ClubId,
}

/// Resultado completo de uma copa numa temporada: cada rodada, na ordem em
/// que foi disputada (rodada 0 = primeira), e o campeão final.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CupResult {
    pub competition: CompetitionId,
    pub rounds: Vec<Vec<CupMatch>>,
    pub champion: ClubId,
}

/// Participantes de uma copa: todos os clubes do país da competição — ver
/// doc do módulo. `pack::resolve::check_competition_club_counts` já garante
/// na validação do pack que essa contagem bate com `format.teams` e é
/// potência de 2; esta função não revalida (confia num pack já validado,
/// como o resto de `world`).
#[must_use]
pub fn cup_participants(pack: &LoadedPack, competition: &ResolvedCompetition) -> Vec<ClubId> {
    pack.clubs
        .iter()
        .filter(|c| c.nation == competition.nation)
        .map(|c| c.id)
        .collect()
}

/// Simula uma copa inteira (todas as rodadas até sobrar um campeão) para
/// `competition`. `participants` deve estar na ordem do chaveamento (ver
/// [`cup_participants`]); `profiles[club.as_usize()]` dá a força/qualidade
/// de cada clube — o mesmo vetor que a liga usa na mesma temporada.
///
/// `None` se `participants` não puder ser pareado em alguma rodada (número
/// que não é potência de 2) — não deveria acontecer com um pack validado,
/// mas não é um `panic!`: `docs/02-arquitetura.md §9.2` proíbe exatamente
/// isso, e `world::season::run_season` já segue a mesma disciplina para
/// `rules::round_robin` (pula a competição em vez de travar o mundo).
#[must_use]
pub fn run_cup(
    competition: CompetitionId,
    participants: &[ClubId],
    profiles: &[TeamMatchProfile],
    world_seed: u64,
    season_index: u32,
) -> Option<CupResult> {
    let mut contenders = participants.to_vec();
    let mut rounds = Vec::new();
    let mut round_index = 0u32;

    while contenders.len() > 1 {
        let pairs = rules::pair_round(&contenders).ok()?;
        let mut winners = Vec::with_capacity(pairs.len());
        let mut matches = Vec::with_capacity(pairs.len());

        for (home, away) in pairs {
            let key = cup_fixture_key(season_index, home, away);
            let ctx = MatchContext {
                world_seed,
                fixture: key,
            };
            let events =
                engine::simulate(profiles[home.as_usize()], profiles[away.as_usize()], ctx);
            let (home_goals, away_goals) = engine::score(&events);

            let (winner, decided_by_coin_toss) = if home_goals > away_goals {
                (home, false)
            } else if away_goals > home_goals {
                (away, false)
            } else {
                let mut coin = DeterministicRng::seeded(world_seed, "world.cup.penalties", key, 0);
                if coin.chance_per_mille(500) {
                    (home, true)
                } else {
                    (away, true)
                }
            };

            winners.push(winner);
            matches.push(CupMatch {
                round: round_index,
                home,
                away,
                home_goals,
                away_goals,
                decided_by_coin_toss,
                winner,
            });
        }

        rounds.push(matches);
        contenders = winners;
        round_index += 1;
    }

    let champion = *contenders.first()?;
    Some(CupResult {
        competition,
        rounds,
        champion,
    })
}

/// Combina temporada + o par (mandante, visitante) num `u64` para
/// `MatchContext.fixture` — mesma técnica de `world::season::fixture_key`,
/// com um bit reservado (o mais alto) para nunca colidir com o espaço de
/// chaves da liga: sem isso, uma partida de copa entre dois clubes que
/// também se enfrentam na liga na mesma temporada reproduziria byte a byte
/// o mesmo sorteio da partida de liga — dois jogos "diferentes" com o
/// resultado idêntico por coincidência de implementação, não por acaso do
/// mundo. `(temporada, mandante, visitante)` já é suficiente sem a rodada:
/// o mesmo par nunca se repete duas vezes na mesma copa (eliminação), e
/// temporadas diferentes nunca compartilham essa chave.
fn cup_fixture_key(season_index: u32, home: ClubId, away: ClubId) -> u64 {
    const BITS: u32 = 20; // mesma folga de `world::season::fixture_key`
    const CUP_FLAG: u64 = 1 << 63;
    debug_assert!(home.index() < (1 << BITS));
    debug_assert!(away.index() < (1 << BITS));
    debug_assert!(season_index < (1 << BITS));
    CUP_FLAG
        | (u64::from(season_index) << (2 * BITS))
        | (u64::from(home.index()) << BITS)
        | u64::from(away.index())
}

#[cfg(test)]
mod tests {
    use super::*;
    use engine::{FinishingQuality, GoalkeepingQuality, TeamStrength};

    fn profile(strength: i32) -> TeamMatchProfile {
        TeamMatchProfile {
            strength: TeamStrength::new(domain::Fixed::from_int(strength)),
            finishing: FinishingQuality::new(domain::Fixed::from_int(10)),
            goalkeeping: GoalkeepingQuality::new(domain::Fixed::from_int(10)),
        }
    }

    fn clubs(n: u32) -> Vec<ClubId> {
        (0..n).map(ClubId::new).collect()
    }

    fn uniform_profiles(n: u32) -> Vec<TeamMatchProfile> {
        (0..n).map(|_| profile(300)).collect()
    }

    #[test]
    fn copa_de_dois_times_produz_um_campeao_em_uma_rodada() {
        let result = run_cup(
            CompetitionId::new(0),
            &clubs(2),
            &uniform_profiles(2),
            42,
            0,
        )
        .unwrap();
        assert_eq!(result.rounds.len(), 1);
        assert_eq!(result.rounds[0].len(), 1);
        assert!(result.champion == ClubId::new(0) || result.champion == ClubId::new(1));
    }

    #[test]
    fn copa_de_dezesseis_times_produz_quatro_rodadas() {
        // log2(16) = 4: oitavas, quartas, semi, final.
        let result = run_cup(
            CompetitionId::new(0),
            &clubs(16),
            &uniform_profiles(16),
            7,
            0,
        )
        .unwrap();
        assert_eq!(result.rounds.len(), 4);
        assert_eq!(result.rounds[0].len(), 8);
        assert_eq!(result.rounds[1].len(), 4);
        assert_eq!(result.rounds[2].len(), 2);
        assert_eq!(result.rounds[3].len(), 1);
    }

    #[test]
    fn numero_de_participantes_que_nao_e_potencia_de_dois_devolve_none() {
        assert!(run_cup(CompetitionId::new(0), &clubs(6), &uniform_profiles(6), 1, 0).is_none());
    }

    #[test]
    fn vencedor_de_cada_partida_e_sempre_um_dos_dois_times_que_jogaram() {
        let result = run_cup(
            CompetitionId::new(0),
            &clubs(8),
            &uniform_profiles(8),
            99,
            0,
        )
        .unwrap();
        for round in &result.rounds {
            for m in round {
                assert!(m.winner == m.home || m.winner == m.away);
            }
        }
    }

    #[test]
    fn time_muito_mais_forte_tende_a_ser_campeao() {
        // Sanidade grosseira (como `engine::simulate`'s
        // `time_muito_mais_forte_vence_a_grande_maioria_das_simulacoes`):
        // um único time muito mais forte que os outros 7 deveria ser
        // campeão na maioria das seeds, não só por acaso (1/8 = 12,5%).
        let mut profiles = uniform_profiles(8);
        profiles[0] = profile(2000); // clube 0 muito mais forte que o resto
        let mut champions_is_club0 = 0u32;
        let trials = 200u64;
        for seed in 0..trials {
            let result = run_cup(CompetitionId::new(0), &clubs(8), &profiles, seed, 0).unwrap();
            if result.champion == ClubId::new(0) {
                champions_is_club0 += 1;
            }
        }
        let rate_percent = champions_is_club0 * 100 / trials as u32;
        assert!(
            rate_percent >= 50,
            "clube muito mais forte só foi campeão em {rate_percent}% das {trials} copas"
        );
    }

    #[test]
    fn mesma_seed_produz_a_mesma_copa() {
        let a = run_cup(CompetitionId::new(0), &clubs(8), &uniform_profiles(8), 5, 2).unwrap();
        let b = run_cup(CompetitionId::new(0), &clubs(8), &uniform_profiles(8), 5, 2).unwrap();
        assert_eq!(a, b);
    }

    #[test]
    fn seeds_diferentes_tendem_a_produzir_copas_diferentes() {
        let a = run_cup(CompetitionId::new(0), &clubs(8), &uniform_profiles(8), 1, 0).unwrap();
        let b = run_cup(CompetitionId::new(0), &clubs(8), &uniform_profiles(8), 2, 0).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn temporadas_diferentes_nao_repetem_a_mesma_copa() {
        // Sem `season_index` na chave, o mesmo chaveamento (times idênticos,
        // mesma ordem) produziria a mesma copa toda temporada — o
        // equivalente de `fixture_key_muda_com_a_temporada` em
        // `world::season`, mas para copa.
        let a = run_cup(CompetitionId::new(0), &clubs(8), &uniform_profiles(8), 3, 0).unwrap();
        let b = run_cup(CompetitionId::new(0), &clubs(8), &uniform_profiles(8), 3, 1).unwrap();
        assert_ne!(a, b);
    }

    #[test]
    fn cup_fixture_key_nunca_colide_com_fixture_key_da_liga() {
        // O bit mais alto reservado (`CUP_FLAG`) garante isso por
        // construção: replica aqui a mesma fórmula de
        // `world::season::fixture_key` (sem acessar a função privada) só
        // para provar que o bit extra realmente separa os dois espaços,
        // nos limites documentados de `season_index`/`ClubId`.
        const BITS: u32 = 20;
        let league_key = (0u64 << (2 * BITS)) | (u64::from(1u32) << BITS) | u64::from(2u32);
        let cup_key = cup_fixture_key(0, ClubId::new(1), ClubId::new(2));
        assert_ne!(league_key, cup_key);
        assert_eq!(
            cup_key & (1 << 63),
            1 << 63,
            "bit de copa deveria estar setado"
        );
        assert_eq!(
            league_key & (1 << 63),
            0,
            "chave de liga nunca usa o bit mais alto"
        );
    }

    #[test]
    fn cup_participants_sao_todos_os_clubes_do_pais_nao_so_os_da_competicao() {
        let mut raw_pack = pack::LoadedPack::default();
        raw_pack.nations.push(pack::ResolvedNation {
            id: domain::NationId::new(0),
            external_id: "ex".to_string(),
            name: "Exemplo".to_string(),
        });
        let league = domain::CompetitionId::new(0);
        let cup = domain::CompetitionId::new(1);
        for i in 0..4 {
            raw_pack.clubs.push(pack::ResolvedClub {
                id: ClubId::new(i),
                external_id: format!("c{i}"),
                name: format!("Clube {i}"),
                nation: domain::NationId::new(0),
                competition: league, // nenhum clube aponta pra `cup`
                founded: None,
                stadium: None,
            });
        }
        let cup_comp = ResolvedCompetition {
            id: cup,
            external_id: "ex.cup".to_string(),
            name: "Copa".to_string(),
            nation: domain::NationId::new(0),
            tier: 0,
            format: pack::Format::Knockout { teams: 4 },
            tiebreakers: vec![],
            promotion: pack::Movement { to: None, slots: 0 },
            relegation: pack::Movement { to: None, slots: 0 },
        };
        let participants = cup_participants(&raw_pack, &cup_comp);
        assert_eq!(participants.len(), 4);
    }
}
