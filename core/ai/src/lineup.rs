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

/// Formação fixa e simplificada: 1 GK, 4 DF, 4 MF, 2 FW = 11. É a que a IA
/// de clube usa sempre ([`Formation::F442`]); o técnico humano escolhe
/// entre as de [`Formation::ALL`].
pub const FORMATION: [(Position, usize); 4] = [
    (Position::Goalkeeper, 1),
    (Position::Defender, 4),
    (Position::Midfielder, 4),
    (Position::Forward, 2),
];

/// Formações disponíveis ao técnico (RF-PA-01, `docs/01 §2.5`): quantos
/// defensores, meio-campistas e atacantes jogam à frente do goleiro. Só a
/// distribuição por linha — o motor v0.5 não tem zonas nem posições finas
/// (lateral, volante, ponta: `docs/04 §2.2`, M3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Formation {
    F442,
    F433,
    F352,
    F451,
    F532,
}

impl Formation {
    pub const ALL: [Formation; 5] = [
        Formation::F442,
        Formation::F433,
        Formation::F352,
        Formation::F451,
        Formation::F532,
    ];

    /// `(defensores, meio-campistas, atacantes)`; a soma é sempre 10.
    #[must_use]
    pub const fn lines(self) -> (usize, usize, usize) {
        match self {
            Formation::F442 => (4, 4, 2),
            Formation::F433 => (4, 3, 3),
            Formation::F352 => (3, 5, 2),
            Formation::F451 => (4, 5, 1),
            Formation::F532 => (5, 3, 2),
        }
    }

    /// Posição de cada um dos 11 espaços, na ordem de escalação: goleiro,
    /// defensores, meio-campistas, atacantes.
    #[must_use]
    pub fn slots(self) -> [Position; 11] {
        let (d, m, _f) = self.lines();
        let mut slots = [Position::Forward; 11];
        slots[0] = Position::Goalkeeper;
        for (i, slot) in slots.iter_mut().enumerate().skip(1) {
            *slot = if i <= d {
                Position::Defender
            } else if i <= d + m {
                Position::Midfielder
            } else {
                Position::Forward
            };
        }
        slots
    }

    /// Nome de exibição/chave de localização, ex.: `"4-4-2"`.
    #[must_use]
    pub const fn key(self) -> &'static str {
        match self {
            Formation::F442 => "4-4-2",
            Formation::F433 => "4-3-3",
            Formation::F352 => "3-5-2",
            Formation::F451 => "4-5-1",
            Formation::F532 => "5-3-2",
        }
    }

    /// Código estável de 1 byte (log de comandos do save): **nunca
    /// renumerar**.
    #[must_use]
    pub const fn code(self) -> u8 {
        match self {
            Formation::F442 => 0,
            Formation::F433 => 1,
            Formation::F352 => 2,
            Formation::F451 => 3,
            Formation::F532 => 4,
        }
    }

    #[must_use]
    pub const fn from_code(code: u8) -> Option<Self> {
        match code {
            0 => Some(Formation::F442),
            1 => Some(Formation::F433),
            2 => Some(Formation::F352),
            3 => Some(Formation::F451),
            4 => Some(Formation::F532),
            _ => None,
        }
    }
}

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
    select_starting_eleven_for(squad, Formation::F442)
}

/// [`select_starting_eleven`] para uma [`Formation`] qualquer: os titulares
/// saem na ordem dos espaços da formação (goleiro, defensores,
/// meio-campistas, atacantes), cada linha do melhor pro pior. Com
/// [`Formation::F442`] é exatamente [`select_starting_eleven`].
#[must_use]
pub fn select_starting_eleven_for(squad: &[PlayerRating], formation: Formation) -> Vec<PlayerId> {
    let (d, m, f) = formation.lines();
    let lines = [
        (Position::Goalkeeper, 1),
        (Position::Defender, d),
        (Position::Midfielder, m),
        (Position::Forward, f),
    ];
    let mut selected = Vec::with_capacity(11);
    for &(position, slots) in &lines {
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
    fn toda_formacao_soma_onze_e_tem_um_goleiro() {
        for formation in Formation::ALL {
            let (d, m, f) = formation.lines();
            assert_eq!(1 + d + m + f, 11, "{}", formation.key());
            let slots = formation.slots();
            assert_eq!(slots[0], Position::Goalkeeper);
            let count = |p| slots.iter().filter(|&&s| s == p).count();
            assert_eq!(count(Position::Goalkeeper), 1);
            assert_eq!(count(Position::Defender), d);
            assert_eq!(count(Position::Midfielder), m);
            assert_eq!(count(Position::Forward), f);
        }
    }

    #[test]
    fn codigo_de_formacao_faz_round_trip_e_rejeita_desconhecido() {
        for formation in Formation::ALL {
            assert_eq!(Formation::from_code(formation.code()), Some(formation));
        }
        assert_eq!(Formation::from_code(99), None);
    }

    #[test]
    fn f442_e_identico_a_selecao_padrao() {
        let mut squad = vec![rating(0, Position::Goalkeeper, 100)];
        squad.extend((1..=6).map(|i| rating(i, Position::Defender, 60 + i as u8)));
        squad.extend((7..=12).map(|i| rating(i, Position::Midfielder, 60 + i as u8)));
        squad.extend((13..=16).map(|i| rating(i, Position::Forward, 60 + i as u8)));
        assert_eq!(
            select_starting_eleven_for(&squad, Formation::F442),
            select_starting_eleven(&squad)
        );
    }

    #[test]
    fn formacao_433_escala_tres_atacantes_e_tres_meias() {
        let mut squad = vec![rating(0, Position::Goalkeeper, 100)];
        squad.extend((1..=6).map(|i| rating(i, Position::Defender, 80)));
        squad.extend((7..=12).map(|i| rating(i, Position::Midfielder, 80)));
        squad.extend((13..=17).map(|i| rating(i, Position::Forward, 80)));
        let starters = select_starting_eleven_for(&squad, Formation::F433);
        assert_eq!(starters.len(), 11);
        let count = |p: Position| {
            starters
                .iter()
                .filter(|id| squad[id.as_usize()].position == p)
                .count()
        };
        assert_eq!(count(Position::Defender), 4);
        assert_eq!(count(Position::Midfielder), 3);
        assert_eq!(count(Position::Forward), 3);
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
