//! `managerfc-cli` — binário headless do núcleo (`docs/02-arquitetura.md §10`).
//!
//! É o único jeito de rodar o núcleo sem UI: CI, balanceamento e modders
//! passam por aqui, nunca pelo Flutter (RNF-13). Neste estágio (M0) parte
//! dos subcomandos ainda é **placeholder documentado** — existem para que a
//! forma final da CLI já apareça em `--help` desde o primeiro commit, mesmo
//! antes de `world`/`rules` terem regra de negócio real para expor.
//! `pack validate` e `bench` já são reais: o carregador de data pack e o
//! motor v0 são entregáveis do próprio M0
//! (`docs/07-roadmap.md#m0--fundação-6-semanas`). `bench` hoje só mede o que
//! existe (o custo de uma partida isolada, RNF-01) — os orçamentos de dia e
//! de temporada (RNF-02/03) esperam o loop de calendário do `world`. Cada
//! placeholder restante diz explicitamente em que marco passa a funcionar,
//! para não ser confundido com um comando quebrado.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

#[derive(Parser)]
#[command(
    name = "managerfc-cli",
    version,
    about = "Núcleo headless do ManagerFC — simular, calibrar, validar, sem UI.",
    long_about = None
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// Mostra versão do binário e dos crates do núcleo — smoke test de que o
    /// workspace está corretamente ligado.
    Version,

    /// Mede o custo de simular uma partida contra o orçamento de RNF-01
    /// (`docs/01 §3.1`). Os orçamentos de dia/temporada (RNF-02/03) ainda
    /// não são medíveis — esperam o `world` (M1).
    Bench {
        /// Quantas partidas simular para tirar a média.
        #[arg(long, default_value_t = 10_000)]
        trials: u32,
    },

    /// Simula temporadas headless e emite métricas de calibração (`docs/08 §4`).
    /// Placeholder — chega com o motor v0/v1 (M1–M2).
    Calibrate {
        /// Número de temporadas a simular.
        #[arg(long, default_value_t = 20)]
        seasons: u32,
        /// Seed do mundo.
        #[arg(long, default_value_t = 42)]
        seed: u64,
    },

    /// Operações sobre data packs (`docs/03 §7`).
    Pack {
        #[command(subcommand)]
        action: PackAction,
    },

    /// Grava ou verifica golden masters (`docs/08 §3`).
    Golden {
        #[command(subcommand)]
        action: GoldenAction,
    },
}

#[derive(Subcommand)]
enum PackAction {
    /// Valida um data pack contra o schema e as regras de consistência
    /// (`docs/03 §7`). Ainda só lê diretórios — `.fmpack` zipado é trabalho
    /// futuro (M4, `docs/07-roadmap.md`).
    Validate {
        /// Caminho do diretório do pack (contendo `pack.toml`).
        path: PathBuf,
    },
}

#[derive(Subcommand)]
enum GoldenAction {
    /// Grava um golden master de referência a partir de uma simulação.
    /// Placeholder — chega com o motor v0 (M1).
    Record {
        #[arg(long)]
        seed: u64,
        #[arg(long, default_value_t = 3)]
        seasons: u32,
    },
    /// Verifica a simulação atual contra um golden master gravado.
    /// Placeholder — chega com o motor v0 (M1).
    Verify {
        #[arg(long)]
        seed: u64,
    },
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    match cli.command {
        Command::Version => {
            print_version();
            ExitCode::SUCCESS
        }
        Command::Bench { trials } => bench(trials),
        Command::Calibrate { seasons, seed } => not_yet(
            &format!("calibrate --seasons {seasons} --seed {seed}"),
            "M1/M2 — docs/08-qualidade-e-testes.md#4-calibração",
        ),
        Command::Pack {
            action: PackAction::Validate { path },
        } => pack_validate(&path),
        Command::Golden {
            action: GoldenAction::Record { seed, seasons },
        } => not_yet(
            &format!("golden record --seed {seed} --seasons {seasons}"),
            "M1 — docs/08-qualidade-e-testes.md#3-golden-masters",
        ),
        Command::Golden {
            action: GoldenAction::Verify { seed },
        } => not_yet(
            &format!("golden verify --seed {seed}"),
            "M1 — docs/08-qualidade-e-testes.md#3-golden-masters",
        ),
    }
}

fn print_version() {
    println!("managerfc-cli {}", env!("CARGO_PKG_VERSION"));
    println!(
        "domain        {} (crate ligado e testável)",
        env!("CARGO_PKG_VERSION")
    );
    // Smoke test real, não só decorativo: se `domain` não estivesse
    // corretamente ligado ao workspace, esta chamada nem compilaria.
    let seed_check = domain::Fixed::from_ratio(1, 3);
    println!("fixed-point   1/3 = {seed_check} (verificação de que o crate domain responde)");
}

