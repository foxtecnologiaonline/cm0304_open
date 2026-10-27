//! Escalação e instruções do técnico humano (RF-PA-01/02, `docs/01 §2.5`) —
//! o único ponto onde uma decisão do usuário muda a simulação de partida.
//!
//! Só o clube sob comando de um técnico tem um [`ClubSetup`]; todos os
//! outros (a IA) seguem o caminho de sempre — 4-4-2, melhores por posição,
//! sem tática —, e é por isso que um [`ClubSetup::new`] padrão (formação
//! 4-4-2, escalação automática, mentalidade equilibrada, pressão normal)
//! produz **exatamente** o mesmo perfil e as mesmas táticas neutras que o
//! caminho da IA (testado): assumir um clube não muda nada até o técnico
//! decidir algo.
//!
//! ## O que cada decisão faz (nada aqui é calibrado — `docs/04 §4`)
//!
//! * **Formação** define os 11 espaços por posição ([`ai::Formation`]) e
//!   também a tática: mais atacantes sobem as finalizações dos dois lados;
//!   mais defensores baixam as dos dois.
//! * **Escalação manual**: o técnico escolhe quem ocupa cada espaço. Quem
//!   joga **fora da posição natural** rende menos ([`position_fit_permille`]).
//!   Se um titular escolhido está indisponível na rodada (lesão nova,
//!   suspensão, vendido), entra o melhor substituto disponível — o motor
//!   nunca joga com menos de 11 por causa disso, e a troca é reportada
//!   ([`ResolvedXi::adjustments`]).
//! * **Mentalidade**: ataque sobe o que o time cria e o que cede; defesa
//!   baixa os dois. Ajuda o favorito a atacar e o azarão a se fechar.
//! * **Pressão alta**: paga ou cobra conforme a resistência média do XI —
//!   com elenco em forma cria mais e cede menos; **sem fôlego, o efeito
//!   inverte**. (Fadiga ao longo da temporada, o custo natural da pressão,
//!   chega com RF-JG-07 completo.)
//!
//! Táticas finas (largura, linha defensiva, marcação) pedem zonas no motor
//! (`docs/04 §2.2`, M3) e ficam de fora.

use std::collections::HashSet;

use ai::Formation;
use domain::{Attribute, ClubId, Fixed, PlayerId, Position};
use engine::{FinishingQuality, GoalkeepingQuality, Tactics, TeamMatchProfile, TeamStrength};
use pack::LoadedPack;

use crate::condition::FULL_CONDITION;
use crate::progression::PlayerState;

/// Mentalidade do time (RF-PA-02).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Mentality {
    Defensive,
    Balanced,
    Attacking,
}

impl Mentality {
    /// Código estável de 1 byte (log de comandos): **nunca renumerar**.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Mentality::Defensive => 0,
            Mentality::Balanced => 1,
            Mentality::Attacking => 2,
        }
    }

    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Mentality::Defensive),
            1 => Some(Mentality::Balanced),
            2 => Some(Mentality::Attacking),
            _ => None,
        }
    }
}

/// Intensidade da pressão (RF-PA-02).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Pressing {
    Normal,
    High,
}

impl Pressing {
    /// Código estável de 1 byte (log de comandos): **nunca renumerar**.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Pressing::Normal => 0,
            Pressing::High => 1,
        }
    }

    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Pressing::Normal),
            1 => Some(Pressing::High),
            _ => None,
        }
    }
}

/// As decisões do técnico para o clube que ele comanda.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClubSetup {
    pub club: ClubId,
    pub formation: Formation,
    /// Os 11 titulares na ordem dos espaços de `formation.slots()`
    /// (goleiro, defensores, meio-campistas, atacantes). `None` = o melhor
    /// onze disponível a cada rodada (escalação automática).
    pub starters: Option<Vec<PlayerId>>,
    pub mentality: Mentality,
    pub pressing: Pressing,
}

impl ClubSetup {
    /// O padrão: indistinguível da IA (ver o doc do módulo).
    #[must_use]
    pub fn new(club: ClubId) -> Self {
        Self {
            club,
            formation: Formation::F442,
            starters: None,
            mentality: Mentality::Balanced,
            pressing: Pressing::Normal,
        }
    }
}

