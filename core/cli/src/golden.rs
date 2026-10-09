//! Golden masters (`docs/08-qualidade-e-testes.md §3`) — "resultado
//! congelado de simulações de referência", o mecanismo central de proteção
//! contra regressão de simulação silenciosa (desbalanceamento sem crash).
//!
//! Escopo real desta primeira versão, mais estreito que o documento
//! completo: grava e verifica a tabela final de cada competição de liga e
//! o campeão de cada copa (via `app::GameSession`, as mesmas
//! `dispatch`/`query` que `play` usa — pega regressão em qualquer parte do
//! pipeline: motor, progressão, mercado, lesões, mata-mata) mais os totais
//! agregados de partidas/movimentações/transferências/lesões, resumidos
//! num único hash determinístico. **Não** cobre "artilheiros" nem
//! "distribuição de CA" (`docs/08 §3` completo):
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
/// Bump 1→2: `CompetitionSnapshot` ganhou `cup_champion` (`pack::Format::Knockout`,
/// `docs/07-roadmap.md` M1 — copa). Bump 2→3: `total_tired_players`
/// (`world::apply_season_fatigue`, RF-JG-07 — condição física). Bump 3→4:
/// `total_suspensions` (`world::discipline`, RF-JG-09 — suspensão por
/// expulsão; liga agora simulada rodada a rodada no caminho de carreira,
/// ver `world::season`, então o hash de qualquer golden anterior já não
/// bateria mesmo sem esse campo novo). Bump 4→5: `total_payroll_paid_cents`
/// (`world::pay_salaries`, RF-CL-02 — folha salarial; orçamento final dos
/// clubes muda, então o hash de qualquer golden anterior também já não
/// bateria mesmo sem esse campo novo). Bump 5→6: `total_match_day_revenue_cents`
/// (`world::finance::match_day_revenue`, RF-CL-03 — bilheteria; orçamento
/// final dos clubes muda de novo, mesma justificativa do bump anterior).
const SCHEMA_VERSION: u16 = 6;

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
    /// Vazio se a competição não rodou nenhuma partida, ou se é uma copa
    /// (`pack::Format::Knockout` não tem tabela — ver `cup_champion`
    /// abaixo).
    table: Vec<TableRowSnapshot>,
    /// `Some` só para uma competição de copa que já rodou — o campeão e o
    /// total de partidas de mata-mata (`app::Query::CupChampion`,
    /// `world::cup`). `None` para liga, ou para copa que ainda não rodou.
    #[serde(default)]
    cup_champion: Option<CupChampionSnapshot>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
struct CupChampionSnapshot {
    champion_club_name: String,
    rounds_played: u32,
    matches_played: u32,
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
    /// Soma de `CommandReceipt::tired_players` de cada temporada (mesma
    /// semântica de `total_injuries`: soma de instantâneos por temporada,
    /// não uma contagem de eventos distintos — `world::apply_season_fatigue`).
    total_tired_players: usize,
    /// Soma de `CommandReceipt::suspensions` de cada temporada — ao
    /// contrário de `total_tired_players`, esta é mesmo uma contagem de
    /// eventos distintos (uma expulsão por rodada em que aconteceu,
    /// `world::discipline::roll_suspensions`), não um instantâneo.
    total_suspensions: usize,
    /// Soma de `CommandReceipt::payroll_paid` (em centavos) de cada
    /// temporada — `world::pay_salaries`, RF-CL-02. `i64`, não `Money`:
    /// o golden master é JSON de leitura humana, não precisa do tipo de
    /// domínio, só do número.
    total_payroll_paid_cents: i64,
    /// Soma de `CommandReceipt::match_day_revenue` (em centavos) de cada
    /// temporada — `world::finance::match_day_revenue`, RF-CL-03. Mesma
    /// justificativa de `i64` que `total_payroll_paid_cents`.
    total_match_day_revenue_cents: i64,
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
    let mut total_tired_players = 0usize;
    let mut total_suspensions = 0usize;
    let mut total_payroll_paid_cents = 0i64;
    let mut total_match_day_revenue_cents = 0i64;
    for _ in 0..seasons {
        let receipt = session
            .dispatch(app::Command::AdvanceSeason)
            .map_err(|err| format!("falha ao avançar temporada: {err}"))?;
        total_matches_played += receipt.matches_played;
        total_movements += receipt.movements;
        total_transfers += receipt.transfers;
        total_injuries += receipt.injuries;
        total_tired_players += receipt.tired_players;
        total_suspensions += receipt.suspensions;
        total_payroll_paid_cents += receipt.payroll_paid.cents();
        total_match_day_revenue_cents += receipt.match_day_revenue.cents();
    }

    let app::QueryResult::Competitions(competitions_list) = session.query(app::Query::Competitions)
    else {
        unreachable!("Query::Competitions sempre devolve QueryResult::Competitions")
    };

    let mut competitions = Vec::with_capacity(competitions_list.len());
    for competition in competitions_list {
        // `kind` já diz qual das duas consultas faz sentido — poupa a outra
        // (sempre devolveria `None`/vazio para o formato errado, ver
        // `app::query::CompetitionKind`).
        let (table, cup_champion) = match competition.kind {
            app::CompetitionKind::League => {
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
                (table, None)
            }
            app::CompetitionKind::Cup => {
                let cup_champion = match session.query(app::Query::CupChampion {
                    competition: competition.id,
                }) {
                    app::QueryResult::CupChampion(Some(champion)) => Some(CupChampionSnapshot {
                        champion_club_name: champion.champion_club_name,
                        rounds_played: champion.rounds_played,
                        matches_played: champion.matches_played,
                    }),
                    _ => None,
                };
                (Vec::new(), cup_champion)
            }
        };
        competitions.push(CompetitionSnapshot {
            name: competition.name,
            nation_name: competition.nation_name,
            table,
            cup_champion,
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
        total_tired_players,
        total_suspensions,
        total_payroll_paid_cents,
        total_match_day_revenue_cents,
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
    eprintln!(
        "  jogadores cansados: esperado {}, obtido {}",
        expected.total_tired_players, actual.total_tired_players
    );
    eprintln!(
        "  suspensões: esperado {}, obtido {}",
        expected.total_suspensions, actual.total_suspensions
    );
    eprintln!(
        "  folha salarial (centavos): esperado {}, obtido {}",
        expected.total_payroll_paid_cents, actual.total_payroll_paid_cents
    );
    eprintln!(
        "  bilheteria (centavos): esperado {}, obtido {}",
        expected.total_match_day_revenue_cents, actual.total_match_day_revenue_cents
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
        if exp_comp.cup_champion != act_comp.cup_champion {
            eprintln!(
                "    campeão de copa: esperado {:?}, obtido {:?}",
                exp_comp.cup_champion, act_comp.cup_champion
            );
        }
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
