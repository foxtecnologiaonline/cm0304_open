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
//!
//! **"Jogo importante"** — quinta fatia real de RF-JG-02 (atributos
//! ocultos, `docs/01 §2.3`) — mora aqui porque a copa é a única "partida
//! importante" que existe hoje (um mata-mata de eliminação direta, ao
//! contrário de qualquer rodada de liga): [`generate_big_game_mentality`]
//! sorteia uma mentalidade sintética (0-100) por jogador, uma vez por
//! carreira, e [`apply_big_game_bonus`] escala a força de cada clube **só
//! para a copa** conforme a média dos titulares (0,9x com média 0 a 1,1x
//! com média 100) antes de chamar [`run_cup`] — a força usada pela liga no
//! mesmo `profiles` não muda. Só existe no caminho de carreira
//! (`roster: Some`); no caminho estático (`managerfc-cli
//! calibrate`/`bench`), `apply_big_game_bonus` devolve `profiles` sem
//! tocar em nada, mesma disciplina de todo outro atributo oculto de
//! RF-JG-02 (`world::injuries`/`world::progression`/`world::market`/
//! `world::discipline`).

use domain::{ClubId, CompetitionId, DeterministicRng, PlayerId};
use engine::{MatchContext, TeamMatchProfile};
use pack::{LoadedPack, ResolvedCompetition};

use crate::progression::PlayerState;

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

/// Piso e teto da mentalidade sintética de "jogo importante", em escala
/// 0-100 — mesmo layout de `crate::discipline`/`crate::reputation`.
const MIN_SYNTHETIC_BIG_GAME_MENTALITY: u8 = 0;
const MAX_SYNTHETIC_BIG_GAME_MENTALITY: u8 = 100;

/// Gera a mentalidade de "jogo importante" de cada jogador do pack, na mesma
/// ordem (e portanto no mesmo índice denso) que
/// `crate::progression::initial_roster`. Chamada uma vez por carreira
/// (`app::GameSession::new`): sem atributos ocultos de verdade ainda
/// (RF-JG-02 completo), a mentalidade de um jogador nunca muda depois de
/// gerada.
#[must_use]
pub fn generate_big_game_mentality(pack: &LoadedPack, world_seed: u64) -> Vec<u8> {
    pack.players
        .iter()
        .map(|p| synthetic_big_game_mentality(p.id, world_seed))
        .collect()
}

fn synthetic_big_game_mentality(player: PlayerId, world_seed: u64) -> u8 {
    let mut rng = DeterministicRng::seeded(
        world_seed,
        "world.big_game_mentality",
        u64::from(player.index()),
        0,
    );
    let span = u32::from(MAX_SYNTHETIC_BIG_GAME_MENTALITY - MIN_SYNTHETIC_BIG_GAME_MENTALITY) + 1;
    MIN_SYNTHETIC_BIG_GAME_MENTALITY
        + u8::try_from(rng.below(span)).expect("span cabe em u8 (0..=100)")
}

/// Multiplicador de força de um clube na copa, em milésimos (`Fixed`),
/// conforme a mentalidade média dos titulares: `900` (média 0, 0,9x) a
/// `1100` (média 100, 1,1x) — não calibrado, só a direção certa.
const BIG_GAME_BONUS_BASE_PERMILLE: i64 = 900;
const BIG_GAME_BONUS_SLOPE_PERMILLE: i64 = 2;

/// Aplica o bônus de "jogo importante" ao perfil de cada clube antes de
/// simular a copa — ver o doc do módulo. `career`, quando `Some((roster,
/// mentality))`, escala a força de cada clube pela média de
/// [`synthetic_big_game_mentality`] dos seus titulares atuais
/// (`crate::quality::starters_from_roster`); `mentality` tem que estar no
/// mesmo índice denso que `roster` inteiro (`generate_big_game_mentality`).
/// `None` devolve `profiles` sem nenhuma mudança — caminho estático, ver o
/// doc do módulo.
#[must_use]
pub fn apply_big_game_bonus(
    pack: &LoadedPack,
    career: Option<(&[PlayerState], &[u8])>,
    profiles: &[TeamMatchProfile],
) -> Vec<TeamMatchProfile> {
    let Some((roster, mentality)) = career else {
        return profiles.to_vec();
    };
    pack.clubs
        .iter()
        .map(|club| {
            let idx = club.id.as_usize();
            let starters = crate::quality::starters_from_roster(roster, club.id);
            if starters.is_empty() {
                return profiles[idx];
            }
            let total: u32 = starters
                .iter()
                .map(|&p| u32::from(mentality[p.as_usize()]))
                .sum();
            let average = total / starters.len() as u32;
            scale_strength_for_cup(profiles[idx], average)
        })
        .collect()
}

