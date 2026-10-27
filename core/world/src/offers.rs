//! Propostas da IA pelos jogadores do técnico humano (RF-TR-05, lado
//! "vender"): a cada rodada com a janela aberta, no máximo um clube da IA
//! faz uma oferta por um jogador do clube sob comando. O técnico responde
//! (`app::Command::RespondToOffer`); sem resposta a oferta expira.
//!
//! Tudo deriva de `(world_seed, temporada, rodada)` e do estado — função
//! pura, reproduzível por replay do log de comandos. Quem faz a oferta: um
//! clube da IA para o qual o jogador seria **titular** (melhor que o pior
//! titular dele na mesma posição), que tenha caixa pelo valor ofertado e
//! que o jogador aceitaria (mesma regra de ambição/reputação do mercado da
//! IA, `ai::accepts_destination`). Entre os pares possíveis, o clube vai
//! atrás do melhor jogador; o valor ofertado é o de mercado entre 90% e
//! 120%. Nada calibrado (`docs/04 §4`).

use domain::{ClubId, DeterministicRng, Money, PlayerId, Position};

use crate::contract::ContractState;
use crate::progression::PlayerState;

/// Chance (em milésimos) de surgir uma oferta numa rodada com janela aberta.
const OFFER_CHANCE_PERMILLE: u32 = 300;
/// Faixa do valor ofertado sobre o valor de mercado, em milésimos.
const MIN_BID_PERMILLE: u32 = 900;
const MAX_BID_PERMILLE: u32 = 1200;
/// Quantas rodadas uma oferta fica de pé.
pub const OFFER_LIFETIME_MATCHDAYS: u32 = 2;

/// Uma oferta pendente por um jogador do técnico.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IncomingOffer {
    pub id: u32,
    pub player: PlayerId,
    pub from: ClubId,
    pub fee: Money,
    /// A oferta vale enquanto a próxima rodada a jogar for menor que isto.
    pub expires_matchday: u32,
}

/// Tudo que a geração lê do mundo, agrupado (como `CareerRound`).
pub struct OfferWorld<'a> {
    pub roster: &'a [PlayerState],
    pub contracts: &'a [ContractState],
    pub budgets: &'a [Money],
    pub club_reputation: &'a [u8],
    pub ambition: &'a [u8],
}

/// Pior titular de `club` em `position` (menor CA), ou `None` se o clube não
/// tem titular ali. Titular = o onze automático da IA.
fn weakest_starter_ca(world: &OfferWorld<'_>, club: ClubId, position: Position) -> Option<u8> {
    crate::quality::starters_from_roster(world.roster, club)
        .into_iter()
        .map(|id| &world.roster[id.as_usize()])
        .filter(|p| p.position == position)
        .map(|p| p.ability.current())
        .min()
}

