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
//! — `world::ContractState`), **idade** e **PA** (potencial não realizado)
//! — três das outras variáveis de RF-TR-02 completo (demanda ainda falta;
//! reputação entra parcialmente via [`MarketPlayer::ambition`], abaixo).
//! [`run_market_day`] também já considera **lealdade** (RF-JG-02, atributo
//! oculto): jogador muito leal nunca é candidato a saída, mesmo que o
//! preço bata. E **ambição** (RF-JG-02, atributo oculto): jogador muito
//! ambicioso recusa ir para um clube de reputação menor que a do seu
//! clube atual, mesmo que o preço e a lealdade permitam — o inverso não
//! tem restrição (ambicioso aceita ir pra um clube igual ou mais
//! reputado, e pouco ambicioso aceita qualquer destino, igual a antes
//! desta fatia).
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
    /// PA — teto de evolução de `domain::Ability::potential()` — entra em
    /// [`market_value`] como prêmio sobre a margem ainda não realizada
    /// (PA − CA): muito espaço pra crescer vale mais, mesmo com CA baixo
    /// hoje (especulação sobre o futuro), complementando o prêmio de
    /// idade (jovem sem margem de crescimento não ganha nada extra aqui).
    pub potential_ability: u8,
    /// Lealdade ao clube atual, 0-100 — acima de
    /// [`LOYALTY_REFUSAL_THRESHOLD`], o jogador nunca é candidato a saída
    /// em [`run_market_day`], mesmo que o preço bata (RF-JG-02, atributo
    /// oculto, `docs/01 §2.3`).
    pub loyalty: u8,
    /// Ambição, 0-100 — acima de [`AMBITION_REFUSAL_THRESHOLD`], o
    /// jogador recusa ir pra um clube de reputação menor que a do seu
    /// clube atual em [`run_market_day`] (RF-JG-02, atributo oculto,
    /// `docs/01 §2.3`); abaixo do limiar, aceita qualquer destino que já
    /// batesse preço e lealdade.
    pub ambition: u8,
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

/// Prêmio sobre o valor de base conforme a margem de evolução ainda não
/// realizada (PA − CA, nunca negativa — `domain::Ability` já garante
/// `current <= potential`), em milésimos — fatia de "PA" de RF-TR-02
/// (`docs/01 §2.4`): quanto maior a margem, maior o prêmio especulativo.
/// Faixas escolhidas só pra dar o efeito na direção certa, não calibradas.
fn potential_premium_permille(current_ability: u8, potential_ability: u8) -> u32 {
    let gap = potential_ability.saturating_sub(current_ability);
    match gap {
        0..=5 => 1000,
        6..=20 => 1100,
        21..=50 => 1300,
        _ => 1600,
    }
}

/// Valor de mercado de um jogador — função de CA (quadrática: dobrar a
/// habilidade quadruplica o valor de base), de duração restante de
/// contrato ([`contract_discount_permille`]), de idade
/// ([`age_value_permille`]) e de margem de PA ainda não realizada
/// ([`potential_premium_permille`]) — o suficiente para um jogador de
/// elite custar ordens de magnitude mais que um da base, para contrato
/// curto baratear qualquer um, para idade inclinar o preço a favor do
/// jovem e contra o veterano, e para potencial não realizado encarecer um
/// jogador que ainda vai crescer muito — sem precisar ainda de
/// reputação/demanda (RF-TR-02 completo). Constante de escala (`10_000`
/// centavos por CA²) escolhida só pra dar números de magnitude plausível
/// no pack de exemplo; não é dado calibrado nem carregável de pack ainda
/// (`docs/03 §5`, mesmo status que as constantes de `engine::simulate`
/// antes da calibração).
#[must_use]
pub fn market_value(
    current_ability: u8,
    contract_seasons_remaining: u8,
    age_years: u8,
    potential_ability: u8,
) -> Money {
    const CENTS_PER_CA_SQUARED: i64 = 10_000;
    let ca = i64::from(current_ability);
    let base = ca * ca * CENTS_PER_CA_SQUARED;
    let contract_discount = i64::from(contract_discount_permille(contract_seasons_remaining));
    let age_factor = i64::from(age_value_permille(age_years));
    let potential_premium = i64::from(potential_premium_permille(
        current_ability,
        potential_ability,
    ));
    Money::from_cents(base * contract_discount * age_factor * potential_premium / 1_000_000_000)
}

