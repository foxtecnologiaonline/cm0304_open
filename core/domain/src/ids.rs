//! Identificadores densos do domínio.
//!
//! Todos os ids são `u32` de índice denso — atribuídos na carga do data pack,
//! nunca derivados de string em caminho quente (ver `docs/03-modelo-de-dados.md §2`).
//! Cada tipo é um newtype para que o compilador impeça trocar `PlayerId` por
//! `ClubId` por engano em qualquer assinatura de função.

use std::fmt;

/// Declara um novo tipo de id denso (`u32`) com as conversões e traits padrão.
macro_rules! dense_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
        pub struct $name(u32);

        impl $name {
            /// Constrói o id a partir de um índice denso já resolvido pelo carregador de pack.
            #[must_use]
            pub const fn new(index: u32) -> Self {
                Self(index)
            }

            /// Índice denso subjacente — uso interno (SoA, serialização).
            #[must_use]
            pub const fn index(self) -> u32 {
                self.0
            }

            /// Índice denso como `usize`, para indexar diretamente em `Vec`.
            #[must_use]
            pub const fn as_usize(self) -> usize {
                self.0 as usize
            }
        }

        impl fmt::Display for $name {
            fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                write!(f, "{}#{}", stringify!($name), self.0)
            }
        }

        impl From<u32> for $name {
            fn from(index: u32) -> Self {
                Self(index)
            }
        }
    };
}

dense_id!(
    /// Id de uma pessoa (jogador ou membro de comissão técnica) — ver `docs/03 §3`.
    PersonId
);
dense_id!(
    /// Id de um jogador (uma `Person` com perfil de atleta).
    PlayerId
);
dense_id!(
    /// Id de um clube.
    ClubId
);
dense_id!(
    /// Id de um país/associação.
    NationId
);
dense_id!(
    /// Id de uma competição (liga ou copa).
    CompetitionId
);
dense_id!(
    /// Id de uma edição de competição (`SeasonId` no ER de `docs/03 §1`).
    SeasonId
);
dense_id!(
    /// Id de uma partida agendada no calendário.
    FixtureId
);
dense_id!(
    /// Id de um contrato de trabalho (jogador ou staff).
    ContractId
);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_diferentes_nao_sao_intercambiaveis_em_tempo_de_compilacao() {
        // Este teste é, sobretudo, documentação executável: PlayerId e ClubId
        // não implementam From um do outro, então o código abaixo não
        // compilaria se alguém tentasse `let c: ClubId = player_id;`.
        let player = PlayerId::new(7);
        let club = ClubId::new(7);
        assert_eq!(player.index(), club.index());
        assert_eq!(player.to_string(), "PlayerId#7");
        assert_eq!(club.to_string(), "ClubId#7");
    }

    #[test]
    fn index_denso_e_preservado() {
        let id = FixtureId::new(12_345);
        assert_eq!(id.index(), 12_345);
        assert_eq!(id.as_usize(), 12_345usize);
    }
}
