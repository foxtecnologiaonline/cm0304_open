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

/// CA/PA crus — `docs/03 §5`. Diferente do resto da carreira, `pack` não
/// calcula isso a partir de uma tabela de pesos (`ability.toml` ainda não
/// existe, `docs/03 §5`): o pack declara os dois valores diretamente,
/// curados por quem monta o pack, como um ponto de partida de carreira já
/// coerente em vez de um formulário fora do escopo deste marco.
#[derive(Debug, Clone, Deserialize)]
pub struct RawAbility {
    pub current: u8,
    pub potential: u8,
}

/// Um jogador cru — `docs/03 §3` e `§7` (`people/*.json`). `attributes` é um
/// mapa esparso (`nome_snake_case -> 1..=20`); atributos ausentes assumem o
/// piso de `domain::PlayerAttributes::default()` (`ATTR_MIN`), não uma
/// média — evita que um pack preguiçoso pareça ter um jogador mediano em
/// tudo que não declarou.
#[derive(Debug, Clone, Deserialize)]
pub struct RawPlayer {
    pub id: String,
    pub club: String,
    pub nation: String,
    pub first_name: String,
    pub last_name: String,
    /// `"AAAA-MM-DD"` — `docs/03 §3`, `Person.birth`.
    pub birth_date: String,
    /// Posição primária (`"gk" | "df" | "mf" | "fw"`) — ver
    /// `domain::Position` para a simplificação em relação à familiaridade
    /// completa por posição de `docs/03 §3`.
    pub position: String,
    #[serde(default)]
    pub attributes: std::collections::BTreeMap<String, u8>,
    pub ability: RawAbility,
}

/// O conteúdo cru de um pack, ainda com ids em `String` e sem nenhuma
/// validação semântica aplicada.
#[derive(Debug, Default)]
pub struct RawPack {
    pub nations: Vec<RawNation>,
    pub competitions: Vec<RawCompetition>,
    pub clubs: Vec<RawClub>,
    pub players: Vec<RawPlayer>,
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

/// Lê todo `.json` em `<root>/people` como um **lote** de jogadores por
/// arquivo (`Vec<RawPlayer>`), não um-arquivo-uma-entidade como
/// [`read_entities`] — na volumetria alvo de `docs/03 §9` (250 mil
/// jogadores), um arquivo por jogador seria impraticável de navegar; um
/// arquivo por elenco de clube (ou por lote qualquer que o autor do pack
/// preferir) é o formato real. A ordem de leitura dos arquivos continua
/// alfabética, pelo mesmo motivo de determinismo de `read_entities`.
fn read_people(dir: &Path) -> Result<Vec<RawPlayer>, PackError> {
    if !dir.is_dir() {
        return Ok(Vec::new());
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

    let mut players = Vec::new();
    for path in paths {
        let contents = std::fs::read_to_string(&path).map_err(|source| PackError::Io {
            path: path.clone(),
            source,
        })?;
        let batch: Vec<RawPlayer> = serde_json::from_str(&contents)
            .map_err(|source| PackError::InvalidJson { path, source })?;
        players.extend(batch);
    }
    Ok(players)
}

/// Lê `nations/`, `competitions/`, `clubs/` e `people/` sob `root`. Cada
/// subpasta é opcional (um pack "só regras", por exemplo, pode não ter
/// clubes nem jogadores).
pub fn load(root: &Path) -> Result<RawPack, PackError> {
    Ok(RawPack {
        nations: read_entities(&root.join("nations"))?,
        competitions: read_entities(&root.join("competitions"))?,
        clubs: read_entities(&root.join("clubs"))?,
        players: read_people(&root.join("people"))?,
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
        assert!(raw.players.is_empty());
    }

    #[test]
    fn le_jogadores_em_lote_por_arquivo_e_achata_em_ordem_alfabetica() {
        let dir = TempDir::new();
        let people = dir.path().join("people");
        std::fs::create_dir_all(&people).unwrap();
        std::fs::write(
            people.join("ex.b.json"),
            r#"[{
                "id": "ex.b.p01", "club": "ex.b", "nation": "ex",
                "first_name": "Bê", "last_name": "Jogador",
                "birth_date": "2000-01-01", "position": "df",
                "attributes": {"marking": 12},
                "ability": {"current": 100, "potential": 140}
            }]"#,
        )
        .unwrap();
        std::fs::write(
            people.join("ex.a.json"),
            r#"[
                {
                    "id": "ex.a.p01", "club": "ex.a", "nation": "ex",
                    "first_name": "A1", "last_name": "Jogador",
                    "birth_date": "1999-05-20", "position": "gk",
                    "attributes": {"reflexes": 15},
                    "ability": {"current": 110, "potential": 130}
                },
                {
                    "id": "ex.a.p02", "club": "ex.a", "nation": "ex",
                    "first_name": "A2", "last_name": "Jogador",
                    "birth_date": "2002-11-03", "position": "fw",
                    "attributes": {"finishing": 16},
                    "ability": {"current": 95, "potential": 160}
                }
            ]"#,
        )
        .unwrap();

        let raw = load(dir.path()).unwrap();
        assert_eq!(raw.players.len(), 3);
        // ex.a.json antes de ex.b.json -> os dois jogadores de ex.a vêm primeiro.
        assert_eq!(raw.players[0].id, "ex.a.p01");
        assert_eq!(raw.players[1].id, "ex.a.p02");
        assert_eq!(raw.players[2].id, "ex.b.p01");
        assert_eq!(raw.players[0].ability.current, 110);
        assert_eq!(raw.players[0].attributes.get("reflexes"), Some(&15));
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