/// Lealdade a partir da qual um jogador nunca é candidato a saída em
/// [`run_market_day`], mesmo que o preço bata e ele esteja no banco —
/// fatia de "lealdade" de RF-JG-02 (`docs/01 §2.3`, atributo oculto): um
/// jogador muito leal ao clube recusa qualquer proposta. Sem negociação
/// de verdade ainda (`RF-TR-03`/`04`, M2/M3): não é "quase aceita por
/// mais dinheiro", é um limiar fixo — abaixo dele, aceita sempre que o
/// preço bate, igual a antes desta fatia. Não calibrado, só a direção
/// certa.
const LOYALTY_REFUSAL_THRESHOLD: u8 = 80;

/// Ambição a partir da qual um jogador recusa ir pra um clube de
/// reputação menor que a do seu clube atual em [`run_market_day`] — fatia
/// de "ambição" de RF-JG-02 (`docs/01 §2.3`, atributo oculto): um jogador
/// muito ambicioso não aceita um passo atrás de carreira, mesmo com preço
/// e lealdade batendo. Abaixo do limiar, aceita qualquer destino, igual a
/// antes desta fatia. Não calibrado, só a direção certa.
const AMBITION_REFUSAL_THRESHOLD: u8 = 70;

/// `true` se um jogador com `ambition` aceita se mudar de um clube de
/// reputação `seller_reputation` para um de reputação `buyer_reputation`
/// — só recusa quando é muito ambicioso **e** o destino é de reputação
/// estritamente menor; em qualquer outro caso (pouco ambicioso, ou
/// destino igual/melhor), aceita.
fn accepts_destination(ambition: u8, seller_reputation: u8, buyer_reputation: u8) -> bool {
    ambition < AMBITION_REFUSAL_THRESHOLD || buyer_reputation >= seller_reputation
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
/// Só jogadores que **não titularizam pelo próprio clube**, cuja
/// lealdade está abaixo de [`LOYALTY_REFUSAL_THRESHOLD`] e que (via
/// [`accepts_destination`]) aceitam o clube comprador são candidatos a
/// sair — vender o time titular inteiro não faz sentido pra uma primeira
/// versão da IA, um jogador muito leal recusa qualquer proposta, e um
/// jogador muito ambicioso recusa um passo atrás de reputação
/// (`RF-TR-05` completo disputaria/desistiria de propostas de verdade;
/// aqui ainda não há negociação, só esses filtros fixos — o "dono" vende
/// se o preço bate e lealdade/ambição permitem). Quem sai muda de clube
/// em `players` (mutado in-place); `budgets` é debitado do comprador e
/// creditado no vendedor pelo mesmo valor — uma economia fechada, sem
/// receita externa (bilheteria, patrocínio — nenhuma existe ainda).
/// `club_reputation[club.as_usize()]` dá a reputação de cada clube
/// (`world::generate_club_reputation`, mesmo índice denso de `budgets`) —
/// só usada pela recusa de ambição, em nada mais.
#[must_use]
pub fn run_market_day(
    players: &mut [MarketPlayer],
    budgets: &mut [Money],
    club_reputation: &[u8],
) -> Vec<Transfer> {
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
                    && p.loyalty < LOYALTY_REFUSAL_THRESHOLD
                    && accepts_destination(
                        p.ambition,
                        club_reputation[p.club.as_usize()],
                        club_reputation[buyer_index],
                    )
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
                    players[idx].potential_ability,
                )
        }) else {
            continue;
        };

        let fee = market_value(
            players[winner_idx].current_ability,
            players[winner_idx].contract_seasons_remaining,
            players[winner_idx].age_years,
            players[winner_idx].potential_ability,
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

    // Lealdade "baixa" por padrão (sempre vendável, abaixo do limiar de
    // recusa) — testes que não são sobre lealdade não precisam pensar
    // nisso.
    const LOW_LOYALTY: u8 = 0;

    // Ambição "baixa" por padrão (aceita qualquer destino, abaixo do
    // limiar de recusa) — testes que não são sobre ambição não precisam
    // pensar nisso.
    const LOW_AMBITION: u8 = 0;

    /// Reputação uniforme pra todo clube — zera o efeito de
    /// `accepts_destination` (nenhum destino é "pior" que outro), pros
    /// testes que não são sobre ambição em si.
    fn uniform_reputation(n_clubs: usize) -> Vec<u8> {
        vec![50; n_clubs]
    }

    fn player(id: u32, club: u32, position: Position, ca: u8) -> MarketPlayer {
        // Potencial igual ao CA (margem zero) — mesmo preço cheio dos
        // outros padrões, pra testes que não são sobre PA.
        player_with(
            id,
            club,
            position,
            ca,
            FULL_CONTRACT,
            PRIME_AGE,
            ca,
            LOW_LOYALTY,
            LOW_AMBITION,
        )
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
            ca,
            LOW_LOYALTY,
            LOW_AMBITION,
        )
    }

    fn player_with_loyalty(
        id: u32,
        club: u32,
        position: Position,
        ca: u8,
        loyalty: u8,
    ) -> MarketPlayer {
        player_with(
            id,
            club,
            position,
            ca,
            FULL_CONTRACT,
            PRIME_AGE,
            ca,
            loyalty,
            LOW_AMBITION,
        )
    }

    fn player_with_ambition(
        id: u32,
        club: u32,
        position: Position,
        ca: u8,
        ambition: u8,
    ) -> MarketPlayer {
        player_with(
            id,
            club,
            position,
            ca,
            FULL_CONTRACT,
            PRIME_AGE,
            ca,
            LOW_LOYALTY,
            ambition,
        )
    }

    #[allow(clippy::too_many_arguments)]
    fn player_with(
        id: u32,
        club: u32,
        position: Position,
        ca: u8,
        contract_seasons_remaining: u8,
        age_years: u8,
        potential_ability: u8,
        loyalty: u8,
        ambition: u8,
    ) -> MarketPlayer {
        MarketPlayer {
            player: PlayerId::new(id),
            club: ClubId::new(club),
            position,
            current_ability: ca,
            contract_seasons_remaining,
            age_years,
            potential_ability,
            loyalty,
            ambition,
        }
    }

    #[test]
    fn market_value_cresce_quadraticamente_com_ca() {
        assert_eq!(market_value(0, FULL_CONTRACT, PRIME_AGE, 0), Money::ZERO);
        assert!(
            market_value(200, FULL_CONTRACT, PRIME_AGE, 200)
                > market_value(100, FULL_CONTRACT, PRIME_AGE, 100)
        );
        // Dobrar o CA (10 -> 20) quadruplica o valor.
        assert_eq!(
            market_value(20, FULL_CONTRACT, PRIME_AGE, 20).cents(),
            market_value(10, FULL_CONTRACT, PRIME_AGE, 10).cents() * 4
        );
    }

    #[test]
    fn market_value_cai_com_contrato_mais_curto() {
        assert!(market_value(100, 5, PRIME_AGE, 100) > market_value(100, 2, PRIME_AGE, 100));
        assert!(market_value(100, 2, PRIME_AGE, 100) > market_value(100, 1, PRIME_AGE, 100));
        // `0` (não deveria vir de `world::ContractState`) trata como `1`.
        assert_eq!(
            market_value(100, 0, PRIME_AGE, 100),
            market_value(100, 1, PRIME_AGE, 100)
        );
    }

    #[test]
    fn market_value_favorece_jovem_e_penaliza_veterano() {
        let young = market_value(100, FULL_CONTRACT, 20, 100);
        let prime = market_value(100, FULL_CONTRACT, PRIME_AGE, 100);
        let declining = market_value(100, FULL_CONTRACT, 32, 100);
        let veteran = market_value(100, FULL_CONTRACT, 38, 100);
        assert!(young > prime);
        assert!(prime > declining);
        assert!(declining > veteran);
    }

    #[test]
    fn market_value_cresce_com_a_margem_de_potencial_nao_realizado() {
        let no_gap = market_value(100, FULL_CONTRACT, PRIME_AGE, 100);
        let small_gap = market_value(100, FULL_CONTRACT, PRIME_AGE, 110);
        let big_gap = market_value(100, FULL_CONTRACT, PRIME_AGE, 160);
        assert!(small_gap > no_gap);
        assert!(big_gap > small_gap);
    }

    #[test]
    fn clube_sem_orcamento_nao_compra_nada() {
        let mut players = vec![
            player(0, 0, Position::Goalkeeper, 40), // titular fraco do clube 0
            player(1, 1, Position::Goalkeeper, 90), // titular do clube 1
            player(2, 1, Position::Goalkeeper, 70), // reserva do clube 1 — candidato válido, se houvesse dinheiro
        ];
        let mut budgets = vec![Money::ZERO, Money::ZERO];
        let transfers = run_market_day(&mut players, &mut budgets, &uniform_reputation(2));
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

        let transfers = run_market_day(&mut players, &mut budgets, &uniform_reputation(2));

        assert_eq!(transfers.len(), 1);
        let t = transfers[0];
        assert_eq!(t.player, PlayerId::new(2));
        assert_eq!(t.from, ClubId::new(1));
        assert_eq!(t.to, ClubId::new(0));
        assert_eq!(t.fee, market_value(70, FULL_CONTRACT, PRIME_AGE, 70));
        assert_eq!(players[2].club, ClubId::new(0));
        // Orçamento debitado do comprador, creditado no vendedor.
        assert_eq!(
            budgets[0],
            Money::from_cents(1_000_000_000) - market_value(70, FULL_CONTRACT, PRIME_AGE, 70)
        );
        assert_eq!(budgets[1], market_value(70, FULL_CONTRACT, PRIME_AGE, 70));
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
            run_market_day(
                &mut full_contract,
                &mut budgets_full,
                &uniform_reputation(2)
            )
            .is_empty(),
            "contrato cheio não deveria caber no orçamento"
        );

        let mut short_contract = vec![
            player(0, 0, Position::Goalkeeper, 40),
            player(1, 1, Position::Goalkeeper, 90),
            player_with_contract(2, 1, Position::Goalkeeper, 70, 1),
        ];
        let mut budgets_short = vec![budget, Money::ZERO];
        let transfers = run_market_day(
            &mut short_contract,
            &mut budgets_short,
            &uniform_reputation(2),
        );
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
        let transfers = run_market_day(&mut players, &mut budgets, &uniform_reputation(2));
        assert!(transfers.is_empty());
    }

    #[test]
    fn jogador_muito_leal_recusa_a_saida_mesmo_cabendo_no_orcamento() {
        // Mesmo cenário de `clube_compra_upgrade_de_verdade_do_banco_de_outro_clube`
        // (reserva do clube B seria vendido pro clube A), mas agora com
        // lealdade acima do limiar de recusa — a transferência não deveria
        // acontecer, mesmo com preço e orçamento batendo igual antes.
        let mut players = vec![
            player(0, 0, Position::Goalkeeper, 40),
            player(1, 1, Position::Goalkeeper, 90),
            player_with_loyalty(2, 1, Position::Goalkeeper, 70, 100),
        ];
        let mut budgets = vec![Money::from_cents(1_000_000_000), Money::ZERO];
        let transfers = run_market_day(&mut players, &mut budgets, &uniform_reputation(2));
        assert!(
            transfers.is_empty(),
            "jogador com lealdade máxima não deveria aceitar sair"
        );
        assert_eq!(players[2].club, ClubId::new(1));
    }

    #[test]
    fn lealdade_abaixo_do_limiar_ainda_aceita_normalmente() {
        // Mesmo cenário, lealdade só um pouco abaixo do limiar de recusa —
        // a transferência tem que continuar acontecendo normalmente.
        let mut players = vec![
            player(0, 0, Position::Goalkeeper, 40),
            player(1, 1, Position::Goalkeeper, 90),
            player_with_loyalty(
                2,
                1,
                Position::Goalkeeper,
                70,
                LOYALTY_REFUSAL_THRESHOLD - 1,
            ),
        ];
        let mut budgets = vec![Money::from_cents(1_000_000_000), Money::ZERO];
        let transfers = run_market_day(&mut players, &mut budgets, &uniform_reputation(2));
        assert_eq!(transfers.len(), 1);
        assert_eq!(players[2].club, ClubId::new(0));
    }

    #[test]
    fn jogador_muito_ambicioso_recusa_ir_pra_clube_de_reputacao_menor() {
        // Mesmo cenário de `clube_compra_upgrade_de_verdade_do_banco_de_outro_clube`
        // (reserva do clube 1 seria vendido pro clube 0), mas agora com
        // ambição acima do limiar de recusa e reputação do clube 0 (o
        // comprador) menor que a do clube 1 (o vendedor atual) — a
        // transferência não deveria acontecer.
        let mut players = vec![
            player(0, 0, Position::Goalkeeper, 40),
            player(1, 1, Position::Goalkeeper, 90),
            player_with_ambition(2, 1, Position::Goalkeeper, 70, 100),
        ];
        let mut budgets = vec![Money::from_cents(1_000_000_000), Money::ZERO];
        let club_reputation = vec![20u8, 80u8]; // clube 0 (comprador) bem menos reputado
        let transfers = run_market_day(&mut players, &mut budgets, &club_reputation);
        assert!(
            transfers.is_empty(),
            "jogador muito ambicioso não deveria aceitar um passo atrás de reputação"
        );
        assert_eq!(players[2].club, ClubId::new(1));
    }

    #[test]
    fn jogador_muito_ambicioso_aceita_clube_de_reputacao_igual_ou_maior() {
        // Mesmo cenário, mas o comprador (clube 0) tem reputação igual ou
        // maior que o vendedor (clube 1) — ambição não deveria impedir
        // nada aqui, mesmo acima do limiar de recusa.
        let mut players = vec![
            player(0, 0, Position::Goalkeeper, 40),
            player(1, 1, Position::Goalkeeper, 90),
            player_with_ambition(2, 1, Position::Goalkeeper, 70, 100),
        ];
        let mut budgets = vec![Money::from_cents(1_000_000_000), Money::ZERO];
        let club_reputation = vec![80u8, 80u8]; // mesma reputação
        let transfers = run_market_day(&mut players, &mut budgets, &club_reputation);
        assert_eq!(transfers.len(), 1);
        assert_eq!(players[2].club, ClubId::new(0));
    }

    #[test]
    fn ambicao_abaixo_do_limiar_aceita_qualquer_destino() {
        // Mesmo cenário de recusa (comprador bem menos reputado), mas com
        // ambição só um pouco abaixo do limiar de recusa — a
        // transferência tem que continuar acontecendo normalmente.
        let mut players = vec![
            player(0, 0, Position::Goalkeeper, 40),
            player(1, 1, Position::Goalkeeper, 90),
            player_with_ambition(
                2,
                1,
                Position::Goalkeeper,
                70,
                AMBITION_REFUSAL_THRESHOLD - 1,
            ),
        ];
        let mut budgets = vec![Money::from_cents(1_000_000_000), Money::ZERO];
        let club_reputation = vec![20u8, 80u8];
        let transfers = run_market_day(&mut players, &mut budgets, &club_reputation);
        assert_eq!(transfers.len(), 1);
        assert_eq!(players[2].club, ClubId::new(0));
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
        let ta = run_market_day(&mut a, &mut budgets_a, &uniform_reputation(2));

        let mut b = base;
        let mut budgets_b = vec![Money::from_cents(1_000_000_000), Money::ZERO];
        let tb = run_market_day(&mut b, &mut budgets_b, &uniform_reputation(2));

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
    /// um `hash_set` como em `lineup::proptests`), mais uma reputação
    /// arbitrária por clube.
    fn arbitrary_league() -> impl Strategy<Value = (Vec<MarketPlayer>, Vec<Money>, Vec<u8>)> {
        (2usize..6).prop_flat_map(|n_clubs| {
            let squads = prop::collection::vec(
                prop::collection::vec(
                    (
                        arbitrary_position(),
                        any::<u8>(),
                        any::<u8>(),
                        any::<u8>(),
                        any::<u8>(),
                        any::<u8>(),
                        any::<u8>(),
                    ),
                    0..10,
                ),
                n_clubs,
            );
            let budgets = prop::collection::vec(0i64..1_000_000_000, n_clubs);
            let club_reputation = prop::collection::vec(any::<u8>(), n_clubs);
            (squads, budgets, club_reputation).prop_map(
                |(squads, budget_cents, club_reputation)| {
                    let mut next_id = 0u32;
                    let mut players = Vec::new();
                    for (club_index, squad) in squads.into_iter().enumerate() {
                        for (
                            position,
                            ca,
                            contract_seasons_remaining,
                            age_years,
                            potential_ability,
                            loyalty,
                            ambition,
                        ) in squad
                        {
                            players.push(MarketPlayer {
                                player: PlayerId::new(next_id),
                                club: ClubId::new(club_index as u32),
                                position,
                                current_ability: ca,
                                contract_seasons_remaining,
                                age_years,
                                potential_ability,
                                loyalty,
                                ambition,
                            });
                            next_id += 1;
                        }
                    }
                    let budgets = budget_cents.into_iter().map(Money::from_cents).collect();
                    (players, budgets, club_reputation)
                },
            )
        })
    }

    proptest! {
        /// `run_market_day` nunca panica e nunca cria ou destrói dinheiro —
        /// a soma dos orçamentos antes e depois é sempre a mesma, porque
        /// toda transferência move exatamente `fee` do comprador pro
        /// vendedor, nunca menos nem mais (economia fechada, ver o doc do
        /// módulo).
        #[test]
        fn dinheiro_total_e_conservado((mut players, mut budgets, club_reputation) in arbitrary_league()) {
            let total_before: i64 = budgets.iter().map(|b| b.cents()).sum();
            let _ = run_market_day(&mut players, &mut budgets, &club_reputation);
            let total_after: i64 = budgets.iter().map(|b| b.cents()).sum();
            prop_assert_eq!(total_before, total_after);
        }

        /// Puro e determinístico: a mesma liga sempre produz a mesma
        /// sequência de transferências e o mesmo estado final.
        #[test]
        fn e_deterministico_para_qualquer_liga((players, budgets, club_reputation) in arbitrary_league()) {
            let mut a = players.clone();
            let mut budgets_a = budgets.clone();
            let ta = run_market_day(&mut a, &mut budgets_a, &club_reputation);

            let mut b = players;
            let mut budgets_b = budgets;
            let tb = run_market_day(&mut b, &mut budgets_b, &club_reputation);

            prop_assert_eq!(ta, tb);
            prop_assert_eq!(a, b);
            prop_assert_eq!(budgets_a, budgets_b);
        }
    }
}
