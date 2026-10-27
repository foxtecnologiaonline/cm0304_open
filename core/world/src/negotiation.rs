//! Negociação de transferência e de contrato (RF-TR-03/04, `docs/01 §2.4`)
//! — as regras de decisão, puras e sem RNG: dado o mesmo estado e a mesma
//! proposta, a resposta é sempre a mesma. É o que torna a negociação
//! iterativa (o técnico tenta, lê o que faltou, ajusta e tenta de novo)
//! reproduzível por replay do log de comandos.
//!
//! Duas conversas em sequência numa compra, como no jogo de verdade:
//!
//! 1. **Clube vendedor** ([`evaluate_club`]): pede o valor de mercado mais
//!    10% ([`asking_fee`]). Abaixo de 70% do pedido é ofensa; entre 70% e
//!    100%, pede mais; no pedido ou acima, aceita — desde que possa abrir
//!    mão do jogador ([`seller_can_part`]: elenco mínimo e uma opção por
//!    linha) e o jogador não seja intocável (lealdade acima do limiar de
//!    `ai::LOYALTY_REFUSAL_THRESHOLD`, o mesmo do mercado da IA).
//! 2. **Jogador** ([`wage_ask_permille`]): recusa de saída se for muito
//!    ambicioso e o destino tiver reputação menor
//!    (`ai::accepts_destination`, a mesma regra da IA); senão pede um
//!    salário, em milésimos do salário-base do CA dele, que sobe com a
//!    distância de reputação entre os clubes e com a ambição.
//!
//! Nada aqui é calibrado (`docs/04 §4`): pontos de partida plausíveis.

use domain::{ClubId, Money, PlayerId, Position};

use crate::progression::PlayerState;

/// Faixa de duração de um contrato negociado, em temporadas.
pub const MIN_CONTRACT_SEASONS: u8 = 1;
pub const MAX_CONTRACT_SEASONS: u8 = 5;

/// Faixa de salário aceita numa proposta, em milésimos do salário-base
/// (`1000` = o salário-base do CA do jogador).
pub const MIN_WAGE_PERMILLE: u16 = 500;
pub const MAX_WAGE_PERMILLE: u16 = 3000;

/// Quanto o vendedor cobra acima do valor de mercado, em milésimos.
const SELLER_MARKUP_PERMILLE: i64 = 1100;
/// Abaixo desta fração do pedido (em milésimos) a proposta é uma ofensa.
const INSULT_THRESHOLD_PERMILLE: i64 = 700;
/// Elenco mínimo que o vendedor mantém depois de vender.
const MIN_SQUAD_AFTER_SALE: usize = 14;

/// Por que um lado recusou de vez (sem contraproposta possível).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Refusal {
    /// Proposta muito abaixo do pedido.
    InsultingOffer,
    /// Jogador intocável (lealdade alta ao clube atual).
    NotForSale,
    /// Vender deixaria o vendedor sem elenco ou sem uma linha.
    SquadLimits,
    /// Jogador ambicioso não aceita um clube de reputação menor.
    WantsBiggerClub,
}

/// Resposta a uma proposta de compra (ou de renovação).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OfferOutcome {
    /// Fechado: o jogador mudou de clube / o contrato foi renovado.
    Accepted,
    /// O clube vendedor quer mais dinheiro: `asking` fecha o negócio.
    ClubWantsMore {
        asking: Money,
    },
    ClubRefuses(Refusal),
    /// O clube comprador não tem caixa para a taxa proposta.
    CannotAfford,
    /// O jogador quer mais salário: `asking_wage_permille` fecha.
    PlayerWantsMore {
        asking_wage_permille: u16,
    },
    PlayerRefuses(Refusal),
}

/// O que o clube vendedor pede por um jogador de valor `value`.
#[must_use]
pub fn asking_fee(value: Money) -> Money {
    Money::from_cents(value.cents() * SELLER_MARKUP_PERMILLE / 1000)
}

