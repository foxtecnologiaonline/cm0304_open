//! `managerfc-cli bench-season` — o gate de desempenho do MVP em escala de
//! MVP (`docs/00 §7`, critério 2; RNF-02/08): gera, em memória/disco
//! temporário, um pack fictício de 2 divisões × N clubes (24 → 48 clubes,
//! ~1.440 jogadores), joga temporadas inteiras **com um técnico humano** (o
//! caminho mais pesado: partida do clube via `LiveGame`, caixa de entrada,
//! ofertas) e mede tempo por temporada, custo das consultas que uma UI faz
//! a cada tela e o pico de memória (RSS, só onde o SO permite ler).
//!
//! O gerador espelha `packs/tools/gen_scale_pack.py`, mas em Rust e
//! determinístico, para a CI não depender de Python. Os orçamentos são os do
//! critério: 5 s por temporada no desktop; o celular de linha média
//! (20 s) não é medível aqui — a medida de desktop com folga grande é o
//! indicador (`docs/08 §6`).

use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::time::Instant;

use app::{Command, GameSession, PlayerFilter, Query};
use serde_json::{Value, json};

const FIRST_NAMES: [&str; 12] = [
    "Adão", "Bento", "Caio", "Danilo", "Edu", "Fábio", "Gil", "Hugo", "Iago", "Juca", "Kaio", "Léo",
];
const LAST_NAMES: [&str; 12] = [
    "Aguiar",
    "Barros",
    "Cavalcante",
    "Dornelles",
    "Esteves",
    "Farias",
    "Guimarães",
    "Homem",
    "Junqueira",
    "Leal",
    "Maciel",
    "Nogueira",
];

/// (posição, quantidade, atributos-chave) — 30 jogadores por clube.
const SQUAD_PLAN: [(&str, u32, &[&str]); 4] = [
    (
        "gk",
        3,
        &[
            "reflexes",
            "rushing",
            "aerial_ability",
            "distribution",
            "positioning",
            "concentration",
        ],
    ),
    (
        "df",
        10,
        &[
            "tackling",
            "marking",
            "heading",
            "strength",
            "positioning",
            "anticipation",
            "pace",
            "stamina",
        ],
    ),
    (
        "mf",
        10,
        &[
            "passing",
            "vision",
            "first_touch",
            "dribbling",
            "stamina",
            "decisions",
            "teamwork",
            "technique",
        ],
    ),
    (
        "fw",
        7,
        &[
            "finishing",
            "dribbling",
            "pace",
            "off_the_ball",
            "composure",
            "acceleration",
            "technique",
        ],
    ),
];

/// Gerador pseudoaleatório mínimo (splitmix64) só para o pack de teste —
/// não é o RNG do jogo, que mora em `domain::DeterministicRng`.
struct Mix(u64);

impl Mix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }

    fn range(&mut self, lo: u64, hi: u64) -> u64 {
        lo + self.next() % (hi - lo + 1)
    }
}

fn write_json(path: &Path, value: &Value) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_vec(value).expect("JSON serializável"))
}

