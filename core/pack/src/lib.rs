//! `pack` — leitura, resolução de ids e validação de data packs.
//!
//! Um data pack é um diretório (ou, no futuro, um `.fmpack` zipado — ver
//! `docs/03-modelo-de-dados.md §7`) com um `pack.toml` na raiz e subpastas
//! `nations/`, `competitions/`, `clubs/`, cada uma com um arquivo `.json`
//! por entidade. Este crate é a **única** porta de entrada de conteúdo
//! externo no núcleo: nada em `world`/`rules`/`engine` lê arquivo — eles
//! recebem um [`LoadedPack`] já resolvido.
//!
//! Ainda não implementado: `.fmpack` zipado, `people/*.json` (jogadores —
//! entra com o loop de mundo no M1), `rules/*.toml` (pesos do motor —
//! entra com o motor v0/v1), múltiplos packs com resolução de precedência
//! (M5). O que existe aqui já é o suficiente para o portão do M0
//! (`docs/07-roadmap.md#m0--fundação-6-semanas`): carregar e validar "1
//! país de exemplo, 2 divisões" de ponta a ponta.
#![warn(clippy::all)]

mod error;
mod manifest;
mod raw;
mod resolve;
mod resolved;
#[cfg(test)]
mod testutil;

pub use error::PackError;
pub use manifest::PackManifest;
pub use resolved::{
    Format, LoadedPack, Movement, ResolvedClub, ResolvedCompetition, ResolvedNation, Tiebreaker,
};

use std::path::Path;

/// Resultado de carregar e validar um pack: o pack resolvido (que pode ter
/// menos entidades do que os arquivos de origem, se alguma foi descartada
/// por referência inválida) mais a lista de problemas encontrados.
#[derive(Debug)]
pub struct PackReport {
    pub manifest: PackManifest,
    pub pack: LoadedPack,
    pub issues: Vec<String>,
}

impl PackReport {
    /// `true` se nenhum problema foi encontrado — o único sinal em que vale
    /// a pena carregar este pack para uma partida de verdade.
    #[must_use]
    pub fn is_valid(&self) -> bool {
        self.issues.is_empty()
    }
}

/// Carrega e valida um data pack a partir de um diretório.
///
/// Erros de I/O ou de sintaxe (JSON/TOML malformado, `pack.toml` ausente)
/// vêm como `Err(PackError)` — o pack está estruturalmente quebrado demais
/// para sequer tentar validar semanticamente. Qualquer outro problema
/// (referência quebrada, contagem de clubes divergente, critério de
/// desempate desconhecido) aparece em `PackReport::issues`, sempre com o
/// pack parcial que deu para montar — é isso que permite ao autor do pack
/// ver todos os problemas de uma vez em vez de corrigir um por um
/// (`docs/03 §7`).
pub fn load_and_validate(root: &Path) -> Result<PackReport, PackError> {
    let manifest = manifest::load(root)?;
    let raw = raw::load(root)?;
    let (pack, issues) = resolve::resolve(raw);
    Ok(PackReport {
        manifest,
        pack,
        issues,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::TempDir;

    /// Monta, em disco, o pack de exemplo mínimo que representa o portão do
    /// M0: 1 país, 2 divisões, com promoção/rebaixamento entre elas.
    fn write_example_pack(dir: &Path) {
        std::fs::write(
            dir.join("pack.toml"),
            r#"
                id = "example.two-tier"
                name = "Exemplo — Duas Divisões"
                version = "0.1.0"
                license = "CC0-1.0"
            "#,
        )
        .unwrap();

        let nations = dir.join("nations");
        std::fs::create_dir_all(&nations).unwrap();
        std::fs::write(
            nations.join("ex.json"),
            r#"{"id":"ex","name":"Exemplolândia"}"#,
        )
        .unwrap();

        let competitions = dir.join("competitions");
        std::fs::create_dir_all(&competitions).unwrap();
        std::fs::write(
            competitions.join("tier1.json"),
            r#"{
                "id": "ex.tier1", "name": "Primeira Divisão", "nation": "ex", "tier": 1,
                "format": { "type": "round_robin", "legs": 2, "teams": 2 },
                "tiebreakers": ["points", "wins", "goal_difference"],
                "relegation": { "to": "ex.tier2", "slots": 1 }
            }"#,
        )
        .unwrap();
        std::fs::write(
            competitions.join("tier2.json"),
            r#"{
                "id": "ex.tier2", "name": "Segunda Divisão", "nation": "ex", "tier": 2,
                "format": { "type": "round_robin", "legs": 2, "teams": 2 },
                "tiebreakers": ["points"],
                "promotion": { "to": "ex.tier1", "slots": 1 }
            }"#,
        )
        .unwrap();

        let clubs = dir.join("clubs");
        std::fs::create_dir_all(&clubs).unwrap();
        for (id, name, competition) in [
            ("ex.a", "Clube A", "ex.tier1"),
            ("ex.b", "Clube B", "ex.tier1"),
            ("ex.c", "Clube C", "ex.tier2"),
            ("ex.d", "Clube D", "ex.tier2"),
        ] {
            std::fs::write(
                clubs.join(format!("{id}.json")),
                format!(
                    r#"{{"id":"{id}","name":"{name}","nation":"ex","competition":"{competition}"}}"#
                ),
            )
            .unwrap();
        }
    }

    #[test]
    fn pack_de_exemplo_carrega_e_valida_sem_nenhum_problema() {
        let dir = TempDir::new();
        write_example_pack(dir.path());

        let report = load_and_validate(dir.path()).unwrap();

        assert!(
            report.is_valid(),
            "problemas inesperados: {:?}",
            report.issues
        );
        assert_eq!(report.manifest.id, "example.two-tier");
        assert_eq!(report.pack.nations.len(), 1);
        assert_eq!(report.pack.competitions.len(), 2);
        assert_eq!(report.pack.clubs.len(), 4);

        let tier1 = report
            .pack
            .competitions
            .iter()
            .find(|c| c.external_id == "ex.tier1")
            .expect("tier1 presente");
        assert_eq!(report.pack.clubs_in(tier1.id).len(), 2);
        assert!(tier1.relegation.to.is_some());
    }

    #[test]
    fn manifesto_ausente_e_erro_estrutural_nao_um_issue() {
        let dir = TempDir::new();
        // Sem pack.toml — nem chega a tentar ler nations/competitions/clubs.
        let err = load_and_validate(dir.path()).unwrap_err();
        assert!(matches!(err, PackError::MissingManifest { .. }));
    }
}
