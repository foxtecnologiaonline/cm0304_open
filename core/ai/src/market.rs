//! Mercado de transferências automático — a "primeira IA de mercado" de
//! `docs/07-roadmap.md` M1, um subconjunto minúsculo de RF-TR-02 (avaliação
//! de valor de mercado) e RF-TR-05 (IA de transferência: identifica
//! carência, orça, compra) de `docs/01 §2.4` — ambos marcados M2 lá porque
//! a versão completa (negociação, disputa entre clubes, desistência,
//! reputação) pede muito mais do que existe: sem negociação de verdade
//! (`RF-TR-03`/`04`), sem demanda/reputação, sem janela por país
//! (`RF-TR-01`). O que há aqui é mecânico e automático, sem nenhum jogador
//! humano envolvido: cada clube, uma vez por "dia de mercado", tenta trocar
//! seu titular mais fraco por um reserva melhor de outro clube que caiba no
//! orçamento. [`market_value`] já considera **contrato** (duração restante
//! — `world::ContractState`) e **idade** — duas das outras variáveis de
//! RF-TR-02 completo (PA, reputação, demanda ainda faltam).
//!
//! Como [`crate::lineup`], depende só de `domain` — não sabe o que é um
//! `pack` ou uma `world::PlayerState`; quem chama (`world`) monta
//! [`MarketPlayer`] a partir do que tiver.

use domain::{ClubId, Money, PlayerId, Position};

use crate::lineup::{PlayerRating, select_starting_eleven};

/// Um jogador no mercado: o suficiente de [`PlayerRating`] mais o clube
/// atual — a IA de mercado precisa saber "de quem" para decidir "pra quem".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarketPlayer {
    pub player: PlayerId,
    pub club: ClubId,
    pub position: Position,
    pub current_ability: u8,
    /// Temporadas restantes do contrato atual (`world::ContractState`,
    /// nunca `0` lá) — entra em [`market_value`] como desconto: pouco
    /// contrato sobrando vale menos no mercado.
    pub contract_seasons_remaining: u8,
    /// Idade em anos completos (`world::PlayerState::age_years`, saturado
    /// em `u8` — nenhum pack real chega perto do teto de 255) — entra em
    /// [`market_value`] como prêmio (jovem, potencial de revenda) ou
    /// desconto (veterano, carreira mais curta pela frente).
    pub age_years: u8,
}

/// Uma transferência executada — o registro do que aconteceu, pra quem
/// chama atualizar o resto do estado (roster, notícias, etc.).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Transfer {
    pub player: PlayerId,
    pub from: ClubId,
    pub to: ClubId,
    pub fee: Money,
}

/// Desconto sobre o valor de base conforme a duração restante do contrato,
/// em milésimos — fatia de "contrato" de RF-TR-02 (`docs/01 §2.4`): um
/// jogador com pouco contrato sobrando vale menos (revenda mais arriscada,
/// clube vendedor com menos poder de negociação), mesmo sem agente livre
/// implementado (`RF-TR-07`, M3) — a lógica vale independente disso.
/// Faixas escolhidas só pra dar o efeito na direção certa, não calibradas.
/// Trata `0` (não deveria acontecer vindo de `world::ContractState`, ver
/// acima) como o mesmo piso de `1` — nenhum `panic!` com dado externo
/// (`docs/02 §9.2`).
fn contract_discount_permille(contract_seasons_remaining: u8) -> u32 {
    match contract_seasons_remaining {
        0 | 1 => 600,
        2 => 800,
        _ => 1000,
    }
}

/// Multiplicador sobre o valor de base conforme a idade, em milésimos —
/// fatia de "idade" de RF-TR-02 (`docs/01 §2.4`): jovem carrega prêmio
/// (potencial de revenda, carreira longa pela frente), veterano carrega
/// desconto (carreira curta pela frente, investimento de menor retorno).
/// Mesmas faixas de espírito de `world::progression::delta_ca_for_age`
/// (jovem/pico/declínio/veterano), mas números próprios — a curva de
/// evolução de CA e a curva de valor de revenda não precisam coincidir.
/// Não calibrado, só a direção certa.
fn age_value_permille(age_years: u8) -> u32 {
    match age_years {
        0..=23 => 1200,
        24..=29 => 1000,
        30..=33 => 700,
        _ => 400,
    }
}