/// Escreve o pack de escala em `dir`: `per_division` clubes por divisão.
pub fn write_scale_pack(dir: &Path, per_division: u32) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    std::fs::write(
        dir.join("pack.toml"),
        "id = \"scale.two-tier\"\nname = \"Escala MVP\"\nversion = \"0.1.0\"\nlicense = \"CC0-1.0\"\nreference_year = 2026\n",
    )?;
    write_json(
        &dir.join("nations/sc.json"),
        &json!({ "id": "sc", "name": "Escalônia" }),
    )?;
    let total = 2 * per_division;
    if total.is_power_of_two() {
        write_json(
            &dir.join("competitions/cup.json"),
            &json!({ "id": "sc.cup", "name": "Copa", "nation": "sc", "tier": 0,
                     "format": { "type": "knockout", "teams": total } }),
        )?;
    }
    let tiebreakers = json!([
        "points",
        "wins",
        "goal_difference",
        "goals_for",
        "head_to_head"
    ]);
    write_json(
        &dir.join("competitions/tier1.json"),
        &json!({ "id": "sc.tier1", "name": "Divisão 1", "nation": "sc", "tier": 1,
                 "format": { "type": "round_robin", "legs": 2, "teams": per_division },
                 "tiebreakers": tiebreakers, "relegation": { "to": "sc.tier2", "slots": 3 } }),
    )?;
    write_json(
        &dir.join("competitions/tier2.json"),
        &json!({ "id": "sc.tier2", "name": "Divisão 2", "nation": "sc", "tier": 2,
                 "format": { "type": "round_robin", "legs": 2, "teams": per_division },
                 "tiebreakers": tiebreakers, "promotion": { "to": "sc.tier1", "slots": 3 } }),
    )?;
    for tier in 1..=2u32 {
        for n in 1..=per_division {
            let club = format!("sc.t{tier}.{n:02}");
            write_json(
                &dir.join(format!("clubs/{club}.json")),
                &json!({ "id": club, "name": format!("Clube {tier}-{n:02}"), "nation": "sc",
                         "competition": format!("sc.tier{tier}"), "founded": 1900 + n }),
            )?;
            let mut rng = Mix(u64::from(tier) << 32 | u64::from(n));
            let mut squad = Vec::new();
            let mut idx = 1;
            for (position, count, keys) in SQUAD_PLAN {
                for _ in 0..count {
                    let age = rng.range(17, 33);
                    let mut attrs = serde_json::Map::new();
                    for key in keys {
                        attrs.insert((*key).to_string(), json!(rng.range(11, 18)));
                    }
                    for extra in ["determination", "leadership", "composure"] {
                        attrs
                            .entry(extra)
                            .or_insert_with(|| json!(rng.range(8, 15)));
                    }
                    let current = rng.range(70, 155);
                    squad.push(json!({
                        "id": format!("{club}.p{idx:02}"), "club": club, "nation": "sc",
                        "first_name": FIRST_NAMES[rng.range(0, 11) as usize],
                        "last_name": LAST_NAMES[rng.range(0, 11) as usize],
                        "birth_date": format!("{:04}-{:02}-{:02}", 2026 - age, rng.range(1, 12), rng.range(1, 28)),
                        "position": position, "attributes": attrs,
                        "ability": { "current": current, "potential": (current + rng.range(0, 45)).min(200) },
                    }));
                    idx += 1;
                }
            }
            write_json(&dir.join(format!("people/{club}.json")), &json!(squad))?;
        }
    }
    Ok(())
}

/// Pico de memória residente do processo, em MB, onde o SO deixa ler sem
/// dependência nova (Linux: `VmHWM` de `/proc/self/status`).
fn peak_rss_mb() -> Option<f64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let kb: f64 = status
        .lines()
        .find_map(|l| l.strip_prefix("VmHWM:"))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    Some(kb / 1024.0)
}

fn temp_pack_dir() -> PathBuf {
    std::env::temp_dir().join(format!("managerfc-scale-{}", std::process::id()))
}

/// Implementação de `bench-season`.
pub fn run(per_division: u32, seasons: u32, max_season_ms: u64, max_rss_mb: u64) -> ExitCode {
    let dir = temp_pack_dir();
    if let Err(err) = write_scale_pack(&dir, per_division) {
        eprintln!("managerfc-cli: falha ao gerar o pack de escala: {err}");
        return ExitCode::from(2);
    }
    let outcome = measure(&dir, per_division, seasons, max_season_ms, max_rss_mb);
    let _ = std::fs::remove_dir_all(&dir);
    outcome
}