/// Decisão do clube vendedor sobre a taxa proposta.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClubVerdict {
    Accept,
    Counter(Money),
    Refuse(Refusal),
}

#[must_use]
pub fn evaluate_club(fee: Money, value: Money) -> ClubVerdict {
    let asking = asking_fee(value);
    if fee >= asking {
        ClubVerdict::Accept
    } else if fee.cents() * 1000 < asking.cents() * INSULT_THRESHOLD_PERMILLE {
        ClubVerdict::Refuse(Refusal::InsultingOffer)
    } else {
        ClubVerdict::Counter(asking)
    }
}

/// `true` se `seller` pode vender `player` sem ficar com menos de
/// [`MIN_SQUAD_AFTER_SALE`] jogadores nem sem o mínimo por linha (1
/// goleiro, 4 defensores, 4 meio-campistas, 2 atacantes).
#[must_use]
pub fn seller_can_part(roster: &[PlayerState], seller: ClubId, player: PlayerId) -> bool {
    let mut squad = 0usize;
    let mut by_position = [0usize; 4];
    let mut sold_position = None;
    for p in roster.iter().filter(|p| p.club == seller) {
        squad += 1;
        by_position[position_index(p.position)] += 1;
        if p.player == player {
            sold_position = Some(p.position);
        }
    }
    let Some(sold) = sold_position else {
        return false;
    };
    by_position[position_index(sold)] -= 1;
    let minimum = [1usize, 4, 4, 2];
    squad > MIN_SQUAD_AFTER_SALE
        && by_position
            .iter()
            .zip(minimum)
            .all(|(have, need)| *have >= need)
}

const fn position_index(p: Position) -> usize {
    match p {
        Position::Goalkeeper => 0,
        Position::Defender => 1,
        Position::Midfielder => 2,
        Position::Forward => 3,
    }
}

/// Salário que um jogador pede para se mudar de um clube de reputação
/// `seller_reputation` para um de `buyer_reputation`, em milésimos do
/// salário-base: 1000 + 4 por ponto de reputação que o comprador fica
/// abaixo (até +300) + 2 por ponto de ambição (até +200).
#[must_use]
pub fn wage_ask_permille(ambition: u8, seller_reputation: u8, buyer_reputation: u8) -> u16 {
    let gap = u16::from(seller_reputation.saturating_sub(buyer_reputation)).min(75) * 4;
    let ambition_premium = u16::from(ambition.min(100)) * 2;
    1000 + gap + ambition_premium
}

/// Salário que um jogador do próprio clube pede para renovar, em milésimos
/// do salário-base: 1000 + 3 por ponto de ambição acima de 50 − 1 por ponto
/// de lealdade (leal aceita mais barato). Entre 850 e 1150.
#[must_use]
pub fn renewal_ask_permille(ambition: u8, loyalty: u8) -> u16 {
    let premium = i32::from(ambition.min(100)).saturating_sub(50).max(0) * 3;
    let discount = i32::from(loyalty.min(100));
    (1000 + premium - discount).clamp(850, 1150) as u16
}

#[cfg(test)]
mod tests {
    use super::*;
    use domain::{Ability, PlayerId};

    fn player(id: u32, club: u32, position: Position) -> PlayerState {
        PlayerState {
            player: PlayerId::new(id),
            club: ClubId::new(club),
            position,
            age_years: 25,
            ability: Ability::new(100, 120),
            injured: false,
            condition: 100,
        }
    }

    /// Clube 0 com 16 jogadores (2 GK, 5 DF, 5 MF, 4 FW) e 1 do clube 1.
    fn squad() -> Vec<PlayerState> {
        let mut roster = Vec::new();
        let mut id = 0;
        for (count, position) in [
            (2, Position::Goalkeeper),
            (5, Position::Defender),
            (5, Position::Midfielder),
            (4, Position::Forward),
        ] {
            for _ in 0..count {
                roster.push(player(id, 0, position));
                id += 1;
            }
        }
        roster.push(player(id, 1, Position::Forward));
        roster
    }

