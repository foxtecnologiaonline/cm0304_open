//! Caixa de entrada do técnico (RF-CA-01/07, `docs/01 §2.8`): a fila de
//! eventos que a UI mostra em ordem — resultado da rodada, lesões, suspensões,
//! propostas, contratos a vencer, janela de transferências, fim de temporada.
//!
//! O núcleo devolve **dados estruturados**, nunca texto traduzido
//! ([`InboxKind`] carrega ids; a UI resolve nomes com as consultas e monta
//! a frase localizada). A caixa só existe com um clube sob comando; fica
//! limitada a [`INBOX_CAPACITY`] mensagens (as mais antigas saem) para não
//! crescer sem limite numa carreira longa no celular.
//!
//! Estado "lida/não lida" muda por comando ([`crate::Command::MarkInboxRead`],
//! [`crate::Command::MarkAllInboxRead`]) — assim entra no log do save e o
//! replay reproduz a caixa exatamente.

use std::collections::VecDeque;

use domain::{ClubId, Money, PlayerId};

/// Quantas mensagens a caixa guarda.
pub const INBOX_CAPACITY: usize = 200;

/// O que aconteceu.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum InboxKind {
    /// Pré-temporada feita: folha paga, mercado da IA rodou, calendário
    /// montado.
    SeasonStarted,
    /// Resultado da rodada para o clube sob comando.
    MatchPlayed {
        home: ClubId,
        away: ClubId,
        home_goals: u32,
        away_goals: u32,
    },
    /// Jogadores seus lesionados nesta temporada (sorteio da pré-temporada).
    PlayersInjured {
        count: u16,
    },
    /// Jogador seu suspenso na próxima rodada.
    PlayerSuspended {
        player: PlayerId,
    },
    /// Titular escolhido indisponível na rodada: `incoming` entrou no lugar.
    LineupAdjusted {
        out: PlayerId,
        incoming: PlayerId,
    },
    /// Proposta da IA por um jogador seu (`Command::RespondToOffer`).
    OfferReceived {
        offer: u32,
        player: PlayerId,
        from: ClubId,
        fee: Money,
    },
    /// Jogadores seus com um ano de contrato ou menos.
    ContractsExpiring {
        count: u16,
    },
    TransferWindowOpened,
    TransferWindowClosed,
    /// O clube está devendo (folha que o caixa não cobriu).
    DebtWarning {
        debt: Money,
    },
    /// Fim de temporada: posição final na liga e o que mudou.
    SeasonEnded {
        position: u32,
        clubs: u32,
        promoted: bool,
        relegated: bool,
    },
}

impl InboxKind {
    /// `true` se pede uma decisão do técnico (a UI destaca; "processar
    /// tudo" para aqui).
    #[must_use]
    pub const fn needs_action(self) -> bool {
        matches!(
            self,
            InboxKind::OfferReceived { .. }
                | InboxKind::ContractsExpiring { .. }
                | InboxKind::PlayerSuspended { .. }
                | InboxKind::LineupAdjusted { .. }
        )
    }
}

/// Uma mensagem da caixa de entrada.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InboxMessage {
    /// Crescente por carreira; nunca reutilizado.
    pub id: u32,
    pub season_index: u32,
    /// Próxima rodada a jogar quando a mensagem foi gerada.
    pub matchday: u32,
    pub kind: InboxKind,
    pub read: bool,
}

#[derive(Debug, Default)]
pub(crate) struct Inbox {
    messages: VecDeque<InboxMessage>,
    next_id: u32,
}

impl Inbox {
    pub(crate) fn push(&mut self, season_index: u32, matchday: u32, kind: InboxKind) {
        self.messages.push_back(InboxMessage {
            id: self.next_id,
            season_index,
            matchday,
            kind,
            read: false,
        });
        self.next_id += 1;
        while self.messages.len() > INBOX_CAPACITY {
            self.messages.pop_front();
        }
    }

    /// `false` se a mensagem não existe (já saiu da caixa ou id inválido).
    pub(crate) fn mark_read(&mut self, id: u32) -> bool {
        match self.messages.iter_mut().find(|m| m.id == id) {
            Some(message) => {
                message.read = true;
                true
            }
            None => false,
        }
    }

    pub(crate) fn mark_all_read(&mut self) {
        for message in &mut self.messages {
            message.read = true;
        }
    }

    pub(crate) fn unread(&self) -> usize {
        self.messages.iter().filter(|m| !m.read).count()
    }

    /// Mais novas primeiro.
    pub(crate) fn newest_first(&self) -> impl Iterator<Item = &InboxMessage> {
        self.messages.iter().rev()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_sao_crescentes_e_nunca_reutilizados_mesmo_apos_o_limite() {
        let mut inbox = Inbox::default();
        for i in 0..(INBOX_CAPACITY as u32 + 50) {
            inbox.push(0, i, InboxKind::SeasonStarted);
        }
        assert_eq!(inbox.messages.len(), INBOX_CAPACITY);
        let ids: Vec<u32> = inbox.newest_first().map(|m| m.id).collect();
        assert_eq!(ids[0], INBOX_CAPACITY as u32 + 49);
        assert!(ids.windows(2).all(|w| w[0] > w[1]));
        assert_eq!(*ids.last().unwrap(), 50, "as 50 mais antigas saíram");
    }

    #[test]
    fn marcar_como_lida_conta_e_ignora_id_que_nao_existe() {
        let mut inbox = Inbox::default();
        inbox.push(0, 0, InboxKind::SeasonStarted);
        inbox.push(0, 1, InboxKind::TransferWindowClosed);
        assert_eq!(inbox.unread(), 2);
        assert!(inbox.mark_read(0));
        assert_eq!(inbox.unread(), 1);
        assert!(!inbox.mark_read(99));
        inbox.mark_all_read();
        assert_eq!(inbox.unread(), 0);
    }

    #[test]
    fn so_o_que_pede_decisao_exige_acao() {
        assert!(InboxKind::ContractsExpiring { count: 3 }.needs_action());
        assert!(
            InboxKind::OfferReceived {
                offer: 1,
                player: PlayerId::new(1),
                from: ClubId::new(1),
                fee: Money::ZERO
            }
            .needs_action()
        );
        assert!(!InboxKind::SeasonStarted.needs_action());
        assert!(!InboxKind::TransferWindowOpened.needs_action());
    }
}