/// O onze que de fato entra em campo numa rodada.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedXi {
    /// `(posição do espaço, jogador)`, na ordem de escalação. Menos de 11
    /// só se o clube não tem 11 jogadores disponíveis.
    pub slots: Vec<(Position, PlayerId)>,
    /// `(escalado que não pôde jogar, quem entrou no lugar)`.
    pub adjustments: Vec<(PlayerId, PlayerId)>,
}

impl ResolvedXi {
    #[must_use]
    pub fn players(&self) -> Vec<PlayerId> {
        self.slots.iter().map(|&(_, p)| p).collect()
    }
}

/// Quanto da habilidade de um jogador de posição natural `natural`
/// aproveita num espaço de posição `slot`, em milésimos: 100% na posição
/// natural, 85% numa linha vizinha (defesa↔meio, meio↔ataque), 70% pulando
/// uma linha (defesa↔ataque), 50% se um goleiro está envolvido. Substitui a
/// familiaridade posicional completa (RF-JG-04, 0-20 por posição com
/// aprendizado por treino), que fica de fora do MVP.
#[must_use]
pub fn position_fit_permille(natural: Position, slot: Position) -> u32 {
    use Position::{Defender, Forward, Goalkeeper, Midfielder};
    match (natural, slot) {
        (a, b) if a == b => 1000,
        (Defender, Midfielder)
        | (Midfielder, Defender)
        | (Midfielder, Forward)
        | (Forward, Midfielder) => 850,
        (Defender, Forward) | (Forward, Defender) => 700,
        (Goalkeeper, _) | (_, Goalkeeper) => 500,
        _ => 1000,
    }
}

/// Resolve o onze de `setup.club` para uma rodada, dado quem está
/// indisponível (`excluded`: suspensos) além dos lesionados e dos que já não
/// são do clube.
#[must_use]
pub fn resolve_xi(
    roster: &[PlayerState],
    setup: &ClubSetup,
    excluded: &HashSet<PlayerId>,
) -> ResolvedXi {
    let available =
        |p: &PlayerState| p.club == setup.club && !p.injured && !excluded.contains(&p.player);

    let Some(wanted) = &setup.starters else {
        // Automático: mesma seleção da IA, só que na formação escolhida.
        let squad: Vec<&PlayerState> = roster.iter().filter(|p| available(p)).collect();
        let ratings: Vec<ai::PlayerRating> = squad
            .iter()
            .enumerate()
            .map(|(idx, p)| ai::PlayerRating {
                player: PlayerId::new(idx as u32),
                position: p.position,
                current_ability: p.ability.current(),
            })
            .collect();
        let slots = ai::select_starting_eleven_for(&ratings, setup.formation)
            .into_iter()
            .map(|local| {
                let p = squad[local.as_usize()];
                (p.position, p.player)
            })
            .collect();
        return ResolvedXi {
            slots,
            adjustments: Vec::new(),
        };
    };

    let slot_positions = setup.formation.slots();
    let state_of = |id: PlayerId| roster.get(id.as_usize());
    // Passo 1: quem escalou e está disponível fica onde está.
    let mut chosen: Vec<Option<PlayerId>> = wanted
        .iter()
        .take(slot_positions.len())
        .map(|&id| state_of(id).filter(|p| available(p)).map(|_| id))
        .collect();
    chosen.resize(slot_positions.len(), None);
    let mut used: HashSet<PlayerId> = chosen.iter().flatten().copied().collect();

    // Passo 2: cada espaço vazio recebe o disponível de maior habilidade
    // efetiva (já descontando jogar fora de posição); empate → menor id.
    let mut adjustments = Vec::new();
    for (i, slot) in slot_positions.iter().enumerate() {
        if chosen[i].is_some() {
            continue;
        }
        let replacement = roster
            .iter()
            .filter(|p| available(p) && !used.contains(&p.player))
            .max_by(|a, b| {
                let eff = |p: &PlayerState| {
                    u32::from(p.ability.current()) * position_fit_permille(p.position, *slot)
                };
                eff(a).cmp(&eff(b)).then(b.player.cmp(&a.player))
            })
            .map(|p| p.player);
        if let Some(player) = replacement {
            used.insert(player);
            chosen[i] = Some(player);
            if let Some(&out) = wanted.get(i) {
                adjustments.push((out, player));
            }
        }
    }

    ResolvedXi {
        slots: slot_positions
            .iter()
            .zip(chosen)
            .filter_map(|(&slot, p)| p.map(|p| (slot, p)))
            .collect(),
        adjustments,
    }
}

