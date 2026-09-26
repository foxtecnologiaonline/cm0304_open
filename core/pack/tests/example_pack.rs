//! Teste de integração: o pack de exemplo em `packs/core/example-two-tier`
//! (commitado no repositório, não gerado em memória) precisa continuar
//! carregando **sem nenhum problema** conforme o crate `pack` evolui.
//!
//! Esta fixture é o portão do M0 (`docs/07-roadmap.md#m0--fundação-6-semanas`:
//! "carregador de data pack + validador + 1 país de exemplo, 2 divisões") e
//! a base de futuros testes de `world`/`rules` no M1 — se ela quebrar aqui,
//! quebra silenciosamente em todo lugar que a reusar.

use std::path::PathBuf;

fn example_pack_path() -> PathBuf {
    // `CARGO_MANIFEST_DIR` = .../core/pack ; o pack de exemplo vive em
    // .../packs/core/example-two-tier, na raiz do repositório.
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/example-two-tier")
}

#[test]
fn pack_de_exemplo_do_repositorio_carrega_sem_problemas() {
    let path = example_pack_path();
    assert!(
        path.is_dir(),
        "pack de exemplo não encontrado em {} — foi movido ou renomeado?",
        path.display()
    );

    let report = pack::load_and_validate(&path)
        .unwrap_or_else(|e| panic!("falha estrutural ao carregar {}: {e}", path.display()));

    assert!(
        report.is_valid(),
        "pack de exemplo do repositório tem {} problema(s): {:#?}",
        report.issues.len(),
        report.issues
    );
    assert_eq!(report.manifest.id, "example.two-tier");
    assert_eq!(report.pack.nations.len(), 1);
    assert_eq!(report.pack.competitions.len(), 2);
    assert_eq!(report.pack.clubs.len(), 16);

    // As duas divisões têm o número de clubes que declaram, e promoção/
    // rebaixamento se referenciam mutuamente (é o "2 divisões" do gate do M0).
    let tier1 = report
        .pack
        .competitions
        .iter()
        .find(|c| c.external_id == "es.tier1")
        .unwrap();
    let tier2 = report
        .pack
        .competitions
        .iter()
        .find(|c| c.external_id == "es.tier2")
        .unwrap();
    assert_eq!(report.pack.clubs_in(tier1.id).len(), 8);
    assert_eq!(report.pack.clubs_in(tier2.id).len(), 8);
    assert_eq!(tier1.relegation.to, Some(tier2.id));
    assert_eq!(tier2.promotion.to, Some(tier1.id));
}
