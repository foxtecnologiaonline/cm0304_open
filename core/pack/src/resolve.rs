//! Resolução de ids externos (`String`) para ids densos, e a validação
//! semântica do pack (`docs/03-modelo-de-dados.md §7`).
//!
//! As duas coisas acontecem juntas porque a maioria das validações *é*
//! sobre referências: "este clube aponta para um país que existe?" só
//! responde depois que os países já viraram um índice consultável. O
//! resultado nunca é um `Result` de tudo-ou-nada — é sempre um
//! [`crate::LoadedPack`] (possivelmente com menos entidades que os arquivos
//! de origem) mais uma lista de problemas. Ver `docs/03 §7`: "validador
//! obrigatório... com mensagens de erro úteis", no plural — o objetivo é
//! que o autor do pack veja todos os problemas de uma vez, não um por vez.

use std::collections::BTreeMap;

use domain::{ClubId, CompetitionId, NationId};

use crate::raw::{RawClub, RawCompetition, RawFormat, RawMovement, RawNation, RawPack};
use crate::resolved::{
    Format, LoadedPack, Movement, ResolvedClub, ResolvedCompetition, ResolvedNation, Tiebreaker,
};

/// Resolve um [`RawPack`] em um [`LoadedPack`] + lista de problemas
/// encontrados. Ids são atribuídos **em ordem alfabética do id externo**,
/// não na ordem de leitura dos arquivos — isso torna a atribuição de ids
/// densos determinística e independente da ordem em que o sistema de
/// arquivos lista os diretórios (`ADR 0002`: mesmo pack, mesmos ids,
/// sempre, em qualquer plataforma).
#[must_use]
pub fn resolve(raw: RawPack) -> (LoadedPack, Vec<String>) {
    let mut issues = Vec::new();

    let (nations, nation_index) = resolve_nations(raw.nations, &mut issues);
    let (competitions, competition_index) =
        resolve_competitions(raw.competitions, &nation_index, &mut issues);
    let clubs = resolve_clubs(raw.clubs, &nation_index, &competition_index, &mut issues);

    check_competition_club_counts(&competitions, &clubs, &mut issues);

    (
        LoadedPack {
            nations,
            competitions,
            clubs,
        },
        issues,
    )
}

fn resolve_nations(
    mut raw: Vec<RawNation>,
    issues: &mut Vec<String>,
) -> (Vec<ResolvedNation>, BTreeMap<String, NationId>) {
    raw.sort_by(|a, b| a.id.cmp(&b.id));
    let mut nations = Vec::with_capacity(raw.len());
    let mut index = BTreeMap::new();

    for rec in raw {
        if index.contains_key(&rec.id) {
            issues.push(format!(
                "nação duplicada: id '{}' aparece em mais de um arquivo",
                rec.id
            ));
            continue;
        }
        let id = NationId::new(nations.len() as u32);
        index.insert(rec.id.clone(), id);
        nations.push(ResolvedNation {
            id,
            external_id: rec.id,
            name: rec.name,
        });
    }
    (nations, index)
}

fn resolve_competitions(
    mut raw: Vec<RawCompetition>,
    nation_index: &BTreeMap<String, NationId>,
    issues: &mut Vec<String>,
) -> (Vec<ResolvedCompetition>, BTreeMap<String, CompetitionId>) {
    raw.sort_by(|a, b| a.id.cmp(&b.id));

    // Primeiro passo: valida e monta o índice id-externo -> id-denso, sem
    // ainda resolver promoção/rebaixamento (que pode apontar para uma
    // competição que só será indexada mais adiante nesta mesma lista).
    let mut seen = std::collections::BTreeSet::new();
    let mut accepted: Vec<RawCompetition> = Vec::with_capacity(raw.len());
    let mut index = BTreeMap::new();
    for rec in raw.drain(..) {
        if seen.contains(&rec.id) {
            issues.push(format!(
                "competição duplicada: id '{}' aparece em mais de um arquivo",
                rec.id
            ));
            continue;
        }
        if !nation_index.contains_key(&rec.nation) {
            issues.push(format!(
                "competição '{}' referencia país inexistente '{}'",
                rec.id, rec.nation
            ));
            continue;
        }
        seen.insert(rec.id.clone());
        let id = CompetitionId::new(index.len() as u32);
        index.insert(rec.id.clone(), id);
        accepted.push(rec);
    }

    // Segundo passo: agora que `index` tem todas as competições aceitas,
    // promoção/rebaixamento podem ser resolvidos em qualquer ordem de
    // declaração (ex.: a 1ª divisão pode citar a 2ª antes de o arquivo da
    // 2ª existir na ordem alfabética).
    let mut competitions = Vec::with_capacity(accepted.len());
    for rec in accepted {
        let id = index[&rec.id];
        let nation = nation_index[&rec.nation];
        let format = resolve_format(&rec.id, rec.format, issues);
        let tiebreakers = resolve_tiebreakers(&rec.id, &rec.tiebreakers, issues);
        let promotion = resolve_movement(&rec.id, "promoção", &rec.promotion, &index, issues);
        let relegation = resolve_movement(&rec.id, "rebaixamento", &rec.relegation, &index, issues);

        competitions.push(ResolvedCompetition {
            id,
            external_id: rec.id,
            name: rec.name,
            nation,
            tier: rec.tier,
            format,
            tiebreakers,
            promotion,
            relegation,
        });
    }

    (competitions, index)
}