    #[test]
    fn pedido_e_o_valor_mais_dez_por_cento() {
        assert_eq!(
            asking_fee(Money::from_cents(1_000_000)),
            Money::from_cents(1_100_000)
        );
    }

    #[test]
    fn clube_aceita_no_pedido_pede_mais_no_meio_e_se_ofende_embaixo() {
        let value = Money::from_cents(1_000_000);
        let asking = asking_fee(value);
        assert_eq!(evaluate_club(asking, value), ClubVerdict::Accept);
        assert_eq!(
            evaluate_club(Money::from_cents(asking.cents() * 2), value),
            ClubVerdict::Accept
        );
        assert_eq!(
            evaluate_club(Money::from_cents(asking.cents() - 1), value),
            ClubVerdict::Counter(asking)
        );
        assert_eq!(
            evaluate_club(Money::from_cents(asking.cents() * 70 / 100), value),
            ClubVerdict::Counter(asking),
            "exatamente no limite ainda é contraproposta"
        );
        assert_eq!(
            evaluate_club(Money::from_cents(asking.cents() * 69 / 100), value),
            ClubVerdict::Refuse(Refusal::InsultingOffer)
        );
        assert_eq!(
            evaluate_club(Money::ZERO, value),
            ClubVerdict::Refuse(Refusal::InsultingOffer)
        );
    }

    #[test]
    fn vendedor_mantem_elenco_minimo_e_uma_opcao_por_linha() {
        let mut roster = squad();
        // 16 jogadores: vender um reserva de qualquer linha com folga é ok.
        assert!(seller_can_part(&roster, ClubId::new(0), PlayerId::new(15)));
        // Vender um dos 2 goleiros ainda deixa 1.
        assert!(seller_can_part(&roster, ClubId::new(0), PlayerId::new(0)));
        // Sem o segundo goleiro, vender o único é proibido.
        roster.remove(1);
        assert!(!seller_can_part(&roster, ClubId::new(0), PlayerId::new(0)));
        // Jogador de outro clube / inexistente nunca.
        assert!(!seller_can_part(&roster, ClubId::new(0), PlayerId::new(99)));
    }

    #[test]
    fn vendedor_nao_cai_abaixo_de_catorze() {
        let mut roster = squad();
        roster.truncate(14); // sobram 14 do clube 0: não pode vender nenhum
        for p in 0..14 {
            assert!(!seller_can_part(&roster, ClubId::new(0), PlayerId::new(p)));
        }
    }

    #[test]
    fn salario_pedido_sobe_com_a_distancia_de_reputacao_e_com_a_ambicao() {
        let base = wage_ask_permille(0, 50, 50);
        assert_eq!(base, 1000);
        assert!(wage_ask_permille(0, 80, 40) > base);
        assert!(wage_ask_permille(100, 50, 50) > base);
        assert_eq!(
            wage_ask_permille(0, 40, 80),
            1000,
            "destino maior não pede mais"
        );
        // Teto: nem reputação 100 vs 0 com ambição 100 passa de 1500.
        assert_eq!(wage_ask_permille(100, 100, 0), 1000 + 300 + 200);
    }

    #[test]
    fn renovacao_fica_entre_850_e_1150_e_leal_pede_menos() {
        assert_eq!(renewal_ask_permille(50, 0), 1000);
        assert!(renewal_ask_permille(50, 100) < renewal_ask_permille(50, 0));
        assert!(renewal_ask_permille(100, 0) > renewal_ask_permille(50, 0));
        assert_eq!(renewal_ask_permille(0, 100), 900);
        assert!((850..=1150).contains(&renewal_ask_permille(100, 0)));
        assert_eq!(renewal_ask_permille(100, 0), 1150);
    }
}
