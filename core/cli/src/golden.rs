//! Golden masters (`docs/08-qualidade-e-testes.md §3`) — "resultado
//! congelado de simulações de referência", o mecanismo central de proteção
//! contra regressão de simulação silenciosa (desbalanceamento sem crash).
//!
//! Escopo real desta primeira versão, mais estreito que o documento
//! completo: grava e verifica a tabela final de cada competição (via
//! `app::GameSession`, as mesmas `dispatch`/`query` que `play` usa — pega
//! regressão em qualquer parte do pipeline: motor, progressão, mercado,
//! lesões) mais os totais agregados de partidas/movimentações/
//! transferências/lesões, resumidos num único hash determinístico. **Não**
//! cobre "artilheiros" nem "distribuição de CA" (`docs/08 §3` completo):
//! nenhum dos dois é consultável hoje — `MatchEvent::Goal` não tem autor
//! (precisa de jogadores em campo de verdade, M2/M3) e `app::Query` não
//! expõe CA de elenco (nenhuma tela de elenco existe ainda pra precisar
//! disso). Também não é "hash de estado por dia": não há calendário diário
//! ligado a `world` ainda (`docs/02 §5`) — aqui é por temporada.

use std::fmt::Write as _;
use std::path::Path;
use std::process::ExitCode;

use serde::{Deserialize, Serialize};

/// Versão do formato do arquivo golden — bump exige nova gravação de todos
/// os goldens existentes (mesma disciplina de `persist::SCHEMA_VERSION`).
const SCHEMA_VERSION: u16 = 1;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct TableRowSnapshot {
    position: u32,
    club_name: String,
    played: u32,
    wins: u32,
    draws: u32,
    losses: u32,
    goals_for: u32,
    goals_against: u32,
    points: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct CompetitionSnapshot {
    name: String,
    nation_name: String,
    /// Vazio se a competição não rodou nenhuma partida (não deveria
    /// acontecer com `seasons >= 1`, mas não é um `panic` se acontecer).
    table: Vec<TableRowSnapshot>,
}

/// O conteúdo de um golden master — tudo exceto `state_hash` é dado
/// "de leitura humana" (`docs/08 §3`: o arquivo guarda isso pra ajudar a
/// entender *o que* mudou, não só *que* algo mudou); `state_hash` é o que
/// `verify` de fato compara campo a campo seria frágil demais (qualquer
/// reordenação inofensiva quebraria a comparação) — o hash resume tudo
/// numa comparação só, e o conteúdo legível fica para quando ele falhar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct GoldenMaster {
    schema_version: u16,
    pack_id: String,
    pack_version: String,
    world_seed: u64,
    seasons: u32,
    total_matches_played: u32,
    total_movements: usize,
    total_transfers: usize,
    total_injuries: usize,
    /// Tabela final (depois da última temporada) de cada competição do
    /// pack, na mesma ordem que `app::Query::Competitions` devolve —
    /// determinística (ordem de id denso, `docs/02 §6.1`).
    competitions: Vec<CompetitionSnapshot>,
    /// FNV-1a (mesmo algoritmo de `persist::save`, mesma justificativa: um
    /// checksum pequeno e determinístico não precisa de uma dependência
    /// nova) sobre a serialização JSON de todo o resto deste struct.
    state_hash: String,
}

fn fnv1a64(bytes: &[u8]) -> u64 {
    const OFFSET_BASIS: u64 = 0xcbf2_9ce4_8422_2325;
    const PRIME: u64 = 0x0000_0100_0000_01b3;
    let mut hash = OFFSET_BASIS;
    for &byte in bytes {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(PRIME);
    }
    hash
}

fn to_hex(hash: u64) -> String {
    let mut s = String::with_capacity(16);
    let _ = write!(s, "{hash:016x}");
    s
}