fn resolve_format(competition_id: &str, raw: RawFormat, issues: &mut Vec<String>) -> Format {
    let RawFormat::RoundRobin { legs, teams } = raw;
    if !(1..=2).contains(&legs) {
        issues.push(format!(
            "competição '{competition_id}': 'legs' deve ser 1 ou 2 (veio {legs})"
        ));
    }
    if teams == 0 {
        issues.push(format!(
            "competição '{competition_id}': 'teams' deve ser maior que zero"
        ));
    }
    Format::RoundRobin { legs, teams }
}

fn resolve_tiebreakers(
    competition_id: &str,
    raw: &[String],
    issues: &mut Vec<String>,
) -> Vec<Tiebreaker> {
    raw.iter()
        .filter_map(|s| match Tiebreaker::parse(s) {
            Some(t) => Some(t),
            None => {
                issues.push(format!(
                    "competição '{competition_id}': critério de desempate desconhecido '{s}'"
                ));
                None
            }
        })
        .collect()
}

fn resolve_movement(
    competition_id: &str,
    kind: &str,
    raw: &RawMovement,
    competition_index: &BTreeMap<String, CompetitionId>,
    issues: &mut Vec<String>,
) -> Movement {
    let to = raw.to.as_ref().and_then(|target| {
        match competition_index.get(target) {
            Some(&id) => Some(id),
            None => {
                issues.push(format!(
                    "competição '{competition_id}': {kind} aponta para competição inexistente '{target}'"
                ));
                None
            }
        }
    });
    Movement {
        to,
        slots: raw.slots,
    }
}

fn resolve_clubs(
    mut raw: Vec<RawClub>,
    nation_index: &BTreeMap<String, NationId>,
    competition_index: &BTreeMap<String, CompetitionId>,
    issues: &mut Vec<String>,
) -> Vec<ResolvedClub> {
    raw.sort_by(|a, b| a.id.cmp(&b.id));
    let mut seen = std::collections::BTreeSet::new();
    let mut clubs = Vec::with_capacity(raw.len());

    for rec in raw {
        if seen.contains(&rec.id) {
            issues.push(format!(
                "clube duplicado: id '{}' aparece em mais de um arquivo",
                rec.id
            ));
            continue;
        }
        let Some(&nation) = nation_index.get(&rec.nation) else {
            issues.push(format!(
                "clube '{}' referencia país inexistente '{}'",
                rec.id, rec.nation
            ));
            continue;
        };
        let Some(&competition) = competition_index.get(&rec.competition) else {
            issues.push(format!(
                "clube '{}' referencia competição inexistente '{}'",
                rec.id, rec.competition
            ));
            continue;
        };
        seen.insert(rec.id.clone());
        let id = ClubId::new(clubs.len() as u32);
        clubs.push(ResolvedClub {
            id,
            external_id: rec.id,
            name: rec.name,
            nation,
            competition,
            founded: rec.founded,
            stadium: rec.stadium,
        });
    }
    clubs
}