/// Valor "neutro" de qualidade quando o onze não tem goleiro ou atacante —
/// mesmo centro de escala (atributos 1-20) de `crate::quality`.
const NEUTRAL_QUALITY: i32 = 10;

/// Perfil e táticas de partida de um onze resolvido. `None` se o onze está
/// vazio (clube sem ninguém disponível — quem chama usa o perfil sintético).
///
/// A fórmula de força/finalização/goleiro é a de `crate::quality`
/// (`build_profile`), só que sobre os espaços escolhidos: CA efetivo =
/// CA × encaixe na posição × condição, média de 11; finalização = média de
/// (finalização, frieza) de quem ocupa espaços de ataque; goleiro = média de
/// (reflexos, posicionamento, jogo aéreo) de quem ocupa o gol. Com o onze
/// automático da IA em posições naturais, dá exatamente o mesmo número.
#[must_use]
pub fn profile_for(
    pack: &LoadedPack,
    roster: &[PlayerState],
    xi: &ResolvedXi,
    setup: &ClubSetup,
) -> Option<(TeamMatchProfile, Tactics)> {
    if xi.slots.is_empty() {
        return None;
    }
    let mut total_strength = 0u32;
    let (mut finishing_sum, mut finishing_n) = (0i32, 0i32);
    let mut goalkeeping = NEUTRAL_QUALITY;
    let mut stamina_sum = 0i32;
    for &(slot, id) in &xi.slots {
        let state = &roster[id.as_usize()];
        let attrs = pack.players[id.as_usize()].attributes;
        let fitted =
            u32::from(state.ability.current()) * position_fit_permille(state.position, slot) / 1000;
        total_strength += fitted * u32::from(state.condition) / u32::from(FULL_CONDITION);
        stamina_sum += i32::from(attrs.get(Attribute::Stamina));
        match slot {
            Position::Goalkeeper => {
                goalkeeping = (i32::from(attrs.get(Attribute::Reflexes))
                    + i32::from(attrs.get(Attribute::Positioning))
                    + i32::from(attrs.get(Attribute::AerialAbility)))
                    / 3;
            }
            Position::Forward => {
                finishing_sum += (i32::from(attrs.get(Attribute::Finishing))
                    + i32::from(attrs.get(Attribute::Composure)))
                    / 2;
                finishing_n += 1;
            }
            _ => {}
        }
    }
    let n = xi.slots.len() as u32;
    let finishing = if finishing_n == 0 {
        NEUTRAL_QUALITY
    } else {
        finishing_sum / finishing_n
    };
    let profile = TeamMatchProfile {
        strength: TeamStrength::new(Fixed::from_int((total_strength / n) as i32)),
        finishing: FinishingQuality::new(Fixed::from_int(finishing)),
        goalkeeping: GoalkeepingQuality::new(Fixed::from_int(goalkeeping)),
    };
    let avg_stamina = stamina_sum / xi.slots.len() as i32;
    Some((
        profile,
        tactics_for(
            setup.formation,
            setup.mentality,
            setup.pressing,
            avg_stamina,
        ),
    ))
}

