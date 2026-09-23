//! `pack` — Leitura e validação de data packs (.fmpack).
//!
//! Ainda não implementado além do esqueleto: este crate existe para que a
//! estrutura de dependências descrita em `docs/02-arquitetura.md §3` já
//! compile e seja testável desde o M0, mesmo antes de a regra de negócio
//! chegar (ver `docs/07-roadmap.md` para o marco em que cada peça entra).
//! Ver também: docs/03-modelo-de-dados.md §7
#![warn(clippy::all)]

/// Placeholder que prova que o crate compila e está corretamente ligado a
/// `domain` — removido assim que a primeira funcionalidade real chegar.
#[must_use]
pub fn crate_name() -> &'static str {
    "pack"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crate_esta_ligado_e_nomeado_corretamente() {
        assert_eq!(crate_name(), "pack");
    }
}