/// Roda `seasons` temporadas via `app::GameSession` (seed/pack dados) e
/// monta o `GoldenMaster` correspondente — o núcleo comum entre `record` e
/// `verify`: os dois rodam exatamente a mesma simulação, só o que fazem
/// com o resultado depois muda.
fn simulate(pack_path: &Path, world_seed: u64, seasons: u32) -> Result<GoldenMaster, String> {
    let mut session = app::GameSession::new(pack_path, world_seed)
        .map_err(|err| format!("falha ao iniciar sessão: {err}"))?;

    let mut total_matches_played = 0u32;
    let mut total_movements = 0usize;
    let mut total_transfers = 0usize;
    let mut total_injuries = 0usize;
    for _ in 0..seasons {
        let receipt = session
            .dispatch(app::Command::AdvanceSeason)
            .map_err(|err| format!("falha ao avançar temporada: {err}"))?;
        total_matches_played += receipt.matches_played;
        total_movements += receipt.movements;
        total_transfers += receipt.transfers;
        total_injuries += receipt.injuries;
    }

    let app::QueryResult::Competitions(competitions_list) = session.query(app::Query::Competitions)
    else {
        unreachable!("Query::Competitions sempre devolve QueryResult::Competitions")
    };

    let mut competitions = Vec::with_capacity(competitions_list.len());
    for competition in competitions_list {
        let table = match session.query(app::Query::Standings {
            competition: competition.id,
        }) {
            app::QueryResult::Standings(Some(rows)) => rows
                .into_iter()
                .map(|row| TableRowSnapshot {
                    position: row.position,
                    club_name: row.club_name,
                    played: row.played,
                    wins: row.wins,
                    draws: row.draws,
                    losses: row.losses,
                    goals_for: row.goals_for,
                    goals_against: row.goals_against,
                    points: row.points,
                })
                .collect(),
            _ => Vec::new(),
        };
        competitions.push(CompetitionSnapshot {
            name: competition.name,
            nation_name: competition.nation_name,
            table,
        });
    }

    let save = session.to_save();
    let mut master = GoldenMaster {
        schema_version: SCHEMA_VERSION,
        pack_id: save.pack_id,
        pack_version: save.pack_version,
        world_seed,
        seasons,
        total_matches_played,
        total_movements,
        total_transfers,
        total_injuries,
        competitions,
        state_hash: String::new(),
    };
    let payload = serde_json::to_vec(&master)
        .map_err(|err| format!("falha ao serializar golden master: {err}"))?;
    master.state_hash = to_hex(fnv1a64(&payload));
    Ok(master)
}

/// Implementação real de `golden record` (`docs/08 §3`).
pub fn record(
    seed: u64,
    seasons: u32,
    pack_path: Option<std::path::PathBuf>,
    out_path: &Path,
) -> ExitCode {
    let pack_path = pack_path.unwrap_or_else(crate::default_example_pack_path);
    let master = match simulate(&pack_path, seed, seasons) {
        Ok(master) => master,
        Err(err) => {
            eprintln!("managerfc-cli: {err}");
            return ExitCode::from(2);
        }
    };

    let json = match serde_json::to_string_pretty(&master) {
        Ok(json) => json,
        Err(err) => {
            eprintln!("managerfc-cli: falha ao serializar golden master: {err}");
            return ExitCode::from(2);
        }
    };
    if let Err(err) = std::fs::write(out_path, json) {
        eprintln!(
            "managerfc-cli: falha ao gravar golden master em {}: {err}",
            out_path.display()
        );
        return ExitCode::from(2);
    }

    println!(
        "golden master gravado em {} — seed={seed}, {seasons} temporada(s), hash={}",
        out_path.display(),
        master.state_hash
    );
    ExitCode::SUCCESS
}

/// Implementação real de `golden verify` (`docs/08 §3`). Lê `seasons` e
/// `pack_id`/`pack_version` do próprio arquivo gravado — `verify` reproduz
/// exatamente o que `record` rodou, não aceita um `--seasons` divergente
/// por engano.
pub fn verify(seed: u64, pack_path: Option<std::path::PathBuf>, expect_path: &Path) -> ExitCode {
    let pack_path = pack_path.unwrap_or_else(crate::default_example_pack_path);
    let contents = match std::fs::read_to_string(expect_path) {
        Ok(contents) => contents,
        Err(err) => {
            eprintln!(
                "managerfc-cli: falha ao ler golden master em {}: {err}",
                expect_path.display()
            );
            return ExitCode::from(2);
        }
    };
    let expected: GoldenMaster = match serde_json::from_str(&contents) {
        Ok(expected) => expected,
        Err(err) => {
            eprintln!(
                "managerfc-cli: golden master em {} não é um JSON válido: {err}",
                expect_path.display()
            );
            return ExitCode::from(2);
        }
    };
    if expected.schema_version != SCHEMA_VERSION {
        eprintln!(
            "managerfc-cli: golden master em {} é do schema v{}, este binário entende v{SCHEMA_VERSION}",
            expect_path.display(),
            expected.schema_version
        );
        return ExitCode::from(2);
    }
    if expected.world_seed != seed {
        eprintln!(
            "managerfc-cli: --seed {seed} não bate com a seed gravada no golden master ({}) — \
             golden verify reproduz exatamente o que foi gravado, não uma seed diferente",
            expected.world_seed
        );
        return ExitCode::from(2);
    }

    let actual = match simulate(&pack_path, expected.world_seed, expected.seasons) {
        Ok(actual) => actual,
        Err(err) => {
            eprintln!("managerfc-cli: {err}");
            return ExitCode::from(2);
        }
    };

    if actual.state_hash == expected.state_hash {
        println!(
            "golden master OK — seed={seed}, {} temporada(s), hash={}",
            expected.seasons, actual.state_hash
        );
        return ExitCode::SUCCESS;
    }

    eprintln!(
        "managerfc-cli: golden master DIVERGENTE — hash esperado {}, obtido {}",
        expected.state_hash, actual.state_hash
    );
    eprintln!(
        "  partidas: esperado {}, obtido {}",
        expected.total_matches_played, actual.total_matches_played
    );
    eprintln!(
        "  movimentações: esperado {}, obtido {}",
        expected.total_movements, actual.total_movements
    );
    eprintln!(
        "  transferências: esperado {}, obtido {}",
        expected.total_transfers, actual.total_transfers
    );
    eprintln!(
        "  lesões: esperado {}, obtido {}",
        expected.total_injuries, actual.total_injuries
    );
    report_competition_diff(&expected.competitions, &actual.competitions);
    eprintln!(
        "managerfc-cli: regressão de simulação, ou mudança intencional de balanceamento sem \
         regravar o golden (docs/08 §3) — nunca regrave um golden só pra fazer isto passar."
    );
    ExitCode::from(1)
}