/// Combina formação, mentalidade e pressão em [`Tactics`]. `avg_stamina` é
/// a resistência média (1-20) do onze. Formação 4-4-2 + equilibrada +
/// pressão normal é exatamente [`Tactics::NEUTRAL`].
#[must_use]
pub fn tactics_for(
    formation: Formation,
    mentality: Mentality,
    pressing: Pressing,
    avg_stamina: i32,
) -> Tactics {
    let (d, _m, f) = formation.lines();
    let (d, f) = (d as i64, f as i64);
    // Formação: cada atacante a mais (vs. 2) cria +4% e cede +3%; cada
    // defensor a mais (vs. 4) cria -1,5% e cede -3%.
    let formation_volume = 1000 + 40 * (f - 2) - 15 * (d - 4);
    let formation_concession = 1000 + 30 * (f - 2) - 30 * (d - 4);
    let (mentality_volume, mentality_concession) = match mentality {
        Mentality::Defensive => (880, 850),
        Mentality::Balanced => (1000, 1000),
        Mentality::Attacking => (1120, 1150),
    };
    // Pressão alta: ±0,8% por ponto de resistência acima/abaixo de 12.
    let (press_volume, press_concession) = match pressing {
        Pressing::Normal => (1000, 1000),
        Pressing::High => {
            let fit = i64::from(avg_stamina - 12);
            (1000 + 8 * fit, 1000 - 8 * fit)
        }
    };
    let volume = formation_volume * mentality_volume / 1000 * press_volume / 1000;
    let concession = formation_concession * mentality_concession / 1000 * press_concession / 1000;
    Tactics::new(volume.max(0) as u32, concession.max(0) as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn example_pack() -> LoadedPack {
        let path =
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/example-two-tier");
        pack::load_and_validate(&path).unwrap().pack
    }

    fn roster(pack: &LoadedPack) -> Vec<PlayerState> {
        crate::progression::initial_roster(pack)
    }

    #[test]
    fn setup_padrao_e_identico_ao_caminho_da_ia_em_todo_clube() {
        let pack = example_pack();
        let roster = roster(&pack);
        for club in pack.clubs.iter().map(|c| c.id) {
            let setup = ClubSetup::new(club);
            let xi = resolve_xi(&roster, &setup, &HashSet::new());
            let (profile, tactics) = profile_for(&pack, &roster, &xi, &setup).unwrap();
            let expected = crate::quality::profile_from_roster(&pack, &roster, club).unwrap();
            assert_eq!(profile, expected, "perfil do clube {club}");
            assert_eq!(tactics, Tactics::NEUTRAL);
            assert_eq!(
                xi.players(),
                crate::quality::starters_from_roster(&roster, club),
                "mesmos titulares"
            );
        }
    }

    #[test]
    fn tatica_padrao_e_neutra_e_formacoes_trocam_criar_por_ceder() {
        let neutral = |f| tactics_for(f, Mentality::Balanced, Pressing::Normal, 12);
        assert_eq!(neutral(Formation::F442), Tactics::NEUTRAL);
        let attack = neutral(Formation::F433);
        assert!(attack.shot_volume_permille > 1000 && attack.concession_permille > 1000);
        let defense = neutral(Formation::F532);
        assert!(defense.shot_volume_permille < 1000 && defense.concession_permille < 1000);
        // Defender mais cede proporcionalmente menos do que cria a menos.
        assert!(defense.concession_permille < defense.shot_volume_permille);
    }

    #[test]
    fn mentalidade_ofensiva_cria_e_cede_mais_a_defensiva_cria_e_cede_menos() {
        let t = |m| tactics_for(Formation::F442, m, Pressing::Normal, 12);
        assert!(t(Mentality::Attacking).shot_volume_permille > 1000);
        assert!(t(Mentality::Attacking).concession_permille > 1000);
        assert!(t(Mentality::Defensive).shot_volume_permille < 1000);
        assert!(t(Mentality::Defensive).concession_permille < 1000);
        assert_eq!(t(Mentality::Balanced), Tactics::NEUTRAL);
    }

    #[test]
    fn pressao_alta_ajuda_elenco_em_forma_e_prejudica_elenco_sem_folego() {
        let high = |stamina| {
            tactics_for(
                Formation::F442,
                Mentality::Balanced,
                Pressing::High,
                stamina,
            )
        };
        let fit = high(17);
        assert!(fit.shot_volume_permille > 1000 && fit.concession_permille < 1000);
        let tired = high(7);
        assert!(tired.shot_volume_permille < 1000 && tired.concession_permille > 1000);
        assert_eq!(
            high(12),
            Tactics::NEUTRAL,
            "resistência 12 é o ponto neutro"
        );
    }

    #[test]
    fn escalacao_manual_valida_e_respeitada_na_ordem_dos_espacos() {
        let pack = example_pack();
        let roster = roster(&pack);
        let club = ClubId::new(0);
        let auto = resolve_xi(&roster, &ClubSetup::new(club), &HashSet::new());
        let mut setup = ClubSetup::new(club);
        setup.starters = Some(auto.players());
        let xi = resolve_xi(&roster, &setup, &HashSet::new());
        assert_eq!(xi, auto);
        assert!(xi.adjustments.is_empty());
    }

    #[test]
    fn titular_lesionado_ou_suspenso_e_trocado_pelo_melhor_da_mesma_posicao() {
        let pack = example_pack();
        let mut roster = roster(&pack);
        let club = ClubId::new(0);
        let auto = resolve_xi(&roster, &ClubSetup::new(club), &HashSet::new());
        let mut setup = ClubSetup::new(club);
        setup.starters = Some(auto.players());

        let injured = auto.slots[3].1; // um defensor
        let suspended = auto.slots[9].1; // um atacante
        roster[injured.as_usize()].injured = true;
        let excluded: HashSet<PlayerId> = [suspended].into_iter().collect();

        let xi = resolve_xi(&roster, &setup, &excluded);
        assert_eq!(xi.slots.len(), 11);
        assert_eq!(xi.adjustments.len(), 2);
        let ids = xi.players();
        assert!(!ids.contains(&injured) && !ids.contains(&suspended));
        // 11 distintos, todos disponíveis, nas posições naturais (o elenco
        // de exemplo tem reservas de todas as posições).
        let distinct: HashSet<_> = ids.iter().collect();
        assert_eq!(distinct.len(), 11);
        for ((slot, id), expected_slot) in xi.slots.iter().zip(setup.formation.slots()) {
            assert_eq!(*slot, expected_slot);
            assert_eq!(roster[id.as_usize()].position, expected_slot);
        }
        for (out, incoming) in &xi.adjustments {
            assert!([injured, suspended].contains(out));
            assert_eq!(roster[incoming.as_usize()].club, club);
        }
    }

    #[test]
    fn jogador_vendido_deixa_de_ser_titular_e_e_substituido() {
        let pack = example_pack();
        let mut roster = roster(&pack);
        let club = ClubId::new(0);
        let auto = resolve_xi(&roster, &ClubSetup::new(club), &HashSet::new());
        let mut setup = ClubSetup::new(club);
        setup.starters = Some(auto.players());
        let sold = auto.slots[5].1;
        roster[sold.as_usize()].club = ClubId::new(1);
        let xi = resolve_xi(&roster, &setup, &HashSet::new());
        assert!(!xi.players().contains(&sold));
        assert_eq!(xi.adjustments.len(), 1);
    }

    #[test]
    fn jogar_fora_de_posicao_rende_menos_que_na_natural() {
        let pack = example_pack();
        let roster = roster(&pack);
        let club = ClubId::new(0);
        let auto_setup = ClubSetup::new(club);
        let auto = resolve_xi(&roster, &auto_setup, &HashSet::new());
        let (natural, _) = profile_for(&pack, &roster, &auto, &auto_setup).unwrap();

        // Troca o goleiro por um atacante no gol e o atacante pelo goleiro.
        let mut shuffled = auto.clone();
        shuffled.slots[0].1 = auto.slots[10].1;
        shuffled.slots[10].1 = auto.slots[0].1;
        let (displaced, _) = profile_for(&pack, &roster, &shuffled, &auto_setup).unwrap();
        assert!(displaced.strength.value() < natural.strength.value());
        assert!(displaced.goalkeeping.value() < natural.goalkeeping.value());
    }

    #[test]
    fn encaixe_de_posicao_e_simetrico_e_decrescente_com_a_distancia() {
        use Position::{Defender, Forward, Goalkeeper, Midfielder};
        assert_eq!(position_fit_permille(Defender, Defender), 1000);
        assert!(
            position_fit_permille(Defender, Midfielder) > position_fit_permille(Defender, Forward)
        );
        for a in [Goalkeeper, Defender, Midfielder, Forward] {
            for b in [Goalkeeper, Defender, Midfielder, Forward] {
                assert_eq!(position_fit_permille(a, b), position_fit_permille(b, a));
            }
        }
        assert_eq!(position_fit_permille(Goalkeeper, Forward), 500);
    }

    #[test]
    fn codigos_de_mentalidade_e_pressao_fazem_round_trip() {
        for m in [
            Mentality::Defensive,
            Mentality::Balanced,
            Mentality::Attacking,
        ] {
            assert_eq!(Mentality::from_code(m.code()), Some(m));
        }
        for p in [Pressing::Normal, Pressing::High] {
            assert_eq!(Pressing::from_code(p.code()), Some(p));
        }
        assert_eq!(Mentality::from_code(9), None);
        assert_eq!(Pressing::from_code(9), None);
    }

    #[test]
    fn clube_sem_ninguem_disponivel_nao_tem_perfil() {
        let pack = example_pack();
        let mut roster = roster(&pack);
        let club = ClubId::new(0);
        for p in roster.iter_mut().filter(|p| p.club == club) {
            p.injured = true;
        }
        let setup = ClubSetup::new(club);
        let xi = resolve_xi(&roster, &setup, &HashSet::new());
        assert!(xi.slots.is_empty());
        assert!(profile_for(&pack, &roster, &xi, &setup).is_none());
    }
}