/// Valor de mercado de um jogador — função de CA (quadrática: dobrar a
/// habilidade quadruplica o valor de base), de duração restante de
/// contrato ([`contract_discount_permille`]) e de idade
/// ([`age_value_permille`]) — o suficiente para um jogador de elite
/// custar ordens de magnitude mais que um da base, para contrato curto
/// baratear qualquer um, e para idade inclinar o preço a favor do jovem e
/// contra o veterano, sem precisar ainda de PA/reputação/demanda
/// (RF-TR-02 completo). Constante de escala (`10_000` centavos por CA²)
/// escolhida só pra dar números de magnitude plausível no pack de
/// exemplo; não é dado calibrado nem carregável de pack ainda (`docs/03
/// §5`, mesmo status que as constantes de `engine::simulate` antes da
/// calibração).
#[must_use]
pub fn market_value(current_ability: u8, contract_seasons_remaining: u8, age_years: u8) -> Money {
    const CENTS_PER_CA_SQUARED: i64 = 10_000;
    let ca = i64::from(current_ability);
    let base = ca * ca * CENTS_PER_CA_SQUARED;
    let contract_discount = i64::from(contract_discount_permille(contract_seasons_remaining));
    let age_factor = i64::from(age_value_permille(age_years));
    Money::from_cents(base * contract_discount * age_factor / 1_000_000)
}

/// O titular de menor habilidade atual do elenco `squad` (que já jogou,
/// via [`select_starting_eleven`]) — o candidato natural a ser substituído.
/// `None` se `squad` não tem nenhum titular (elenco vazio).
fn weakest_starter(squad: &[PlayerRating]) -> Option<PlayerRating> {
    let starters = select_starting_eleven(squad);
    starters
        .iter()
        .filter_map(|id| squad.iter().find(|p| p.player == *id).copied())
        .min_by(|a, b| {
            a.current_ability
                .cmp(&b.current_ability)
                .then(a.player.cmp(&b.player))
        })
}

/// Roda um dia de mercado: cada clube, em ordem crescente de `ClubId`
/// (`0..budgets.len()`), tenta **uma** transferência — trocar seu titular
/// mais fraco pelo reserva de maior habilidade de outro clube que (a) joga
/// na mesma posição, (b) é melhor que o titular a substituir, (c) cabe no
/// orçamento restante do comprador. Sem essas três condições, o clube não
/// faz nada nesse dia. Determinístico: empates de habilidade são
/// desempatados por `PlayerId` crescente, sem RNG nenhum envolvido — a
/// mesma entrada sempre produz a mesma sequência de transferências.
///
/// Só jogadores que **não titularizam pelo próprio clube** são
/// candidatos a sair — vender o time titular inteiro não faz sentido pra
/// uma primeira versão da IA (`RF-TR-05` completo disputaria/desistiria de
/// propostas; aqui não há oferta recusável, o "dono" sempre vende se o
/// preço bate). Quem sai muda de clube em `players` (mutado in-place);
/// `budgets` é debitado do comprador e creditado no vendedor pelo mesmo
/// valor — uma economia fechada, sem receita externa (bilheteria, patrocínio
/// — nenhuma existe ainda).
#[must_use]
pub fn run_market_day(players: &mut [MarketPlayer], budgets: &mut [Money]) -> Vec<Transfer> {
    // Quem titulariza pelo próprio clube, calculado uma vez antes de
    // qualquer transferência: só se vende gente do banco, e vender um
    // jogador do banco nunca muda quem são os titulares de quem vendeu —
    // por isso não precisa recalcular a cada transferência executada.
    let mut sellable: Vec<bool> = vec![true; players.len()];
    for club_index in 0..budgets.len() {
        let club = ClubId::new(club_index as u32);
        let squad: Vec<PlayerRating> = players
            .iter()
            .filter(|p| p.club == club)
            .map(to_rating)
            .collect();
        for starter in select_starting_eleven(&squad) {
            if let Some(idx) = players.iter().position(|p| p.player == starter) {
                sellable[idx] = false;
            }
        }
    }

    let mut transfers = Vec::new();
    for buyer_index in 0..budgets.len() {
        let buyer = ClubId::new(buyer_index as u32);
        let buyer_squad: Vec<PlayerRating> = players
            .iter()
            .filter(|p| p.club == buyer)
            .map(to_rating)
            .collect();
        let Some(weak_link) = weakest_starter(&buyer_squad) else {
            continue;
        };

        let mut candidates: Vec<usize> = players
            .iter()
            .enumerate()
            .filter(|(idx, p)| {
                sellable[*idx]
                    && p.club != buyer
                    && p.position == weak_link.position
                    && p.current_ability > weak_link.current_ability
            })
            .map(|(idx, _)| idx)
            .collect();
        candidates.sort_by(|&a, &b| {
            players[b]
                .current_ability
                .cmp(&players[a].current_ability)
                .then(players[a].player.cmp(&players[b].player))
        });

        let Some(&winner_idx) = candidates.iter().find(|&&idx| {
            budgets[buyer_index]
                >= market_value(
                    players[idx].current_ability,
                    players[idx].contract_seasons_remaining,
                    players[idx].age_years,
                )
        }) else {
            continue;
        };

        let fee = market_value(
            players[winner_idx].current_ability,
            players[winner_idx].contract_seasons_remaining,
            players[winner_idx].age_years,
        );
        let seller = players[winner_idx].club;
        let player_id = players[winner_idx].player;

        budgets[buyer_index] = budgets[buyer_index] - fee;
        budgets[seller.as_usize()] = budgets[seller.as_usize()] + fee;
        players[winner_idx].club = buyer;
        sellable[winner_idx] = false; // acabou de ser comprado, não revende no mesmo dia

        transfers.push(Transfer {
            player: player_id,
            from: seller,
            to: buyer,
            fee,
        });
    }
    transfers
}

