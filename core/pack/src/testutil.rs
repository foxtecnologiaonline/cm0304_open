//! Utilitário de teste compartilhado entre os módulos deste crate —
//! um diretório temporário simples, sem puxar uma dependência nova só para
//! isso (nenhum código de produção do crate precisa de `tempfile`).
#![cfg(test)]

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

pub(crate) struct TempDir(PathBuf);

impl TempDir {
    pub(crate) fn new() -> Self {
        // pid + contador atômico é suficiente para nomes únicos mesmo com
        // vários testes rodando em paralelo no mesmo processo (o runner
        // padrão de `cargo test` usa uma thread por teste).
        let unique = COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!(
            "managerfc-pack-test-{}-{unique}",
            std::process::id()
        ));
        std::fs::create_dir_all(&path).expect("criar diretório temporário de teste");
        Self(path)
    }

    pub(crate) fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
