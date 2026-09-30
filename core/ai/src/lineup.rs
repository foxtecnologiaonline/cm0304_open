//! Escalação automática — a primeira fatia de `ai` (`docs/07-roadmap.md`
//! M1: "IA de escalação"). Um subconjunto pequeno de RF-PA-01
//! (`docs/01 §2.5`: "escalação com formação, posições, capitão,
//! cobradores") — aqui não há escolha de formação, capitão nem cobrador,
//! só "quem joga".
//!
//! Decide **quem joga**, não **como joga**: sem tática, marcação, plano B
//! (isso é M3, `docs/04 §2.1`) — só os 11 de maior habilidade atual por
//! posição, numa formação fixa. `ai` depende só de `domain`
//! (`docs/02-arquitetura.md §2`: "motor, IA e mundo dependem de domínio —
//! nunca o inverso"; é `world` quem depende de `ai`, não o contrário), então
//! este módulo não sabe nada sobre `pack::ResolvedPlayer` nem
//! `world::PlayerState` — quem chama monta um [`PlayerRating`] a partir do
//! que tiver (pack estático ou roster de carreira) e traduz o resultado de
//! volta.

use domain::{PlayerId, Position};

/// O suficiente de um jogador para a IA de escalação decidir — nenhum outro
/// atributo (moral, condição, forma) importa ainda, porque nenhum existe.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlayerRating {
    pub player: PlayerId,
    pub position: Position,
    pub current_ability: u8,
}

/// Formação fixa e simplificada: 1 GK, 4 DF, 4 MF, 2 FW = 11. Não é uma
/// escolha tática (isso pede o motor v1 entender zonas, `docs/04 §2.2`) —
/// é só uma distribuição plausível de posições pra "quem joga" fazer
/// sentido sem uma única linha de configuração ainda.
pub const FORMATION: [(Position, usize); 4] = [
    (Position::Goalkeeper, 1),
    (Position::Defender, 4),
    (Position::Midfielder, 4),
    (Position::Forward, 2),
];

/// Seleciona os titulares de `squad`: para cada posição de [`FORMATION`],
/// os jogadores daquela posição com maior `current_ability`, até o número
/// de vagas. Empate de habilidade é resolvido por `PlayerId` crescente —
/// determinístico (`ADR 0002`) independente da ordem em que `squad` chega.
///
/// Um elenco menor que 11 (ou sem jogadores de alguma posição) não é erro:
/// devolve o que der para escalar, nunca menos que isso nem um `panic`. Um
/// elenco vazio devolve uma lista vazia.
#[must_use]
pub fn select_starting_eleven(squad: &[PlayerRating]) -> Vec<PlayerId> {
    let mut selected = Vec::with_capacity(11);
    for &(position, slots) in &FORMATION {
        let mut candidates: Vec<&PlayerRating> =
            squad.iter().filter(|p| p.position == position).collect();
        candidates.sort_by(|a, b| {
            b.current_ability
                .cmp(&a.current_ability)
                .then(a.player.cmp(&b.player))
        });
        selected.extend(candidates.into_iter().take(slots).map(|p| p.player));
    }
    selected
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rating(id: u32, position: Position, ca: u8) -> PlayerRating {
        PlayerRating {
            player: PlayerId::new(id),
            position,
            current_ability: ca,
        }
    }

    #[test]
    fn elenco_vazio_devolve_lista_vazia() {
        assert!(select_starting_eleven(&[]).is_empty());
    }

    #[test]
    fn seleciona_o_time_completo_quando_o_elenco_bate_exatamente_a_formacao() {
        let mut squad = vec![rating(0, Position::Goalkeeper, 100)];
        squad.extend((1..=4).map(|i| rating(i, Position::Defender, 100)));
        squad.extend((5..=8).map(|i| rating(i, Position::Midfielder, 100)));
        squad.extend((9..=10).map(|i| rating(i, Position::Forward, 100)));

        let starters = select_starting_eleven(&squad);
        assert_eq!(starters.len(), 11);
        // Todo mundo do elenco (11 jogadores, formação exata) tem que jogar.
        let mut ids: Vec<u32> = starters.iter().map(|p| p.index()).collect();
        ids.sort_unstable();
        assert_eq!(ids, (0..=10).collect::<Vec<_>>());
    }

    #[test]
    fn escala_os_de_maior_habilidade_por_posicao() {
        let squad = vec![
            rating(0, Position::Goalkeeper, 100),
            rating(1, Position::Midfielder, 80),
            rating(2, Position::Midfielder, 150), // melhor -> titular
            rating(3, Position::Midfielder, 90),
            rating(4, Position::Midfielder, 140), // segundo melhor -> titular
            rating(5, Position::Midfielder, 60),  // banco (só 4 vagas de MF)
        ];
        let starters = select_starting_eleven(&squad);
        assert!(starters.contains(&PlayerId::new(2)));
        assert!(starters.contains(&PlayerId::new(4)));
        assert!(!starters.contains(&PlayerId::new(5)));
    }

    #[test]
    fn empate_de_habilidade_e_desempatado_por_player_id_de_forma_deterministica() {
        let squad = vec![
            rating(9, Position::Forward, 100),
            rating(3, Position::Forward, 100),
            rating(7, Position::Forward, 100),
        ]; // formação pede só 2 de ataque; os 3 empatam em CA.
        let starters = select_starting_eleven(&squad);
        assert_eq!(starters, vec![PlayerId::new(3), PlayerId::new(7)]);
    }

    #[test]
    fn elenco_incompleto_escala_o_que_tem_sem_panicar() {
        let squad = vec![rating(0, Position::Midfielder, 100)];
        let starters = select_starting_eleven(&squad);
        assert_eq!(starters, vec![PlayerId::new(0)]);
    }

    #[test]
    fn resultado_nao_depende_da_ordem_de_entrada() {
        let a = vec![
            rating(0, Position::Forward, 90),
            rating(1, Position::Forward, 120),
        ];
        let mut b = a.clone();
        b.reverse();
        assert_eq!(select_starting_eleven(&a), select_starting_eleven(&b));
    }
}