fn to_rating(p: &MarketPlayer) -> PlayerRating {
    PlayerRating {
        player: p.player,
        position: p.position,
        current_ability: p.current_ability,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Contrato "cheio" e idade "de pico" por padrão (preço cheio, sem
    // desconto/prêmio) — testes que não são sobre contrato/idade não
    // precisam pensar nisso.
    const FULL_CONTRACT: u8 = 5;
    const PRIME_AGE: u8 = 26;

    fn player(id: u32, club: u32, position: Position, ca: u8) -> MarketPlayer {
        player_with(id, club, position, ca, FULL_CONTRACT, PRIME_AGE)
    }

    fn player_with_contract(
        id: u32,
        club: u32,
        position: Position,
        ca: u8,
        contract_seasons_remaining: u8,
    ) -> MarketPlayer {
        player_with(
            id,
            club,
            position,
            ca,
            contract_seasons_remaining,
            PRIME_AGE,
        )
    }

    fn player_with(
        id: u32,
        club: u32,
        position: Position,
        ca: u8,
        contract_seasons_remaining: u8,
        age_years: u8,
    ) -> MarketPlayer {
        MarketPlayer {
            player: PlayerId::new(id),
            club: ClubId::new(club),
            position,
            current_ability: ca,
            contract_seasons_remaining,
            age_years,
        }
    }

    #[test]
    fn market_value_cresce_quadraticamente_com_ca() {
        assert_eq!(market_value(0, FULL_CONTRACT, PRIME_AGE), Money::ZERO);
        assert!(
            market_value(200, FULL_CONTRACT, PRIME_AGE)
                > market_value(100, FULL_CONTRACT, PRIME_AGE)
        );
        // Dobrar o CA (10 -> 20) quadruplica o valor.
        assert_eq!(
            market_value(20, FULL_CONTRACT, PRIME_AGE).cents(),
            market_value(10, FULL_CONTRACT, PRIME_AGE).cents() * 4
        );
    }

    #[test]
    fn market_value_cai_com_contrato_mais_curto() {
        assert!(market_value(100, 5, PRIME_AGE) > market_value(100, 2, PRIME_AGE));
        assert!(market_value(100, 2, PRIME_AGE) > market_value(100, 1, PRIME_AGE));
        // `0` (não deveria vir de `world::ContractState`) trata como `1`.
        assert_eq!(
            market_value(100, 0, PRIME_AGE),
            market_value(100, 1, PRIME_AGE)
        );
    }

    #[test]
    fn market_value_favorece_jovem_e_penaliza_veterano() {
        let young = market_value(100, FULL_CONTRACT, 20);
        let prime = market_value(100, FULL_CONTRACT, PRIME_AGE);
        let declining = market_value(100, FULL_CONTRACT, 32);
        let veteran = market_value(100, FULL_CONTRACT, 38);
        assert!(young > prime);
        assert!(prime > declining);
        assert!(declining > veteran);
    }

    #[test]
    fn clube_sem_orcamento_nao_compra_nada() {
        let mut players = vec![
            player(0, 0, Position::Goalkeeper, 40), // titular fraco do clube 0
            player(1, 1, Position::Goalkeeper, 90), // titular do clube 1
            player(2, 1, Position::Goalkeeper, 70), // reserva do clube 1 — candidato válido, se houvesse dinheiro
        ];
        let mut budgets = vec![Money::ZERO, Money::ZERO];
        let transfers = run_market_day(&mut players, &mut budgets);
        assert!(transfers.is_empty());
        assert_eq!(players[2].club, ClubId::new(1)); // ninguém mudou de clube
    }

    #[test]
    fn clube_compra_upgrade_de_verdade_do_banco_de_outro_clube() {
        // Clube 0: um único goleiro fraco (titular obrigatório, único da
        // posição -> sempre titular, mesmo sendo ruim).
        // Clube 1: dois goleiros — um titular mediano e um reserva muito
        // bom (CA maior que o do clube 0, mas pior que o titular do clube
        // 1, então fica no banco).
        let mut players = vec![
            player(0, 0, Position::Goalkeeper, 40), // titular fraco do clube 0
            player(1, 1, Position::Goalkeeper, 90), // titular do clube 1
            player(2, 1, Position::Goalkeeper, 70), // reserva do clube 1 (banco: só 1 vaga de GK)
        ];
        let mut budgets = vec![Money::from_cents(1_000_000_000), Money::ZERO];

        let transfers = run_market_day(&mut players, &mut budgets);

        assert_eq!(transfers.len(), 1);
        let t = transfers[0];
        assert_eq!(t.player, PlayerId::new(2));
        assert_eq!(t.from, ClubId::new(1));
        assert_eq!(t.to, ClubId::new(0));
        assert_eq!(t.fee, market_value(70, FULL_CONTRACT, PRIME_AGE));
        assert_eq!(players[2].club, ClubId::new(0));
        // Orçamento debitado do comprador, creditado no vendedor.
        assert_eq!(
            budgets[0],
            Money::from_cents(1_000_000_000) - market_value(70, FULL_CONTRACT, PRIME_AGE)
        );
        assert_eq!(budgets[1], market_value(70, FULL_CONTRACT, PRIME_AGE));
    }

    #[test]
    fn contrato_curto_barateia_e_pode_viabilizar_a_compra() {
        // Mesmo cenário de `clube_compra_upgrade_de_verdade_do_banco_de_outro_clube`,
        // mas com orçamento entre o preço cheio e o preço com desconto de
        // contrato curto — só vira transferência quando o reserva tem
        // pouco contrato restante.
        let budget = Money::from_cents(35_000_000); // entre 29.4M (desconto) e 49M (cheio)

        let mut full_contract = vec![
            player(0, 0, Position::Goalkeeper, 40),
            player(1, 1, Position::Goalkeeper, 90),
            player_with_contract(2, 1, Position::Goalkeeper, 70, FULL_CONTRACT),
        ];
        let mut budgets_full = vec![budget, Money::ZERO];
        assert!(
            run_market_day(&mut full_contract, &mut budgets_full).is_empty(),
            "contrato cheio não deveria caber no orçamento"
        );

        let mut short_contract = vec![
            player(0, 0, Position::Goalkeeper, 40),
            player(1, 1, Position::Goalkeeper, 90),
            player_with_contract(2, 1, Position::Goalkeeper, 70, 1),
        ];
        let mut budgets_short = vec![budget, Money::ZERO];
        let transfers = run_market_day(&mut short_contract, &mut budgets_short);
        assert_eq!(
            transfers.len(),
            1,
            "contrato de 1 temporada deveria baratear o suficiente pra caber no orçamento"
        );
    }

    #[test]
    fn nao_vende_titular_do_proprio_clube_mesmo_sendo_pior_globalmente() {
        // O único goleiro do clube 1 é titular por definição (não tem
        // banco) — mesmo sendo pior que o do clube 0, não pode ser vendido
        // (não há ninguém "no banco" pra vender).
        let mut players = vec![
            player(0, 0, Position::Goalkeeper, 100),
            player(1, 1, Position::Goalkeeper, 30),
        ];
        let mut budgets = vec![Money::from_cents(1_000_000_000), Money::ZERO];
        let transfers = run_market_day(&mut players, &mut budgets);
        assert!(transfers.is_empty());
    }

    #[test]
    fn e_deterministico_para_a_mesma_entrada() {
        let base = vec![
            player(0, 0, Position::Goalkeeper, 40),
            player(1, 1, Position::Goalkeeper, 90),
            player(2, 1, Position::Goalkeeper, 70),
        ];
        let mut a = base.clone();
        let mut budgets_a = vec![Money::from_cents(1_000_000_000), Money::ZERO];
        let ta = run_market_day(&mut a, &mut budgets_a);

        let mut b = base;
        let mut budgets_b = vec![Money::from_cents(1_000_000_000), Money::ZERO];
        let tb = run_market_day(&mut b, &mut budgets_b);

        assert_eq!(ta, tb);
        assert_eq!(a, b);
        assert_eq!(budgets_a, budgets_b);
    }
}

/// Testes de propriedade (`docs/08 §1`).
#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    fn arbitrary_position() -> impl Strategy<Value = Position> {
        prop_oneof![
            Just(Position::Goalkeeper),
            Just(Position::Defender),
            Just(Position::Midfielder),
            Just(Position::Forward),
        ]
    }

    /// `n_clubs` clubes, cada um com um elenco de tamanho e composição
    /// arbitrários (ids de jogador únicos globalmente, garantidos pela
    /// numeração sequencial abaixo — não por sorteio, então não precisa de
    /// um `hash_set` como em `lineup::proptests`).
    fn arbitrary_league() -> impl Strategy<Value = (Vec<MarketPlayer>, Vec<Money>)> {
        (2usize..6).prop_flat_map(|n_clubs| {
            let squads = prop::collection::vec(
                prop::collection::vec(
                    (arbitrary_position(), any::<u8>(), any::<u8>(), any::<u8>()),
                    0..10,
                ),
                n_clubs,
            );
            let budgets = prop::collection::vec(0i64..1_000_000_000, n_clubs);
            (squads, budgets).prop_map(|(squads, budget_cents)| {
                let mut next_id = 0u32;
                let mut players = Vec::new();
                for (club_index, squad) in squads.into_iter().enumerate() {
                    for (position, ca, contract_seasons_remaining, age_years) in squad {
                        players.push(MarketPlayer {
                            player: PlayerId::new(next_id),
                            club: ClubId::new(club_index as u32),
                            position,
                            current_ability: ca,
                            contract_seasons_remaining,
                            age_years,
                        });
                        next_id += 1;
                    }
                }
                let budgets = budget_cents.into_iter().map(Money::from_cents).collect();
                (players, budgets)
            })
        })
    }

    proptest! {
        /// `run_market_day` nunca panica e nunca cria ou destrói dinheiro —
        /// a soma dos orçamentos antes e depois é sempre a mesma, porque
        /// toda transferência move exatamente `fee` do comprador pro
        /// vendedor, nunca menos nem mais (economia fechada, ver o doc do
        /// módulo).
        #[test]
        fn dinheiro_total_e_conservado((mut players, mut budgets) in arbitrary_league()) {
            let total_before: i64 = budgets.iter().map(|b| b.cents()).sum();
            let _ = run_market_day(&mut players, &mut budgets);
            let total_after: i64 = budgets.iter().map(|b| b.cents()).sum();
            prop_assert_eq!(total_before, total_after);
        }

        /// Puro e determinístico: a mesma liga sempre produz a mesma
        /// sequência de transferências e o mesmo estado final.
        #[test]
        fn e_deterministico_para_qualquer_liga((players, budgets) in arbitrary_league()) {
            let mut a = players.clone();
            let mut budgets_a = budgets.clone();
            let ta = run_market_day(&mut a, &mut budgets_a);

            let mut b = players;
            let mut budgets_b = budgets;
            let tb = run_market_day(&mut b, &mut budgets_b);

            prop_assert_eq!(ta, tb);
            prop_assert_eq!(a, b);
            prop_assert_eq!(budgets_a, budgets_b);
        }
    }
}
