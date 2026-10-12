//! Ponte entre `crate::progression::PlayerState` e `ai::run_market_day` —
//! `world` é quem sabe o que é um roster de carreira; `ai` só enxerga o
//! [`ai::MarketPlayer`] abstrato (`docs/02 §2`: `ai` depende só de
//! `domain`).

use domain::Money;

use crate::contract::ContractState;
use crate::progression::PlayerState;

/// Roda um dia de mercado sobre `roster`/`budgets` (mesmo índice por
/// `PlayerId`/`ClubId` do resto do crate), mutando os dois in-place —
/// transferências reatribuem `PlayerState::club`, `budgets` é debitado do
/// comprador e creditado no vendedor (`ai::run_market_day`, economia
/// fechada). `contracts` tem que estar no mesmo índice denso que `roster`
/// (`crate::contract`, invariante do módulo): todo jogador transferido
/// assina um contrato novo no clube comprador (duração máxima, salário
/// travado no CA do momento da venda — `crate::contract::fresh_contract`),
/// mesma lógica de "clube novo renegocia" de uma renovação automática.
/// Devolve o log de transferências, para quem chama (`app`) reportar o que
/// aconteceu.
pub fn run_market_day(
    roster: &mut [PlayerState],
    contracts: &mut [ContractState],
    budgets: &mut [Money],
) -> Vec<ai::Transfer> {
    let mut players: Vec<ai::MarketPlayer> = roster
        .iter()
        .map(|p| ai::MarketPlayer {
            player: p.player,
            club: p.club,
            position: p.position,
            current_ability: p.ability.current(),
        })
        .collect();

    let transfers = ai::run_market_day(&mut players, budgets);

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
        let mut budgets = vec![Money::from_cents(1_000_000_000), Money::ZERO];

        let transfers = run_market_day(&mut roster, &mut contracts, &mut budgets);

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
        let mut budgets = vec![Money::from_cents(1_000_000_000), Money::ZERO];

        let transfers = run_market_day(&mut roster, &mut contracts, &mut budgets);
        assert_eq!(transfers.len(), 1);

        let renewed = crate::contract::fresh_contract(roster[2].ability.current());
        assert_eq!(contracts[2], renewed);
    }
}