/// Testes de propriedade (`docs/08 §1`).
#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;
    use std::collections::BTreeSet;

    fn arbitrary_position() -> impl Strategy<Value = Position> {
        prop_oneof![
            Just(Position::Goalkeeper),
            Just(Position::Defender),
            Just(Position::Midfielder),
            Just(Position::Forward),
        ]
    }

    /// Elenco arbitrário com **ids de jogador únicos** — a precondição real
    /// de quem chama `select_starting_eleven` (`world` monta isto a partir
    /// de `pack.players_of(club)`, onde cada `PlayerId` já é único por
    /// construção). Ids duplicados são erro de quem chama, não algo que a
    /// função precise tratar — por isso a estratégia nunca os gera.
    fn arbitrary_squad() -> impl Strategy<Value = Vec<PlayerRating>> {
        prop::collection::hash_set(0u32..50, 0..40)
            .prop_flat_map(|ids| {
                let len = ids.len();
                (
                    Just(ids),
                    prop::collection::vec((arbitrary_position(), any::<u8>()), len),
                )
            })
            .prop_map(|(ids, rest)| {
                ids.into_iter()
                    .zip(rest)
                    .map(|(id, (position, current_ability))| PlayerRating {
                        player: PlayerId::new(id),
                        position,
                        current_ability,
                    })
                    .collect()
            })
    }

    proptest! {
        /// Nunca escala o mesmo jogador duas vezes, nunca escala mais de 11,
        /// e nunca panica — para qualquer elenco, de qualquer tamanho.
        #[test]
        fn selecao_e_sempre_valida_para_qualquer_elenco(squad in arbitrary_squad()) {
            let starters = select_starting_eleven(&squad);

            prop_assert!(starters.len() <= 11);
            let unique: BTreeSet<_> = starters.iter().collect();
            prop_assert_eq!(unique.len(), starters.len(), "jogador escalado mais de uma vez");
        }

        /// `select_starting_eleven` é puro: a mesma entrada sempre produz a
        /// mesma escalação.
        #[test]
        fn selecao_e_deterministica(squad in arbitrary_squad()) {
            prop_assert_eq!(select_starting_eleven(&squad), select_starting_eleven(&squad));
        }
    }
}