/// Gera, se houver, a oferta desta rodada. `pending` são os jogadores que já
/// têm oferta de pé (não recebem outra). Devolve `(jogador, comprador,
/// valor)`; quem chama dá o `id` e a validade.
#[must_use]
pub fn generate_incoming_offer(
    world: &OfferWorld<'_>,
    managed: ClubId,
    pending: &[PlayerId],
    world_seed: u64,
    season_index: u32,
    matchday: u32,
) -> Option<(PlayerId, ClubId, Money)> {
    let mut rng = DeterministicRng::seeded(
        world_seed,
        "world.incoming_offer",
        u64::from(season_index),
        u64::from(matchday),
    );
    if !rng.chance_per_mille(OFFER_CHANCE_PERMILLE) {
        return None;
    }
    let bid_permille = MIN_BID_PERMILLE + rng.below(MAX_BID_PERMILLE - MIN_BID_PERMILLE + 1);

    let managed_reputation = world.club_reputation[managed.as_usize()];
    let value_of = |p: &PlayerState| {
        ai::market_value(
            p.ability.current(),
            world.contracts[p.player.as_usize()].seasons_remaining,
            p.age_years.clamp(0, 255) as u8,
            p.ability.potential(),
            managed_reputation,
        )
    };

    // Melhor jogador do técnico que algum clube queira, do melhor pro pior
    // (desempate: menor id).
    let mut targets: Vec<&PlayerState> = world
        .roster
        .iter()
        .filter(|p| p.club == managed && !pending.contains(&p.player))
        .collect();
    targets.sort_by(|a, b| {
        b.ability
            .current()
            .cmp(&a.ability.current())
            .then(a.player.cmp(&b.player))
    });

    for target in targets {
        let value = value_of(target);
        let fee = Money::from_cents(value.cents() * i64::from(bid_permille) / 1000);
        let ambition = world.ambition[target.player.as_usize()];
        for club_index in 0..world.budgets.len() {
            let club = ClubId::new(club_index as u32);
            if club == managed || world.budgets[club_index] < fee {
                continue;
            }
            let club_reputation = world.club_reputation[club_index];
            if !ai::accepts_destination(ambition, managed_reputation, club_reputation) {
                continue;
            }
            let needs = match weakest_starter_ca(world, club, target.position) {
                Some(weakest) => target.ability.current() > weakest,
                None => false,
            };
            if needs {
                return Some((target.player, club, fee));
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    struct Fixture {
        roster: Vec<PlayerState>,
        contracts: Vec<ContractState>,
        budgets: Vec<Money>,
        reputation: Vec<u8>,
        ambition: Vec<u8>,
    }

    fn fixture() -> Fixture {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/example-two-tier");
        let pack = pack::load_and_validate(&path).unwrap().pack;
        Fixture {
            roster: crate::progression::initial_roster(&pack),
            contracts: crate::contract::generate_initial_contracts(&pack, 1),
            budgets: vec![Money::from_cents(1_000_000_000_000); pack.clubs.len()],
            reputation: vec![50; pack.clubs.len()],
            ambition: vec![0; pack.players.len()],
        }
    }

    fn world(f: &Fixture) -> OfferWorld<'_> {
        OfferWorld {
            roster: &f.roster,
            contracts: &f.contracts,
            budgets: &f.budgets,
            club_reputation: &f.reputation,
            ambition: &f.ambition,
        }
    }

    /// Procura uma rodada em que a chance sorteada dá oferta.
    fn first_offer(
        f: &Fixture,
        managed: ClubId,
        pending: &[PlayerId],
    ) -> (u32, PlayerId, ClubId, Money) {
        (0..200)
            .find_map(|matchday| {
                generate_incoming_offer(&world(f), managed, pending, 42, 0, matchday)
                    .map(|(p, c, m)| (matchday, p, c, m))
            })
            .expect("com 30% por rodada, 200 rodadas dão pelo menos uma oferta")
    }

    #[test]
    fn oferta_e_deterministica_para_o_mesmo_estado_e_rodada() {
        let f = fixture();
        let managed = ClubId::new(3);
        let a = first_offer(&f, managed, &[]);
        let b = first_offer(&f, managed, &[]);
        assert_eq!(a, b);
    }

    #[test]
    fn oferta_e_por_jogador_do_tecnico_de_outro_clube_que_caiba_no_caixa() {
        let f = fixture();
        let managed = ClubId::new(3);
        let (_, player, from, fee) = first_offer(&f, managed, &[]);
        assert_eq!(f.roster[player.as_usize()].club, managed);
        assert_ne!(from, managed);
        assert!(f.budgets[from.as_usize()] >= fee);
        assert!(fee.cents() > 0);
        // O comprador de fato teria o jogador como titular.
        let weakest = weakest_starter_ca(&world(&f), from, f.roster[player.as_usize()].position);
        assert!(f.roster[player.as_usize()].ability.current() > weakest.unwrap());
    }

    #[test]
    fn jogador_com_oferta_pendente_nao_recebe_outra() {
        let f = fixture();
        let managed = ClubId::new(3);
        let (matchday, player, _, _) = first_offer(&f, managed, &[]);
        let again = generate_incoming_offer(&world(&f), managed, &[player], 42, 0, matchday);
        if let Some((other, ..)) = again {
            assert_ne!(other, player);
        }
    }

    #[test]
    fn sem_caixa_ou_sem_necessidade_nao_ha_oferta() {
        let mut f = fixture();
        let managed = ClubId::new(3);
        f.budgets = vec![Money::ZERO; f.budgets.len()];
        for matchday in 0..100 {
            assert!(generate_incoming_offer(&world(&f), managed, &[], 42, 0, matchday).is_none());
        }
    }

    #[test]
    fn jogador_ambicioso_nao_recebe_oferta_de_clube_de_reputacao_menor() {
        let mut f = fixture();
        let managed = ClubId::new(3);
        f.reputation = vec![10; f.reputation.len()];
        f.reputation[3] = 90;
        f.ambition = vec![100; f.ambition.len()];
        for matchday in 0..100 {
            assert!(generate_incoming_offer(&world(&f), managed, &[], 42, 0, matchday).is_none());
        }
    }

    #[test]
    fn a_chance_por_rodada_e_a_configurada_em_media() {
        let f = fixture();
        let managed = ClubId::new(3);
        let hits = (0..2000)
            .filter(|&m| generate_incoming_offer(&world(&f), managed, &[], 7, 0, m).is_some())
            .count();
        // 30% ± folga estatística grande.
        assert!((450..=750).contains(&hits), "{hits}");
    }
}
