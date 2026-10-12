//! Contrato de jogador — duração e salário travado — fatia de "duração" de
//! RF-CL-02 (`docs/01-requisitos.md §2.2`), complementando a fatia de
//! "despesa" já existente (`crate::finance::pay_salaries`).
//!
//! Deliberadamente **separado** de [`crate::progression::PlayerState`]:
//! [`ContractState`] é um estado paralelo indexado pelo mesmo id denso de
//! jogador, mesmo padrão de `budgets`/`stadium_capacities` em
//! `app::GameSession` (`crate::finance`) — assim quem só precisa de
//! CA/idade/clube (a maior parte do crate) não paga o custo de saber sobre
//! contrato, e `PlayerState`/`initial_roster` não precisam de `world_seed`
//! pra nascer. **Invariante que quem usa este módulo tem que preservar:**
//! um `Vec<ContractState>` tem que ficar sempre no mesmo índice denso que o
//! `roster` que o acompanha — nunca ordenar, filtrar ou remover um sem o
//! outro.
//!
//! Sem negociação de verdade (`RF-TR-03`/`04`, M2/M3) nem agente livre
//! (`RF-TR-07`, M3): a cada `AdvanceSeason`, a duração restante de todo
//! contrato decrementa ([`advance_contracts`]); ao chegar a zero, o
//! contrato "renova" automaticamente — nova duração sorteada, salário
//! travado no CA atual do jogador. Toda transferência
//! (`crate::market::run_market_day`) também assina um contrato novo: um
//! clube que acaba de comprar um jogador lhe dá o contrato mais longo
//! possível (proteger o investimento), travado no CA do momento da venda.
//! `crate::finance::pay_salaries` usa o CA travado, não o CA corrente —
//! assim o salário só muda quando o contrato de fato é renegociado, como
//! um contrato de verdade, não a cada ponto de CA ganho durante o contrato.

use domain::DeterministicRng;
use pack::LoadedPack;

use crate::progression::PlayerState;

/// Piso e teto de duração de um contrato novo, em temporadas — faixa
/// plausível (1 a 5 anos), não calibrada.
const MIN_SEASONS: u8 = 1;
const MAX_SEASONS: u8 = 5;

/// Estado de contrato de um jogador — ver o doc do módulo para o
/// invariante de indexação que quem usa isto tem que preservar.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ContractState {
    pub seasons_remaining: u8,
    /// CA no momento da última assinatura (criação da carreira, renovação
    /// automática ou transferência) — `crate::finance::pay_salaries`
    /// calcula o salário a partir deste valor, não do CA corrente.
    pub signed_ability: u8,
}

/// Contrato novo de duração máxima, travado em `signed_ability` — o que
/// toda transferência assina (ver o doc do módulo) e o que uma renovação
/// automática usa de template.
pub(crate) fn fresh_contract(signed_ability: u8) -> ContractState {
    ContractState {
        seasons_remaining: MAX_SEASONS,
        signed_ability,
    }
}

fn roll_duration(rng: &mut DeterministicRng) -> u8 {
    let span = u32::from(MAX_SEASONS - MIN_SEASONS + 1);
    MIN_SEASONS + u8::try_from(rng.below(span)).expect("faixa pequena e fixa cabe em u8")
}

/// Gera o contrato inicial de cada jogador do pack, na mesma ordem (e
/// portanto no mesmo índice denso) que `crate::progression::initial_roster`
/// — duração sorteada entre 1 e 5 temporadas, salário travado no CA
/// inicial do pack.
#[must_use]
pub fn generate_initial_contracts(pack: &LoadedPack, world_seed: u64) -> Vec<ContractState> {
    pack.players
        .iter()
        .map(|p| {
            let mut rng =
                DeterministicRng::seeded(world_seed, "world.contract", u64::from(p.id.index()), 0);
            ContractState {
                seasons_remaining: roll_duration(&mut rng),
                signed_ability: p.ability.current(),
            }
        })
        .collect()
}

