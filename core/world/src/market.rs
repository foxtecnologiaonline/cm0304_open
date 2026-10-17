//! Ponte entre `crate::progression::PlayerState` e `ai::run_market_day` —
//! `world` é quem sabe o que é um roster de carreira; `ai` só enxerga o
//! [`ai::MarketPlayer`] abstrato (`docs/02 §2`: `ai` depende só de
//! `domain`). [`generate_loyalty`] mora aqui, não em `crate::progression`,
//! porque é o único consumidor de lealdade — `crate::progression` não
//! precisa saber que isso existe. [`generate_ambition`] (sétima fatia de
//! RF-JG-02, `docs/01 §2.3`) mora aqui pelo mesmo motivo: jogador muito
//! ambicioso recusa ir pra um clube de reputação menor que a do seu
//! (`ai::run_market_day`, via [`ai::MarketPlayer::ambition`]).

use domain::{DeterministicRng, Money, PlayerId};
use pack::LoadedPack;

use crate::contract::ContractState;
use crate::progression::PlayerState;

const MIN_SYNTHETIC_LOYALTY: u8 = 0;
const MAX_SYNTHETIC_LOYALTY: u8 = 100;

/// Gera a lealdade de cada jogador do pack, na mesma ordem (e portanto no
/// mesmo índice denso) que `crate::progression::initial_roster` — mesmo
/// invariante de indexação de `crate::contract::ContractState`. Chamada
/// uma vez por carreira (`app::GameSession::new`): sem atributos ocultos
/// de verdade ainda (RF-JG-02 completo, `docs/01 §2.3`), a lealdade de um
/// jogador nunca muda depois de gerada. `ai::run_market_day` usa isso pra
/// nunca vender um jogador muito leal, mesmo que o preço bata.
#[must_use]
pub fn generate_loyalty(pack: &LoadedPack, world_seed: u64) -> Vec<u8> {
    pack.players
        .iter()
        .map(|p| synthetic_loyalty(p.id, world_seed))
        .collect()
}

fn synthetic_loyalty(player: PlayerId, world_seed: u64) -> u8 {
    let mut rng =
        DeterministicRng::seeded(world_seed, "world.loyalty", u64::from(player.index()), 0);
    let span = u32::from(MAX_SYNTHETIC_LOYALTY - MIN_SYNTHETIC_LOYALTY) + 1;
    MIN_SYNTHETIC_LOYALTY + u8::try_from(rng.below(span)).expect("span cabe em u8 (0..=100)")
}

const MIN_SYNTHETIC_AMBITION: u8 = 0;
const MAX_SYNTHETIC_AMBITION: u8 = 100;

/// Gera a ambição de cada jogador do pack, na mesma ordem (e portanto no
/// mesmo índice denso) que `crate::progression::initial_roster`. Chamada
/// uma vez por carreira (`app::GameSession::new`): sem atributos ocultos
/// de verdade ainda (RF-JG-02 completo), a ambição de um jogador nunca
/// muda depois de gerada. `ai::run_market_day` usa isso, junto da
/// reputação de clube, pra recusar transferência que seria um passo atrás
/// de carreira pra um jogador muito ambicioso.
#[must_use]
pub fn generate_ambition(pack: &LoadedPack, world_seed: u64) -> Vec<u8> {
    pack.players
        .iter()
        .map(|p| synthetic_ambition(p.id, world_seed))
        .collect()
}

fn synthetic_ambition(player: PlayerId, world_seed: u64) -> u8 {
    let mut rng =
        DeterministicRng::seeded(world_seed, "world.ambition", u64::from(player.index()), 0);
    let span = u32::from(MAX_SYNTHETIC_AMBITION - MIN_SYNTHETIC_AMBITION) + 1;
    MIN_SYNTHETIC_AMBITION + u8::try_from(rng.below(span)).expect("span cabe em u8 (0..=100)")
}