/// Implementação real de `pack validate <path>` (`docs/03 §7`).
///
/// Erro estrutural (pack.toml ausente, JSON/TOML malformado) sai por
/// `stderr` com código 2, sem tentar validar semanticamente algo que nem
/// carregou direito. Problemas semânticos (referência quebrada, contagem de
/// clubes divergente...) são **todos** listados de uma vez — o objetivo é
/// que o autor do pack corrija numa passada, não descubra um erro por
/// execução (`docs/03 §7`: "validador... com mensagens de erro úteis").
fn pack_validate(path: &std::path::Path) -> ExitCode {
    let report = match pack::load_and_validate(path) {
        Ok(report) => report,
        Err(err) => {
            eprintln!(
                "managerfc-cli: falha ao carregar pack em {}: {err}",
                path.display()
            );
            return ExitCode::from(2);
        }
    };

    println!(
        "pack '{}' ({}) — {} país(es), {} competição(ões), {} clube(s)",
        report.manifest.id,
        report.manifest.version,
        report.pack.nations.len(),
        report.pack.competitions.len(),
        report.pack.clubs.len(),
    );

    if report.is_valid() {
        println!("válido — nenhum problema encontrado.");
        ExitCode::SUCCESS
    } else {
        eprintln!("{} problema(s) encontrado(s):", report.issues.len());
        for issue in &report.issues {
            eprintln!("  - {issue}");
        }
        ExitCode::from(1)
    }
}

/// Custo orçado de simular uma partida em modo instantâneo, em desktop
/// (`docs/01 §3.1`, RNF-01). O orçamento de mobile (≤ 6.000 µs) não é
/// verificado aqui — precisa do aparelho de referência (`docs/08 §6`), não
/// de uma constante neste binário.
const BENCH_BUDGET_DESKTOP_NANOS: u128 = 1_500_000;

/// Implementação real de `bench` — mede `engine::simulate` (`docs/04`),
/// hoje a única peça do núcleo com custo por partida para medir. Duas
/// forças iguais são usadas de propósito: é o caso que mais dispara o
/// motor (nem time nem o outro "resolve" cedo por diferença grande de
/// força), então tende a ser o teto de custo, não o típico.
fn bench(trials: u32) -> ExitCode {
    let home = engine::TeamStrength::new(domain::Fixed::from_int(100));
    let away = engine::TeamStrength::new(domain::Fixed::from_int(100));

    let start = std::time::Instant::now();
    let mut total_events = 0usize;
    for i in 0..trials {
        let ctx = engine::MatchContext {
            world_seed: 1,
            fixture: u64::from(i),
        };
        total_events += engine::simulate(home, away, ctx).len();
    }
    let elapsed = start.elapsed();
    // Nanossegundos, não microssegundos: o motor v0 é rápido o bastante
    // (bem abaixo de 1 µs/partida) para que "µs/partida" trunque para 0 e
    // pareça um bug em vez de uma boa notícia de performance.
    let per_match_nanos = elapsed.as_nanos() / u128::from(trials.max(1));

    println!(
        "engine::simulate — {trials} partidas em {elapsed:?} \
         ({per_match_nanos} ns/partida em média, {total_events} eventos no total)"
    );

    if per_match_nanos <= BENCH_BUDGET_DESKTOP_NANOS {
        println!(
            "dentro do orçamento de desktop (RNF-01: ≤ {BENCH_BUDGET_DESKTOP_NANOS} ns/partida)"
        );
        ExitCode::SUCCESS
    } else {
        eprintln!(
            "ACIMA do orçamento de desktop (RNF-01: ≤ {BENCH_BUDGET_DESKTOP_NANOS} ns/partida)"
        );
        ExitCode::from(1)
    }
}

/// Mensagem padrão para subcomandos ainda não implementados — nunca um
/// `panic!`/`unimplemented!`, porque isso quebraria `--help` e scripts que
/// sondam a CLI (`RNF`: núcleo devolve erro tipado, nunca panic — docs/02 §9.2).
fn not_yet(command: &str, milestone: &str) -> ExitCode {
    eprintln!("managerfc-cli: `{command}` ainda não implementado.");
    eprintln!("Previsto para: {milestone}");
    ExitCode::from(2)
}

#[cfg(test)]
mod tests {
    use clap::CommandFactory;

    use super::Cli;

    #[test]
    fn cli_esta_bem_formada() {
        // clap valida a definição de argumentos/subcomandos em tempo de
        // construção; isso pega erro de configuração (flags duplicadas,
        // conflito de nomes) sem precisar rodar o binário.
        Cli::command().debug_assert();
    }
}
