//! `managerfc-cli` — binário headless do núcleo (`docs/02-arquitetura.md §10`).
//!
//! É o único jeito de rodar o núcleo sem UI: CI, balanceamento e modders
//! passam por aqui, nunca pelo Flutter (RNF-13). Neste estágio (M0) a maior
//! parte dos subcomandos é **placeholder documentado** — existem para que a
//! forma final da CLI já apareça em `--help` desde o primeiro commit, mesmo
//! antes de `engine`/`world`/`pack` terem regra de negócio real para expor.
//! Cada placeholder diz explicitamente em que marco passa a funcionar
//! (`docs/07-roadmap.md`), para não ser confundido com um comando quebrado.

use clap::{Parser, Subcommand};
use std::process::ExitCode;

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

    /// Roda os benchmarks de performance contra os orçamentos de `docs/01 §3.1`.
    /// Placeholder — chega com o motor v0 (M1, `docs/07-roadmap.md`).
    Bench,

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
    /// Valida um data pack contra o schema e as regras de consistência.
    /// Placeholder — chega com o crate `pack` (M4, `docs/07-roadmap.md`).
    Validate {
        /// Caminho do pack (diretório ou `.fmpack`).
        path: String,
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
        Command::Bench => not_yet(
            "bench",
            "M1 — motor v0, docs/07-roadmap.md#m1--kick-off-headless-10-semanas",
        ),
        Command::Calibrate { seasons, seed } => not_yet(
            &format!("calibrate --seasons {seasons} --seed {seed}"),
            "M1/M2 — docs/08-qualidade-e-testes.md#4-calibração",
        ),
        Command::Pack {
            action: PackAction::Validate { path },
        } => not_yet(
            &format!("pack validate {path}"),
            "M4 — crate pack, docs/07-roadmap.md#m4--beta-14-semanas",
        ),
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