/// Roda um dia de mercado sobre `roster`/`budgets` (mesmo índice por
/// `PlayerId`/`ClubId` do resto do crate), mutando os dois in-place —
/// transferências reatribuem `PlayerState::club`, `budgets` é debitado do
/// comprador e creditado no vendedor (`ai::run_market_day`, economia
/// fechada). `contracts`, `loyalty` e `ambition` têm que estar no mesmo
/// índice denso que `roster` (`crate::contract`, invariante do módulo); já
/// `club_reputation` está no índice denso de `budgets`/`ClubId`
/// (`crate::reputation::generate_club_reputation`). A duração restante de
/// cada contrato, a idade e o PA de `roster` entram em `ai::market_value`
/// como desconto/prêmio (RF-TR-02); a lealdade filtra quem nunca é
/// candidato a saída e a ambição recusa um passo atrás de reputação
/// (RF-JG-02); todo jogador transferido assina um contrato novo no clube
/// comprador (duração máxima, salário travado no CA do momento da venda —
/// `crate::contract::fresh_contract`), mesma lógica de "clube novo
/// renegocia" de uma renovação automática. Devolve o log de
/// transferências, para quem chama (`app`) reportar o que aconteceu.
pub fn run_market_day(
    roster: &mut [PlayerState],
    contracts: &mut [ContractState],
    loyalty: &[u8],
    ambition: &[u8],
    club_reputation: &[u8],
    budgets: &mut [Money],
) -> Vec<ai::Transfer> {
    let mut players: Vec<ai::MarketPlayer> = roster
        .iter()
        .zip(contracts.iter())
        .zip(loyalty.iter())
        .zip(ambition.iter())
        .map(
            |(((p, contract), &player_loyalty), &player_ambition)| ai::MarketPlayer {
                player: p.player,
                club: p.club,
                position: p.position,
                current_ability: p.ability.current(),
                contract_seasons_remaining: contract.seasons_remaining,
                // `age_years` é `i32` em `PlayerState` (nunca negativo — pack
                // validado garante `birth.year() < reference_year`), saturado
                // em `u8` pra `ai::MarketPlayer`: nenhum pack real chega perto
                // do teto de 255, e nenhum `panic!` com dado externo (`docs/02
                // §9.2`) se algum chegasse.
                age_years: u8::try_from(p.age_years).unwrap_or(u8::MAX),
                potential_ability: p.ability.potential(),
                loyalty: player_loyalty,
                ambition: player_ambition,
            },
        )
        .collect();

    let transfers = ai::run_market_day(&mut players, budgets, club_reputation);

    for (state, updated) in roster.iter_mut().zip(players.iter()) {
        state.club = updated.club;
    }
    for transfer in &transfers {
        let idx = transfer.player.as_usize();
        contracts[idx] = crate::contract::fresh_contract(roster[idx].ability.current());
    }
    transfers
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::progression::initial_roster;
    use domain::{ClubId, CompetitionId, GameDate, NationId, Position};
    use pack::{LoadedPack, ResolvedClub, ResolvedNation, ResolvedPlayer};

    fn two_club_pack() -> LoadedPack {
        let nation = ResolvedNation {
            id: NationId::new(0),
            external_id: "ex".to_string(),
            name: "Exemplolândia".to_string(),
        };
        let clubs = vec![
            ResolvedClub {
                id: ClubId::new(0),
                external_id: "ex.a".to_string(),
                name: "Clube A".to_string(),
                nation: nation.id,
                competition: CompetitionId::new(0),
                founded: None,
                stadium: None,
            },
            ResolvedClub {
                id: ClubId::new(1),
                external_id: "ex.b".to_string(),
                name: "Clube B".to_string(),
                nation: nation.id,
                competition: CompetitionId::new(0),
                founded: None,
                stadium: None,
            },
        ];
        let players = vec![
            ResolvedPlayer {
                id: domain::PlayerId::new(0),
                external_id: "ex.a.p00".to_string(),
                first_name: "Nome".to_string(),
                last_name: "Sobrenome".to_string(),
                birth: GameDate::from_ymd(2000, 1, 1),
                nation: nation.id,
                club: clubs[0].id,
                position: Position::Goalkeeper,
                attributes: domain::PlayerAttributes::default(),
                ability: domain::Ability::new(40, 100),
            },
            ResolvedPlayer {
                id: domain::PlayerId::new(1),
                external_id: "ex.b.p00".to_string(),
                first_name: "Nome".to_string(),
                last_name: "Sobrenome".to_string(),
                birth: GameDate::from_ymd(2000, 1, 1),
                nation: nation.id,
                club: clubs[1].id,
                position: Position::Goalkeeper,
                attributes: domain::PlayerAttributes::default(),
                ability: domain::Ability::new(90, 100),
            },
            ResolvedPlayer {
                id: domain::PlayerId::new(2),
                external_id: "ex.b.p01".to_string(),
                first_name: "Nome".to_string(),
                last_name: "Sobrenome".to_string(),
                birth: GameDate::from_ymd(2000, 1, 1),
                nation: nation.id,
                club: clubs[1].id,
                position: Position::Goalkeeper,
                attributes: domain::PlayerAttributes::default(),
                ability: domain::Ability::new(70, 100),
            },
        ];
        LoadedPack {
            reference_year: 2026,
            nations: vec![nation],
            competitions: vec![],
            clubs,
            players,
        }
    }

    #[test]
    fn transferencia_reatribui_o_clube_no_roster_e_atualiza_orcamentos() {
        let pack = two_club_pack();
        let mut roster = initial_roster(&pack);
        let mut contracts = crate::contract::generate_initial_contracts(&pack, 1);
        let loyalty = vec![0u8; roster.len()];
        let ambition = vec![0u8; roster.len()];
        let club_reputation = vec![50u8; pack.clubs.len()];
        let mut budgets = vec![Money::from_cents(1_000_000_000), Money::ZERO];

        let transfers = run_market_day(
            &mut roster,
            &mut contracts,
            &loyalty,
            &ambition,
            &club_reputation,
            &mut budgets,
        );

        assert_eq!(transfers.len(), 1);
        let t = transfers[0];
        assert_eq!(t.player, domain::PlayerId::new(2)); // reserva do clube B
        assert_eq!(t.to, ClubId::new(0));

        // O roster tem que refletir a mudança de clube.
        assert_eq!(roster[2].club, ClubId::new(0));
        // Os outros dois jogadores não se mexeram.
        assert_eq!(roster[0].club, ClubId::new(0));
        assert_eq!(roster[1].club, ClubId::new(1));
        // Orçamento debitado/creditado pelo mesmo valor (conservação —
        // ver `ai::market::proptests::dinheiro_total_e_conservado` para a
        // propriedade geral).
        assert_eq!(budgets[0] + budgets[1], Money::from_cents(1_000_000_000));
    }

    #[test]
    fn transferencia_assina_contrato_novo_no_clube_comprador() {
        let pack = two_club_pack();
        let mut roster = initial_roster(&pack);
        let mut contracts = crate::contract::generate_initial_contracts(&pack, 1);
        // Força o contrato antigo pra um estado claramente diferente do
        // que uma transferência produziria, pra provar que de fato mudou.
        contracts[2] = crate::contract::ContractState {
            seasons_remaining: 1,
            signed_ability: 1,
        };
        let loyalty = vec![0u8; roster.len()];
        let ambition = vec![0u8; roster.len()];
        let club_reputation = vec![50u8; pack.clubs.len()];
        let mut budgets = vec![Money::from_cents(1_000_000_000), Money::ZERO];

        let transfers = run_market_day(
            &mut roster,
            &mut contracts,
            &loyalty,
            &ambition,
            &club_reputation,
            &mut budgets,
        );
        assert_eq!(transfers.len(), 1);

        let renewed = crate::contract::fresh_contract(roster[2].ability.current());
        assert_eq!(contracts[2], renewed);
    }

    #[test]
    fn jogador_leal_nunca_e_vendido_pela_ponte() {
        // Caixa branca: prova que `loyalty` está de fato ligado nesta
        // ponte, não só em `ai::market` isolado — mesmo cenário dos outros
        // testes (reserva do clube B seria vendido pro clube A), mas com
        // lealdade máxima pra esse jogador especificamente.
        let pack = two_club_pack();
        let mut roster = initial_roster(&pack);
        let mut contracts = crate::contract::generate_initial_contracts(&pack, 1);
        let mut loyalty = vec![0u8; roster.len()];
        loyalty[2] = 100;
        let ambition = vec![0u8; roster.len()];
        let club_reputation = vec![50u8; pack.clubs.len()];
        let mut budgets = vec![Money::from_cents(1_000_000_000), Money::ZERO];

        let transfers = run_market_day(
            &mut roster,
            &mut contracts,
            &loyalty,
            &ambition,
            &club_reputation,
            &mut budgets,
        );

        assert!(transfers.is_empty());
        assert_eq!(roster[2].club, ClubId::new(1));
    }

    #[test]
    fn jogador_ambicioso_recusa_ir_pra_clube_de_reputacao_menor_pela_ponte() {
        // Caixa branca: prova que `ambition`/`club_reputation` estão de
        // fato ligados nesta ponte, não só em `ai::market` isolado — mesmo
        // cenário dos outros testes (reserva do clube B seria vendido pro
        // clube A), mas com ambição máxima e clube A (comprador) menos
        // reputado que o clube B (vendedor atual).
        let pack = two_club_pack();
        let mut roster = initial_roster(&pack);
        let mut contracts = crate::contract::generate_initial_contracts(&pack, 1);
        let loyalty = vec![0u8; roster.len()];
        let mut ambition = vec![0u8; roster.len()];
        ambition[2] = 100;
        let club_reputation = vec![20u8, 80u8]; // clube A (comprador) bem menos reputado
        let mut budgets = vec![Money::from_cents(1_000_000_000), Money::ZERO];

        let transfers = run_market_day(
            &mut roster,
            &mut contracts,
            &loyalty,
            &ambition,
            &club_reputation,
            &mut budgets,
        );

        assert!(transfers.is_empty());
        assert_eq!(roster[2].club, ClubId::new(1));
    }

    #[test]
    fn generate_loyalty_e_deterministico_para_a_mesma_seed() {
        let pack = two_club_pack();
        assert_eq!(generate_loyalty(&pack, 7), generate_loyalty(&pack, 7));
    }

    #[test]
    fn generate_ambition_e_deterministico_para_a_mesma_seed() {
        let pack = two_club_pack();
        assert_eq!(generate_ambition(&pack, 7), generate_ambition(&pack, 7));
    }
}
