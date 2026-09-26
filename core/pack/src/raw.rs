//! Estruturas "cruas": espelham exatamente o JSON de um data pack
//! (`docs/03-modelo-de-dados.md §7`), com ids externos em `String` — a forma
//! como um humano escreve o arquivo. `resolve.rs` transforma isto em
//! [`crate::resolved`], onde as referências viram ids densos.

use std::path::Path;

use serde::Deserialize;

use crate::error::PackError;

#[derive(Debug, Clone, Deserialize)]
pub struct RawNation {
    pub id: String,
    pub name: String,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawCompetition {
    pub id: String,
    pub name: String,
    pub nation: String,
    pub tier: u8,
    pub format: RawFormat,
    #[serde(default)]
    pub tiebreakers: Vec<String>,
    #[serde(default)]
    pub promotion: RawMovement,
    #[serde(default)]
    pub relegation: RawMovement,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum RawFormat {
    RoundRobin { legs: u8, teams: u32 },
}

/// Promoção ou rebaixamento — `to: null`/campo ausente é o padrão (nenhum
/// movimento), e é isso que `#[serde(default)]` produz.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct RawMovement {
    #[serde(default)]
    pub to: Option<String>,
    #[serde(default)]
    pub slots: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct RawClub {
    pub id: String,
    pub name: String,
    pub nation: String,
    pub competition: String,
    #[serde(default)]
    pub founded: Option<i32>,
    #[serde(default)]
    pub stadium: Option<String>,
}

/// O conteúdo cru de um pack, ainda com ids em `String` e sem nenhuma
/// validação semântica aplicada.
#[derive(Debug, Default)]
pub struct RawPack {
    pub nations: Vec<RawNation>,
    pub competitions: Vec<RawCompetition>,
    pub clubs: Vec<RawClub>,
}

/// Lê todo `.json` em `<root>/<subdir>`, desserializando cada arquivo como
/// um único `T` (um arquivo = uma entidade — `docs/03 §7`). Arquivos são
/// lidos em ordem alfabética do nome, para que erros de parse sejam
/// reportados numa ordem estável e previsível entre execuções.
fn read_entities<T: for<'de> Deserialize<'de>>(dir: &Path) -> Result<Vec<T>, PackError> {
    if !dir.is_dir() {
        return Ok(Vec::new()); // subpasta ausente = zero entidades, não é erro
    }
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .map_err(|source| PackError::Io {
            path: dir.to_path_buf(),
            source,
        })?
        .filter_map(|entry| entry.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|ext| ext == "json"))
        .collect();
    paths.sort();

    paths
        .into_iter()
        .map(|path| {
            let contents = std::fs::read_to_string(&path).map_err(|source| PackError::Io {
                path: path.clone(),
                source,
            })?;
            serde_json::from_str(&contents)
                .map_err(|source| PackError::InvalidJson { path, source })
        })
        .collect()
}

/// Lê `nations/`, `competitions/` e `clubs/` sob `root`. Cada subpasta é
/// opcional (um pack "só regras", por exemplo, pode não ter clubes).
pub fn load(root: &Path) -> Result<RawPack, PackError> {
    Ok(RawPack {
        nations: read_entities(&root.join("nations"))?,
        competitions: read_entities(&root.join("competitions"))?,
        clubs: read_entities(&root.join("clubs"))?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    #[test]
    fn subpastas_ausentes_viram_listas_vazias() {
        let dir = TempDir::new();
        let raw = load(dir.path()).unwrap();
        assert!(raw.nations.is_empty());
        assert!(raw.competitions.is_empty());
        assert!(raw.clubs.is_empty());
    }

    #[test]
    fn le_nacoes_em_ordem_alfabetica_de_arquivo() {
        let dir = TempDir::new();
        let nations_dir = dir.path().join("nations");
        std::fs::create_dir_all(&nations_dir).unwrap();
        std::fs::write(
            nations_dir.join("z.json"),
            r#"{"id":"zz","name":"Zetalândia"}"#,
        )
        .unwrap();
        std::fs::write(
            nations_dir.join("a.json"),
            r#"{"id":"aa","name":"Alfazenda"}"#,
        )
        .unwrap();

        let raw = load(dir.path()).unwrap();
        assert_eq!(raw.nations.len(), 2);
        assert_eq!(raw.nations[0].id, "aa"); // a.json antes de z.json
        assert_eq!(raw.nations[1].id, "zz");
    }

    #[test]
    fn json_invalido_vira_erro_com_o_caminho_do_arquivo() {
        let dir = TempDir::new();
        let clubs_dir = dir.path().join("clubs");
        std::fs::create_dir_all(&clubs_dir).unwrap();
        std::fs::write(clubs_dir.join("quebrado.json"), "{ isso não é json").unwrap();

        let err = load(dir.path()).unwrap_err();
        match err {
            PackError::InvalidJson { path, .. } => {
                assert!(path.ends_with("quebrado.json"));
            }
            other => panic!("esperava InvalidJson, veio {other:?}"),
        }
    }

    #[test]
    fn formato_round_robin_desserializa_corretamente() {
        let json = r#"{
            "id": "ex.tier1", "name": "Exemplo 1ª Divisão", "nation": "ex", "tier": 1,
            "format": { "type": "round_robin", "legs": 2, "teams": 8 },
            "tiebreakers": ["points", "wins"],
            "relegation": { "to": "ex.tier2", "slots": 2 }
        }"#;
        let comp: RawCompetition = serde_json::from_str(json).unwrap();
        match comp.format {
            RawFormat::RoundRobin { legs, teams } => {
                assert_eq!(legs, 2);
                assert_eq!(teams, 8);
            }
        }
        assert_eq!(comp.relegation.to.as_deref(), Some("ex.tier2"));
        assert_eq!(comp.promotion.to, None); // ausente no JSON -> default
    }
}