/// Verificação final: o número de clubes de fato inscritos numa competição
/// precisa bater com `format.teams` declarado — é o tipo de inconsistência
/// de digitação que só aparece jogando, se ninguém checar antes.
fn check_competition_club_counts(
    competitions: &[ResolvedCompetition],
    clubs: &[ResolvedClub],
    issues: &mut Vec<String>,
) {
    for comp in competitions {
        let Format::RoundRobin { teams, .. } = comp.format;
        let actual = clubs.iter().filter(|c| c.competition == comp.id).count();
        if actual as u32 != teams {
            issues.push(format!(
                "competição '{}' declara {teams} times em 'format.teams', mas tem {actual} clube(s) inscrito(s)",
                comp.external_id
            ));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::raw::{RawClub, RawCompetition, RawFormat, RawMovement, RawNation};

    fn nation(id: &str) -> RawNation {
        RawNation {
            id: id.to_string(),
            name: format!("Nação {id}"),
        }
    }

    fn competition(id: &str, nation: &str, teams: u32) -> RawCompetition {
        RawCompetition {
            id: id.to_string(),
            name: format!("Competição {id}"),
            nation: nation.to_string(),
            tier: 1,
            format: RawFormat::RoundRobin { legs: 2, teams },
            tiebreakers: vec!["points".to_string()],
            promotion: RawMovement::default(),
            relegation: RawMovement::default(),
        }
    }

    fn club(id: &str, nation: &str, competition: &str) -> RawClub {
        RawClub {
            id: id.to_string(),
            name: format!("Clube {id}"),
            nation: nation.to_string(),
            competition: competition.to_string(),
            founded: None,
            stadium: None,
        }
    }

    #[test]
    fn pack_coerente_nao_gera_nenhum_problema() {
        let raw = RawPack {
            nations: vec![nation("ex")],
            competitions: vec![competition("ex.t1", "ex", 2)],
            clubs: vec![club("ex.a", "ex", "ex.t1"), club("ex.b", "ex", "ex.t1")],
        };
        let (pack, issues) = resolve(raw);
        assert!(
            issues.is_empty(),
            "esperava zero problemas, veio: {issues:?}"
        );
        assert_eq!(pack.nations.len(), 1);
        assert_eq!(pack.competitions.len(), 1);
        assert_eq!(pack.clubs.len(), 2);
    }

    #[test]
    fn ids_densos_sao_atribuidos_em_ordem_alfabetica_do_id_externo() {
        let raw = RawPack {
            nations: vec![nation("zz"), nation("aa")],
            ..RawPack::default()
        };
        let (pack, issues) = resolve(raw);
        assert!(issues.is_empty());
        assert_eq!(pack.nations[0].external_id, "aa");
        assert_eq!(pack.nations[0].id, domain::NationId::new(0));
        assert_eq!(pack.nations[1].external_id, "zz");
        assert_eq!(pack.nations[1].id, domain::NationId::new(1));
    }

    #[test]
    fn clube_com_pais_inexistente_vira_problema_e_e_descartado() {
        let raw = RawPack {
            nations: vec![nation("ex")],
            competitions: vec![competition("ex.t1", "ex", 1)],
            clubs: vec![club("ex.a", "nao_existe", "ex.t1")],
        };
        let (pack, issues) = resolve(raw);
        assert_eq!(pack.clubs.len(), 0);
        assert!(issues.iter().any(|i| i.contains("país inexistente")));
    }

    #[test]
    fn competicao_com_pais_inexistente_vira_problema_e_e_descartada() {
        let raw = RawPack {
            nations: vec![],
            competitions: vec![competition("ex.t1", "fantasma", 1)],
            clubs: vec![],
        };
        let (pack, issues) = resolve(raw);
        assert_eq!(pack.competitions.len(), 0);
        assert!(issues.iter().any(|i| i.contains("país inexistente")));
    }

    #[test]
    fn rebaixamento_pode_referenciar_competicao_declarada_depois_no_alfabeto() {
        // "a.tier1" vem antes de "b.tier2" em ordem alfabética, mas
        // rebaixamento de tier1 aponta para tier2 — o segundo passo de
        // resolve_competitions precisa lidar com isso.
        let mut tier1 = competition("a.tier1", "ex", 1);
        tier1.relegation = RawMovement {
            to: Some("b.tier2".to_string()),
            slots: 1,
        };
        let raw = RawPack {
            nations: vec![nation("ex")],
            competitions: vec![tier1, competition("b.tier2", "ex", 1)],
            clubs: vec![club("ex.a", "ex", "a.tier1"), club("ex.b", "ex", "b.tier2")],
        };
        let (pack, issues) = resolve(raw);
        assert!(issues.is_empty(), "issues inesperadas: {issues:?}");
        let tier1_resolved = &pack.competitions[0];
        assert_eq!(tier1_resolved.external_id, "a.tier1");
        assert!(tier1_resolved.relegation.to.is_some());
    }

    #[test]
    fn contagem_de_clubes_divergente_do_declarado_vira_problema() {
        let raw = RawPack {
            nations: vec![nation("ex")],
            competitions: vec![competition("ex.t1", "ex", 4)], // declara 4 times
            clubs: vec![club("ex.a", "ex", "ex.t1")],          // só 1 inscrito
        };
        let (_pack, issues) = resolve(raw);
        assert!(issues.iter().any(|i| i.contains("declara 4 times")));
    }

    #[test]
    fn tiebreaker_desconhecido_vira_problema_mas_nao_descarta_a_competicao() {
        let mut comp = competition("ex.t1", "ex", 0);
        comp.tiebreakers = vec!["points".to_string(), "sorteio_no_bar".to_string()];
        let raw = RawPack {
            nations: vec![nation("ex")],
            competitions: vec![comp],
            clubs: vec![],
        };
        let (pack, issues) = resolve(raw);
        assert_eq!(pack.competitions.len(), 1);
        assert_eq!(pack.competitions[0].tiebreakers.len(), 1); // só "points" sobrevive
        assert!(
            issues
                .iter()
                .any(|i| i.contains("critério de desempate desconhecido"))
        );
    }

    #[test]
    fn ids_duplicados_geram_problema_e_mantem_so_a_primeira_ocorrencia() {
        let raw = RawPack {
            nations: vec![nation("ex"), nation("ex")],
            ..RawPack::default()
        };
        let (pack, issues) = resolve(raw);
        assert_eq!(pack.nations.len(), 1);
        assert!(issues.iter().any(|i| i.contains("duplicada")));
    }
}