fn scale_strength_for_cup(profile: TeamMatchProfile, average_mentality: u32) -> TeamMatchProfile {
    let multiplier_permille =
        BIG_GAME_BONUS_BASE_PERMILLE + i64::from(average_mentality) * BIG_GAME_BONUS_SLOPE_PERMILLE;
    let multiplier = domain::Fixed::from_ratio(multiplier_permille, 1000);
    TeamMatchProfile {
        strength: engine::TeamStrength::new(profile.strength.value() * multiplier),
        ..profile
    }
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
/// `MatchContext.fixture`. Delega o empacotamento a `crate::fixture_key`
/// (compartilhado com `world::season::fixture_key`) em vez de reimplementar
/// os mesmos bits aqui — duas cópias da mesma constante `BITS` só ficam
/// sincronizadas por comentário, e divergir uma delas silenciosamente
/// colidiria chaves de partidas diferentes (ver doc de `crate::fixture_key`).
/// `is_cup = true` seta o bit reservado que separa este espaço de chaves do
/// da liga: sem isso, uma partida de copa entre dois clubes que também se
/// enfrentam na liga na mesma temporada reproduziria byte a byte o mesmo
/// sorteio da partida de liga. `(temporada, mandante, visitante)` já é
/// suficiente sem a rodada: o mesmo par nunca se repete duas vezes na mesma
/// copa (eliminação), e temporadas diferentes nunca compartilham chave.
fn cup_fixture_key(season_index: u32, home: ClubId, away: ClubId) -> u64 {
    crate::fixture_key::pack(season_index, home, away, true)
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{Ability, GameDate, NationId, PlayerAttributes, Position};
    use engine::{FinishingQuality, GoalkeepingQuality, TeamStrength};
    use pack::{ResolvedClub, ResolvedNation, ResolvedPlayer};

    /// Dois clubes, um jogador titular cada — o mínimo pra
    /// `crate::quality::starters_from_roster` devolver um titular por
    /// clube sem precisar de um elenco completo.
    fn two_club_pack() -> LoadedPack {
        let nation = ResolvedNation {
            id: NationId::new(0),
            external_id: "ex".to_string(),
            name: "Ex".to_string(),
        };
        let club_a = ResolvedClub {
            id: ClubId::new(0),
            external_id: "ex.a".to_string(),
            name: "A".to_string(),
            nation: nation.id,
            competition: CompetitionId::new(0),
            founded: None,
            stadium: None,
        };
        let club_b = ResolvedClub {
            id: ClubId::new(1),
            external_id: "ex.b".to_string(),
            name: "B".to_string(),
            nation: nation.id,
            competition: CompetitionId::new(0),
            founded: None,
            stadium: None,
        };
        let players = vec![
            ResolvedPlayer {
                id: PlayerId::new(0),
                external_id: "ex.a.p00".to_string(),
                first_name: "N".to_string(),
                last_name: "S".to_string(),
                birth: GameDate::from_ymd(2000, 1, 1),
                nation: nation.id,
                club: club_a.id,
                position: Position::Midfielder,
                attributes: PlayerAttributes::default(),
                ability: Ability::new(100, 150),
            },
            ResolvedPlayer {
                id: PlayerId::new(1),
                external_id: "ex.b.p00".to_string(),
                first_name: "N".to_string(),
                last_name: "S".to_string(),
                birth: GameDate::from_ymd(2000, 1, 1),
                nation: nation.id,
                club: club_b.id,
                position: Position::Midfielder,
                attributes: PlayerAttributes::default(),
                ability: Ability::new(100, 150),
            },
        ];
        LoadedPack {
            reference_year: 2026,
            nations: vec![nation],
            competitions: vec![],
            clubs: vec![club_a, club_b],
            players,
        }
    }

    #[test]
    fn generate_big_game_mentality_e_deterministica_para_a_mesma_seed() {
        let p = two_club_pack();
        assert_eq!(
            generate_big_game_mentality(&p, 7),
            generate_big_game_mentality(&p, 7)
        );
    }

    #[test]
    fn apply_big_game_bonus_sem_career_devolve_profiles_inalterados() {
        let p = two_club_pack();
        let profiles = vec![profile(300), profile(300)];
        assert_eq!(apply_big_game_bonus(&p, None, &profiles), profiles);
    }

    #[test]
    fn apply_big_game_bonus_favorece_time_com_mentalidade_mais_alta() {
        let p = two_club_pack();
        let roster = crate::progression::initial_roster(&p);
        // Jogador 0 (clube A): mentalidade máxima; jogador 1 (clube B): mínima.
        let mentality = vec![100u8, 0u8];
        let profiles = vec![profile(300), profile(300)];

        let boosted = apply_big_game_bonus(&p, Some((&roster, &mentality)), &profiles);

        assert!(boosted[0].strength.value() > boosted[1].strength.value());
        // 300 × 1,1 (mentalidade 100) = 330; 300 × 0,9 (mentalidade 0) = 270.
        assert_eq!(boosted[0].strength.value(), domain::Fixed::from_int(330));
        assert_eq!(boosted[1].strength.value(), domain::Fixed::from_int(270));
    }

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
        // `crate::fixture_key` já prova a propriedade em geral
        // (`liga_e_copa_nunca_colidem_para_a_mesma_entrada`) — este teste
        // confirma que `cup_fixture_key` de fato delega pra lá com
        // `is_cup = true`, não uma reimplementação local que poderia
        // divergir.
        let league_key = crate::fixture_key::pack(0, ClubId::new(1), ClubId::new(2), false);
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
