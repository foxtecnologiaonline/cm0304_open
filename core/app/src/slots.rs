//! Convenção de nomes de arquivo para slots de save (`docs/00-escopo.md §7`,
//! critério de MVP: "save cross-platform + autosave + 3 slots").
//!
//! Isto é só a convenção de nome — a tela de "carregar/salvar" que lista os
//! 3 slots pertence à UI (bloqueada neste ambiente por falta do SDK
//! Flutter/Dart, `app/README.md`). O que mora aqui é headless e testável:
//! dado um diretório de saves, qual arquivo é o slot N, qual é o autosave.

use std::path::{Path, PathBuf};

/// Quantidade de slots de save manuais — RF de `docs/00 §7`.
pub const SAVE_SLOT_COUNT: u8 = 3;

/// Caminho do slot `slot` (`0..SAVE_SLOT_COUNT`) dentro de `dir`. Um `slot`
/// fora da faixa ainda produz um caminho válido (`slot-7.cm0304save`) — não
/// há nada de errado em *ler* um arquivo assim, só a UI não deveria
/// oferecer escrever nele; por isso isto não devolve `Option`/`Result`.
#[must_use]
pub fn slot_path(dir: &Path, slot: u8) -> PathBuf {
    dir.join(format!("slot-{slot}.cm0304save"))
}

/// Caminho do autosave dentro de `dir` — sempre um único arquivo,
/// sobrescrito a cada avanço (a "N gerações" de `docs/02 §8.1` é dívida
/// técnica aceita: implementar rotação de gerações só quando houver um
/// consumidor de verdade para testar contra).
#[must_use]
pub fn autosave_path(dir: &Path) -> PathBuf {
    dir.join("autosave.cm0304save")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slots_diferentes_produzem_caminhos_diferentes() {
        let dir = Path::new("/tmp/saves");
        let paths: Vec<_> = (0..SAVE_SLOT_COUNT).map(|s| slot_path(dir, s)).collect();
        for i in 0..paths.len() {
            for j in (i + 1)..paths.len() {
                assert_ne!(paths[i], paths[j]);
            }
        }
    }

    #[test]
    fn autosave_nunca_colide_com_um_slot_numerado() {
        let dir = Path::new("/tmp/saves");
        let autosave = autosave_path(dir);
        for slot in 0..SAVE_SLOT_COUNT {
            assert_ne!(autosave, slot_path(dir, slot));
        }
    }
}