/// Aponta a primeira competição/linha que diverge — não um diff completo,
/// só o suficiente pra quem está depurando saber onde olhar primeiro.
fn report_competition_diff(expected: &[CompetitionSnapshot], actual: &[CompetitionSnapshot]) {
    if expected.len() != actual.len() {
        eprintln!(
            "  competições: esperado {} competições, obtido {}",
            expected.len(),
            actual.len()
        );
        return;
    }
    for (exp_comp, act_comp) in expected.iter().zip(actual) {
        if exp_comp == act_comp {
            continue;
        }
        eprintln!("  competição '{}' diverge:", exp_comp.name);
        for (exp_row, act_row) in exp_comp.table.iter().zip(&act_comp.table) {
            if exp_row != act_row {
                eprintln!(
                    "    posição {}: esperado {} ({} pts), obtido {} ({} pts)",
                    exp_row.position,
                    exp_row.club_name,
                    exp_row.points,
                    act_row.club_name,
                    act_row.points
                );
                break;
            }
        }
        break;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn example_pack_path() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/example-two-tier")
    }

    #[test]
    fn simulate_e_deterministico_para_a_mesma_entrada() {
        let a = simulate(&example_pack_path(), 42, 2).unwrap();
        let b = simulate(&example_pack_path(), 42, 2).unwrap();
        assert_eq!(a, b);
        assert_eq!(a.state_hash, b.state_hash);
    }

    #[test]
    fn seeds_diferentes_produzem_hashes_diferentes() {
        let a = simulate(&example_pack_path(), 1, 2).unwrap();
        let b = simulate(&example_pack_path(), 2, 2).unwrap();
        assert_ne!(a.state_hash, b.state_hash);
    }

    #[test]
    fn record_e_verify_fazem_round_trip_em_disco() {
        let dir =
            std::env::temp_dir().join(format!("managerfc-golden-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("golden.json");

        let exit = record(7, 2, None, &out);
        assert!(matches!(exit, ExitCode::SUCCESS));

        let exit = verify(7, None, &out);
        assert!(matches!(exit, ExitCode::SUCCESS));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn verify_detecta_hash_divergente() {
        // Simula o que uma regressão de verdade produz: a simulação muda
        // (aqui, simulado direto no hash gravado, já que mudar o código de
        // produção não é o que este teste quer exercitar) e a nova
        // simulação, recalculada do zero por `verify`, não bate mais com o
        // que foi gravado. Mutar só um campo descritivo (ex.:
        // `total_matches_played`) sem tocar `state_hash` não serve de
        // regressão aqui: `verify` nunca reconfere a auto-consistência do
        // arquivo, só compara `state_hash` contra uma simulação nova — é
        // esse hash que precisa divergir.
        let dir = std::env::temp_dir().join(format!(
            "managerfc-golden-corrupt-test-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("golden.json");
        record(7, 2, None, &out);

        let mut master: GoldenMaster =
            serde_json::from_str(&std::fs::read_to_string(&out).unwrap()).unwrap();
        master.state_hash = "0000000000000000".to_string();
        std::fs::write(&out, serde_json::to_string_pretty(&master).unwrap()).unwrap();

        let exit = verify(7, None, &out);
        assert!(!matches!(exit, ExitCode::SUCCESS));

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn verify_recusa_seed_diferente_da_gravada() {
        let dir =
            std::env::temp_dir().join(format!("managerfc-golden-seed-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let out = dir.join("golden.json");
        record(7, 2, None, &out);

        let exit = verify(8, None, &out);
        assert!(!matches!(exit, ExitCode::SUCCESS));

        std::fs::remove_dir_all(&dir).ok();
    }
}