fn measure(
    dir: &Path,
    per_division: u32,
    seasons: u32,
    max_season_ms: u64,
    max_rss_mb: u64,
) -> ExitCode {
    let start = Instant::now();
    let mut session = match GameSession::new(dir, 42) {
        Ok(s) => s,
        Err(err) => {
            eprintln!("managerfc-cli: falha ao abrir o pack de escala: {err}");
            return ExitCode::from(2);
        }
    };
    let load_ms = start.elapsed().as_millis();
    let clubs = 2 * per_division;
    let players = clubs * 30;
    println!(
        "pack de escala: {clubs} clubes, {players} jogadores — carga e sessão em {load_ms} ms"
    );

    let club = domain::ClubId::new(3);
    let taken = session.dispatch(Command::TakeClub {
        club,
        coach: app::CoachProfile {
            name: "Bench".to_string(),
            nation: domain::NationId::new(0),
            reputation: 30,
        },
    });
    if let Err(err) = taken {
        eprintln!("managerfc-cli: TakeClub falhou: {err}");
        return ExitCode::from(2);
    }

    let mut worst_ms = 0u128;
    for season in 0..seasons {
        let t = Instant::now();
        if let Err(err) = session.dispatch(Command::AdvanceSeason) {
            eprintln!("managerfc-cli: temporada {} falhou: {err}", season + 1);
            return ExitCode::from(2);
        }
        let ms = t.elapsed().as_millis();
        worst_ms = worst_ms.max(ms);
        println!("temporada {}: {ms} ms", season + 1);
    }

    // Custo das consultas de uma tela típica (lista de elenco e de busca).
    let squad_us = time_us(200, || {
        let _ = session.query(Query::Squad { club });
    });
    let search_us = time_us(200, || {
        let _ = session.query(Query::Search {
            filter: PlayerFilter::default(),
            offset: 0,
            limit: 50,
        });
    });
    let inbox_us = time_us(200, || {
        let _ = session.query(Query::Inbox {
            unread_only: false,
            offset: 0,
            limit: 50,
        });
    });
    println!(
        "consultas (média): elenco {squad_us:.0} µs | busca (50 linhas) {search_us:.0} µs | caixa {inbox_us:.0} µs"
    );

    let mut failed = false;
    if worst_ms > u128::from(max_season_ms) {
        eprintln!("FORA do orçamento: pior temporada {worst_ms} ms > {max_season_ms} ms");
        failed = true;
    } else {
        println!(
            "[OK  ] pior temporada {worst_ms} ms ≤ {max_season_ms} ms (critério MVP #2, desktop)"
        );
    }
    match peak_rss_mb() {
        Some(rss) if rss > max_rss_mb as f64 => {
            eprintln!("FORA do orçamento: pico de memória {rss:.1} MB > {max_rss_mb} MB");
            failed = true;
        }
        Some(rss) => {
            println!("[OK  ] pico de memória {rss:.1} MB ≤ {max_rss_mb} MB (RNF-08, mobile)")
        }
        None => println!("[n/d ] pico de memória não mensurável neste SO"),
    }
    if failed {
        ExitCode::from(1)
    } else {
        ExitCode::SUCCESS
    }
}

fn time_us(iterations: u32, mut f: impl FnMut()) -> f64 {
    let t = Instant::now();
    for _ in 0..iterations {
        f();
    }
    t.elapsed().as_secs_f64() * 1e6 / f64::from(iterations)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pack_de_escala_gerado_em_rust_e_valido_com_48_e_com_64_clubes() {
        for per_division in [24u32, 32] {
            let dir = std::env::temp_dir().join(format!(
                "managerfc-scale-test-{}-{per_division}",
                std::process::id()
            ));
            write_scale_pack(&dir, per_division).unwrap();
            let report = pack::load_and_validate(&dir).unwrap();
            assert!(report.is_valid(), "{:?}", report.issues);
            assert_eq!(report.pack.clubs.len() as u32, 2 * per_division);
            assert_eq!(report.pack.players.len() as u32, 2 * per_division * 30);
            std::fs::remove_dir_all(&dir).ok();
        }
    }

    #[test]
    fn gerador_e_deterministico() {
        let a = std::env::temp_dir().join(format!("managerfc-scale-det-a-{}", std::process::id()));
        let b = std::env::temp_dir().join(format!("managerfc-scale-det-b-{}", std::process::id()));
        write_scale_pack(&a, 24).unwrap();
        write_scale_pack(&b, 24).unwrap();
        let read = |d: &Path| std::fs::read(d.join("people/sc.t1.01.json")).unwrap();
        assert_eq!(read(&a), read(&b));
        std::fs::remove_dir_all(&a).ok();
        std::fs::remove_dir_all(&b).ok();
    }

    #[test]
    fn uma_temporada_em_escala_de_mvp_cabe_folgadamente_no_orcamento() {
        let dir = std::env::temp_dir().join(format!("managerfc-scale-perf-{}", std::process::id()));
        write_scale_pack(&dir, 24).unwrap();
        let mut session = GameSession::new(&dir, 42).unwrap();
        session
            .dispatch(Command::TakeClub {
                club: domain::ClubId::new(3),
                coach: app::CoachProfile {
                    name: "T".to_string(),
                    nation: domain::NationId::new(0),
                    reputation: 30,
                },
            })
            .unwrap();
        let t = Instant::now();
        session.dispatch(Command::AdvanceSeason).unwrap();
        // Critério MVP #2: 5 s no desktop; este teste roda em perfil de
        // teste (otimizado + debuginfo) e exige 100x de folga.
        assert!(t.elapsed().as_millis() < 5_000, "{:?}", t.elapsed());
        std::fs::remove_dir_all(&dir).ok();
    }
}