/// Decrementa a duração de todo contrato em `contracts`; quem chega a
/// zero renova (nova duração sorteada, salário travado no CA atual de
/// `roster`, no mesmo índice — ver o invariante do doc do módulo).
/// `roster` já deveria refletir a progressão desta temporada
/// (`crate::progression::advance_season` chamado antes — `app::GameSession::dispatch`
/// faz isso), pra a renovação travar o CA pós-crescimento, não o de antes
/// da temporada. Devolve quantos contratos de fato renovaram (não conta
/// os renovados por transferência, `crate::market::run_market_day` — esses
/// já saem do mercado com contrato novo, não chegam aqui zerados).
pub fn advance_contracts(
    contracts: &mut [ContractState],
    roster: &[PlayerState],
    world_seed: u64,
    season_index: u32,
) -> usize {
    let mut renewed = 0usize;
    for (idx, contract) in contracts.iter_mut().enumerate() {
        contract.seasons_remaining = contract.seasons_remaining.saturating_sub(1);
        if contract.seasons_remaining == 0 {
            let mut rng = DeterministicRng::seeded(
                world_seed,
                "world.contract",
                u64::from(roster[idx].player.index()),
                u64::from(season_index) + 1,
            );
            contract.seasons_remaining = roll_duration(&mut rng);
            contract.signed_ability = roster[idx].ability.current();
            renewed += 1;
        }
    }
    renewed
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::progression::initial_roster;
    use domain::{ClubId, CompetitionId, GameDate, NationId, PlayerId, Position};
    use pack::{ResolvedClub, ResolvedNation, ResolvedPlayer};
    use std::path::PathBuf;

    fn example_pack() -> LoadedPack {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/example-two-tier");
        pack::load_and_validate(&path).unwrap().pack
    }

    fn single_player_pack(ca: u8) -> LoadedPack {
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
        let player = ResolvedPlayer {
            id: PlayerId::new(0),
            external_id: "ex.a.p00".to_string(),
            first_name: "Nome".to_string(),
            last_name: "Sobrenome".to_string(),
            birth: GameDate::from_ymd(2000, 1, 1),
            nation: nation.id,
            club: club.id,
            position: Position::Midfielder,
            attributes: domain::PlayerAttributes::default(),
            ability: domain::Ability::new(ca, 200),
        };
        LoadedPack {
            reference_year: 2026,
            nations: vec![nation],
            competitions: vec![],
            clubs: vec![club],
            players: vec![player],
        }
    }

    #[test]
    fn gera_um_contrato_por_jogador_dentro_dos_limites() {
        let p = example_pack();
        let contracts = generate_initial_contracts(&p, 42);
        assert_eq!(contracts.len(), p.players.len());
        for c in &contracts {
            assert!(c.seasons_remaining >= MIN_SEASONS);
            assert!(c.seasons_remaining <= MAX_SEASONS);
        }
    }

    #[test]
    fn contrato_inicial_trava_o_ca_inicial_do_pack() {
        let p = single_player_pack(77);
        let contracts = generate_initial_contracts(&p, 1);
        assert_eq!(contracts[0].signed_ability, 77);
    }

    #[test]
    fn e_deterministico_para_a_mesma_seed() {
        let p = example_pack();
        assert_eq!(
            generate_initial_contracts(&p, 7),
            generate_initial_contracts(&p, 7)
        );
    }

    #[test]
    fn seeds_diferentes_dao_contratos_diferentes() {
        let p = example_pack();
        assert_ne!(
            generate_initial_contracts(&p, 1),
            generate_initial_contracts(&p, 2)
        );
    }

    #[test]
    fn advance_contracts_decrementa_sem_renovar_quando_ainda_sobra_duracao() {
        let p = single_player_pack(100);
        let roster = initial_roster(&p);
        let mut contracts = vec![ContractState {
            seasons_remaining: 3,
            signed_ability: 100,
        }];
        let renewed = advance_contracts(&mut contracts, &roster, 1, 0);
        assert_eq!(contracts[0].seasons_remaining, 2);
        assert_eq!(contracts[0].signed_ability, 100); // não mudou, não renovou
        assert_eq!(renewed, 0);
    }

    #[test]
    fn advance_contracts_renova_e_trava_o_ca_atual_quando_chega_a_zero() {
        let p = single_player_pack(100);
        let mut roster = initial_roster(&p);
        roster[0].ability = domain::Ability::new(130, 200); // CA "cresceu" esta temporada
        let mut contracts = vec![ContractState {
            seasons_remaining: 1,
            signed_ability: 100, // salário antigo, travado no CA de antes
        }];
        let renewed = advance_contracts(&mut contracts, &roster, 1, 0);
        assert_eq!(renewed, 1);
        assert!(contracts[0].seasons_remaining >= MIN_SEASONS);
        assert!(contracts[0].seasons_remaining <= MAX_SEASONS);
        assert_eq!(
            contracts[0].signed_ability, 130,
            "renovação deveria travar o CA atual, não o antigo"
        );
    }

    #[test]
    fn advance_contracts_e_deterministico_para_a_mesma_entrada() {
        let p = single_player_pack(100);
        let roster = initial_roster(&p);
        let mut a = vec![ContractState {
            seasons_remaining: 1,
            signed_ability: 100,
        }];
        let mut b = a.clone();
        advance_contracts(&mut a, &roster, 42, 3);
        advance_contracts(&mut b, &roster, 42, 3);
        assert_eq!(a, b);
    }

    #[test]
    fn fresh_contract_e_sempre_duracao_maxima() {
        let c = fresh_contract(150);
        assert_eq!(c.seasons_remaining, MAX_SEASONS);
        assert_eq!(c.signed_ability, 150);
    }
}

/// Testes de propriedade (`docs/08 §1`).
#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        /// `advance_contracts` nunca produz uma duração fora de
        /// `MIN_SEASONS..=MAX_SEASONS` depois de uma renovação, pra
        /// qualquer `world_seed`/`season_index`.
        #[test]
        fn renovacao_sempre_dentro_dos_limites(
            ca: u8,
            world_seed: u64,
            season_index: u32,
        ) {
            use domain::{ClubId, CompetitionId, GameDate, NationId, PlayerId, Position};
            use pack::{ResolvedClub, ResolvedNation, ResolvedPlayer};

            let nation = ResolvedNation { id: NationId::new(0), external_id: "ex".to_string(), name: "Ex".to_string() };
            let club = ResolvedClub { id: ClubId::new(0), external_id: "ex.a".to_string(), name: "A".to_string(), nation: nation.id, competition: CompetitionId::new(0), founded: None, stadium: None };
            let player = ResolvedPlayer {
                id: PlayerId::new(0), external_id: "ex.a.p00".to_string(), first_name: "N".to_string(), last_name: "S".to_string(),
                birth: GameDate::from_ymd(2000, 1, 1), nation: nation.id, club: club.id, position: Position::Midfielder,
                attributes: domain::PlayerAttributes::default(), ability: domain::Ability::new(ca, 200),
            };
            let pack = LoadedPack { reference_year: 2026, nations: vec![nation], competitions: vec![], clubs: vec![club], players: vec![player] };
            let roster = crate::progression::initial_roster(&pack);

            let mut contracts = vec![ContractState { seasons_remaining: 1, signed_ability: ca }];
            advance_contracts(&mut contracts, &roster, world_seed, season_index);
            prop_assert!(contracts[0].seasons_remaining >= MIN_SEASONS);
            prop_assert!(contracts[0].seasons_remaining <= MAX_SEASONS);
        }
    }
}
