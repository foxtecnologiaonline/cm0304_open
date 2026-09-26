//! `pack.toml` — o manifesto obrigatório de todo data pack (`docs/03 §7`).

use std::path::Path;

use serde::Deserialize;

use crate::error::PackError;

/// Manifesto de um data pack: identidade, versão e licença.
#[derive(Debug, Clone, Deserialize)]
pub struct PackManifest {
    pub id: String,
    pub name: String,
    pub version: String,
    /// Licença **dos dados** deste pack — pode (e deve, para packs de
    /// conteúdo real) diferir da licença do código (`docs/05 §5.2`).
    pub license: String,
    /// Ids de outros packs que este depende, na ordem de precedência
    /// (`docs/03 §7`). Vazio por padrão — a maioria dos packs é autônoma.
    #[serde(default)]
    pub dependencies: Vec<String>,
}

/// Lê e desserializa `<root>/pack.toml`.
pub fn load(root: &Path) -> Result<PackManifest, PackError> {
    let path = root.join("pack.toml");
    if !path.is_file() {
        return Err(PackError::MissingManifest {
            path: root.to_path_buf(),
        });
    }
    let contents = std::fs::read_to_string(&path).map_err(|source| PackError::Io {
        path: path.clone(),
        source,
    })?;
    toml::from_str(&contents).map_err(|source| PackError::InvalidManifest { path, source })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    #[test]
    fn le_manifesto_valido() {
        let dir = TempDir::new();
        std::fs::write(
            dir.path().join("pack.toml"),
            r#"
                id = "example.two-tier"
                name = "Exemplo — Duas Divisões"
                version = "0.1.0"
                license = "CC0-1.0"
            "#,
        )
        .unwrap();

        let manifest = load(dir.path()).unwrap();
        assert_eq!(manifest.id, "example.two-tier");
        assert_eq!(manifest.version, "0.1.0");
        assert!(manifest.dependencies.is_empty());
    }

    #[test]
    fn erro_claro_quando_manifesto_ausente() {
        let dir = TempDir::new();
        let err = load(dir.path()).unwrap_err();
        assert!(matches!(err, PackError::MissingManifest { .. }));
    }

    #[test]
    fn erro_claro_quando_manifesto_invalido() {
        let dir = TempDir::new();
        std::fs::write(dir.path().join("pack.toml"), "isso não é toml válido = [[[").unwrap();
        let err = load(dir.path()).unwrap_err();
        assert!(matches!(err, PackError::InvalidManifest { .. }));
    }
}
