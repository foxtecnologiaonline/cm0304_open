//! `managerfc-cli` — binário headless do núcleo (`docs/02-arquitetura.md §10`).
//!
//! É o único jeito de rodar o núcleo sem UI: CI, balanceamento e modders
//! passam por aqui, nunca pelo Flutter (RNF-13). `version`, `pack validate`,
//! `bench`, `calibrate`, `play`, `save`, `load`, `golden record` e `golden
//! verify` são todos reais.
//!
//! `play` é diferente dos outros: não existe pra medir nada, existe pra
//! provar que `app::GameSession` — a futura fronteira com a ponte
//! `flutter_rust_bridge` — funciona de fora do próprio crate que a
//! implementa. Todo teste de `app` roda dentro do crate; `play` é o
//! primeiro consumidor externo do `dispatch`/`query` real. `golden` reusa
//! o mesmo caminho (ver `golden.rs`) — pega regressão em qualquer parte do
//! pipeline (motor, progressão, mercado, lesões), não só no motor. `calibrate
//! --check` (ver `calibration.rs`) é o outro portão: falha se alguma das 5
//! métricas hoje mensuráveis sair da tolerância de `docs/04 §4.1` — é o que
//! roda no job `cli-gate` da CI.

use std::path::PathBuf;
use std::process::ExitCode;

use clap::{Parser, Subcommand};

mod calibration;
mod golden;

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
    /// não são medíveis — não porque falte o `world` (já existe), mas
    /// porque as partidas dele hoje são identificadas por rodada, não por
    /// data de calendário (`domain::GameDate`); "1 dia de calendário
    /// mundial" só faz sentido depois que o calendário tiver datas de
    /// verdade.
    Bench {
        /// Quantas partidas simular para tirar a média.
        #[arg(long, default_value_t = 10_000)]
        trials: u32,
    },

    /// Simula temporadas headless e emite métricas de calibração (`docs/08 §4`).
    Calibrate {
        /// Número de temporadas a simular.
        #[arg(long, default_value_t = 20)]
        seasons: u32,
        /// Seed do mundo.
        #[arg(long, default_value_t = 42)]
        seed: u64,
        /// Diretório do pack a usar. Sem isto, usa o pack de exemplo do
        /// próprio repositório (`packs/core/example-two-tier`) — suficiente
        /// pra exercitar o motor, não para calibração de verdade contra uma
        /// base de escala real (essa espera o pipeline de `docs/05 §3.3`).
        #[arg(long)]
        pack: Option<PathBuf>,
        /// Falha (exit code != 0) se alguma métrica medível sair da
        /// tolerância de `docs/04 §4.1` — é isto que transforma `calibrate`
        /// num portão de CI (`docs/07` M1: "primeiras tolerâncias em CI"),
        /// em vez de só um relatório pra ler.
        #[arg(long)]
        check: bool,
        /// Acrescenta uma linha de CSV com as métricas desta rodada neste
        /// caminho (cabeçalho escrito se o arquivo ainda não existe) —
        /// `docs/04 §4.2`: "calibrate roda temporadas headless e emite CSV".
        #[arg(long)]
        csv: Option<PathBuf>,
    },

    /// Joga N temporadas via `app::GameSession` — o mesmo `dispatch`/`query`
    /// que a futura ponte com o Flutter vai chamar (`docs/02 §4`) — e
    /// imprime a tabela final de cada competição. Existe pra provar que a
    /// fronteira funciona de fora do crate `app`, não pra medir nada.
    Play {
        /// Número de temporadas a avançar.
        #[arg(long, default_value_t = 3)]
        seasons: u32,
        /// Seed do mundo.
        #[arg(long, default_value_t = 42)]
        seed: u64,
        /// Diretório do pack a usar. Sem isto, usa o pack de exemplo do
        /// próprio repositório.
        #[arg(long)]
        pack: Option<PathBuf>,
    },

    /// Avança N temporadas via `app::GameSession` e grava o resultado num
    /// arquivo de save (`persist`, `docs/03 §8.1`) — o mesmo papel de `play`
    /// para `GameSession::save_to_path`: provar que funciona de fora do
    /// crate `app`.
    Save {
        /// Número de temporadas a avançar antes de salvar.
        #[arg(long, default_value_t = 3)]
        seasons: u32,
        /// Seed do mundo.
        #[arg(long, default_value_t = 42)]
        seed: u64,
        /// Diretório do pack a usar. Sem isto, usa o pack de exemplo do
        /// próprio repositório.
        #[arg(long)]
        pack: Option<PathBuf>,
        /// Caminho do arquivo de save a gravar.
        path: PathBuf,
    },

    /// Carrega um save (por replay determinístico — ver `persist::save` para
    /// o porquê disso bastar hoje) e imprime a tabela final de cada
    /// competição, no mesmo formato de `play`.
    Load {
        /// Diretório do pack a usar — precisa ser o mesmo pack (id + versão)
        /// gravado no save, senão `load` recusa (`AppError::SavePackMismatch`).
        #[arg(long)]
        pack: Option<PathBuf>,
        /// Caminho do arquivo de save a carregar.
        path: PathBuf,
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
    /// Grava um golden master de referência a partir de uma simulação
    /// (`docs/08 §3`). Só regrave intencionalmente — nunca "pra fazer a
    /// CI passar" (mesma seção, a regra é verificada na revisão).
    Record {
        #[arg(long)]
        seed: u64,
        #[arg(long, default_value_t = 3)]
        seasons: u32,
        /// Diretório do pack a usar. Sem isto, usa o pack de exemplo do
        /// próprio repositório.
        #[arg(long)]
        pack: Option<PathBuf>,
        /// Caminho do arquivo a gravar.
        #[arg(long)]
        out: PathBuf,
    },
    /// Verifica a simulação atual contra um golden master gravado
    /// (`docs/08 §3`). `--seasons` não existe aqui de propósito: `verify`
    /// reproduz exatamente o que `record` gravou, nunca uma contagem de
    /// temporadas diferente por engano.
    Verify {
        #[arg(long)]
        seed: u64,
        /// Diretório do pack a usar. Sem isto, usa o pack de exemplo do
        /// próprio repositório.
        #[arg(long)]
        pack: Option<PathBuf>,
        /// Caminho do golden master gravado por `golden record`.
        #[arg(long)]
        expect: PathBuf,
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
        Command::Calibrate {
            seasons,
            seed,
            pack,
            check,
            csv,
        } => calibrate(seasons, seed, pack, check, csv.as_deref()),
        Command::Play {
            seasons,
            seed,
            pack,
        } => play(seasons, seed, pack),
        Command::Save {
            seasons,
            seed,
            pack,
            path,
        } => save_command(seasons, seed, pack, &path),
        Command::Load { pack, path } => load_command(pack, &path),
        Command::Pack {
            action: PackAction::Validate { path },
        } => pack_validate(&path),
        Command::Golden {
            action:
                GoldenAction::Record {
                    seed,
                    seasons,
                    pack,
                    out,
                },
        } => golden::record(seed, seasons, pack, &out),
        Command::Golden {
            action: GoldenAction::Verify { seed, pack, expect },
        } => golden::verify(seed, pack, &expect),
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
        "pack '{}' ({}) — {} país(es), {} competição(ões), {} clube(s), {} jogador(es)",
        report.manifest.id,
        report.manifest.version,
        report.pack.nations.len(),
        report.pack.competitions.len(),
        report.pack.clubs.len(),
        report.pack.players.len(),
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

/// Caminho do pack de exemplo do próprio repositório — usado como padrão de
/// `calibrate` quando `--pack` não é informado. Resolvido a partir de
/// `CARGO_MANIFEST_DIR` (fixado em tempo de compilação): funciona enquanto
/// o binário for usado dentro do próprio checkout do repositório (o caso de
/// CI e de desenvolvimento local), não é um caminho válido para uma versão
/// distribuída fora dele — quando isso importar, `--pack` deixa de ser
/// opcional ou passa a ter um default de outra natureza (pack empacotado
/// junto do binário, por exemplo).
pub(crate) fn default_example_pack_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../packs/core/example-two-tier")
}

/// Implementação real de `calibrate` (`docs/08 §4`). Carrega um pack, roda
/// `seasons` temporadas via `world::run_seasons` e imprime as métricas
/// agregadas contra os alvos de `docs/04 §4.1`. Não é a suíte de calibração
/// completa (histórico versionado, distribuição de placares, correlação
/// CA×pontos — essa última nem faz sentido ainda, sem CA de jogador de
/// verdade) — é o suficiente para saber se o motor está grosseiramente
/// fora, o que já é mais do que existia antes deste comando.
fn calibrate(
    seasons: u32,
    seed: u64,
    pack_path: Option<PathBuf>,
    check: bool,
    csv_path: Option<&std::path::Path>,
) -> ExitCode {
    let path = pack_path.unwrap_or_else(default_example_pack_path);
    let report = match pack::load_and_validate(&path) {
        Ok(report) => report,
        Err(err) => {
            eprintln!(
                "managerfc-cli: falha ao carregar pack em {}: {err}",
                path.display()
            );
            return ExitCode::from(2);
        }
    };
    if !report.is_valid() {
        eprintln!(
            "managerfc-cli: pack em {} tem {} problema(s) — rode `pack validate` antes de calibrar:",
            path.display(),
            report.issues.len()
        );
        for issue in &report.issues {
            eprintln!("  - {issue}");
        }
        return ExitCode::from(2);
    }

    println!(
        "calibrando com pack '{}' — seed={seed}, {seasons} temporada(s)",
        report.manifest.id
    );
    let results = world::run_seasons(&report.pack, seed, seasons);

    let matches: u64 = results.iter().map(|r| u64::from(r.matches_played)).sum();
    let goals: u64 = results.iter().map(|r| u64::from(r.total_goals)).sum();
    let shots: u64 = results.iter().map(|r| u64::from(r.total_shots)).sum();
    let home_wins: u64 = results.iter().map(|r| u64::from(r.home_wins)).sum();
    let away_wins: u64 = results.iter().map(|r| u64::from(r.away_wins)).sum();
    let draws: u64 = results.iter().map(|r| u64::from(r.draws)).sum();

    if matches == 0 {
        eprintln!("managerfc-cli: nenhuma partida simulada (pack sem competições jogáveis?)");
        return ExitCode::from(1);
    }

    let away_win_pct = away_wins * 100 / matches;
    let metrics = calibration::Metrics {
        matches,
        avg_goals_permille: goals * 1000 / matches,
        home_win_pct: home_wins * 100 / matches,
        draw_pct: draws * 100 / matches,
        // Finalizações por time (não por partida): docs/04 §4.1 mira 12,5
        // por time, então divide por 2 lados além de por partida.
        avg_shots_per_team_permille: shots * 1000 / matches / 2,
        conversion_permille: if shots == 0 { 0 } else { goals * 1000 / shots },
    };

    let badge = |ok: bool| if ok { "OK  " } else { "FORA" };
    println!("{matches} partidas simuladas ao todo");
    println!(
        "[{}] gols/partida:          {}.{:03}  (alvo docs/04 §4.1: 2.700 ± 0.15)",
        badge(calibration::GOALS_PER_MATCH_PERMILLE.check(metrics.avg_goals_permille)),
        metrics.avg_goals_permille / 1000,
        metrics.avg_goals_permille % 1000
    );
    println!(
        "[{}] vitórias do mandante:  {}%   (alvo: ~44% ± 3pp)",
        badge(calibration::HOME_WIN_PCT.check(metrics.home_win_pct)),
        metrics.home_win_pct
    );
    println!(
        "[{}] empates:               {}%   (alvo: ~25% ± 3pp)",
        badge(calibration::DRAW_PCT.check(metrics.draw_pct)),
        metrics.draw_pct
    );
    println!("     vitórias do visitante: {away_win_pct}%   (complemento, sem alvo próprio)");
    println!(
        "[{}] finalizações/time:     {}.{:03}  (alvo docs/04 §4.1: 12.500 ± 1.5)",
        badge(calibration::SHOTS_PER_TEAM_PERMILLE.check(metrics.avg_shots_per_team_permille)),
        metrics.avg_shots_per_team_permille / 1000,
        metrics.avg_shots_per_team_permille % 1000
    );
    println!(
        "[{}] conversão de chutes:   {}.{}%  (alvo docs/04 §4.1: 10.5% ± 1.5pp)",
        badge(calibration::CONVERSION_PERMILLE.check(metrics.conversion_permille)),
        metrics.conversion_permille / 10,
        metrics.conversion_permille % 10
    );
    println!(
        "(posse, cartões, lesões/1000min, placares 0x0 e correlação CA×pontos de docs/04 §4.1 \
         não são medidos ainda — dependem de dados que o motor não produz, ver cli/src/calibration.rs)"
    );

    if let Some(csv_path) = csv_path {
        if let Err(err) = calibration::append_csv_row(csv_path, seed, seasons, &metrics) {
            eprintln!(
                "managerfc-cli: falha ao escrever CSV em {}: {err}",
                csv_path.display()
            );
            return ExitCode::from(2);
        }
        println!("CSV atualizado em {}", csv_path.display());
    }

    if check && !metrics.all_within_tolerance() {
        eprintln!(
            "managerfc-cli: calibração fora da tolerância de docs/04 §4.1 (ver [FORA] acima)"
        );
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}

/// Implementação de `play` — usa `app::GameSession` exatamente como uma UI
/// usaria: cria a sessão, despacha `AdvanceSeason` N vezes, consulta a
/// tabela final de cada competição. Nenhuma lógica de jogo mora aqui — só
/// carregar entrada, despachar/consultar e imprimir (`docs/02 §4`).
fn play(seasons: u32, seed: u64, pack_path: Option<PathBuf>) -> ExitCode {
    let path = pack_path.unwrap_or_else(default_example_pack_path);
    let mut session = match app::GameSession::new(&path, seed) {
        Ok(session) => session,
        Err(err) => {
            eprintln!(
                "managerfc-cli: falha ao iniciar sessão com pack em {}: {err}",
                path.display()
            );
            return ExitCode::from(2);
        }
    };

    println!("sessão iniciada — seed={seed}, pack em {}", path.display());
    for _ in 0..seasons {
        match session.dispatch(app::Command::AdvanceSeason) {
            Ok(receipt) => println!(
                "temporada {}: {} partidas, {} movimentação(ões) de acesso/queda, \
                 {} transferência(s), {} lesão(ões), {} jogador(es) cansado(s) pra próxima",
                receipt.season_index + 1,
                receipt.matches_played,
                receipt.movements,
                receipt.transfers,
                receipt.injuries,
                receipt.tired_players
            ),
            Err(err) => {
                eprintln!("managerfc-cli: falha ao avançar temporada: {err}");
                return ExitCode::from(1);
            }
        }
    }

    print_all_standings(&session);
    ExitCode::SUCCESS
}

/// Imprime a tabela final de cada competição da sessão — extraído de `play`
/// para ser reaproveitado por `load` (mesmo formato de saída para os dois,
/// já que os dois só diferem em como a sessão foi construída).
fn print_all_standings(session: &app::GameSession) {
    let app::QueryResult::Competitions(competitions) = session.query(app::Query::Competitions)
    else {
        unreachable!("Query::Competitions sempre devolve QueryResult::Competitions")
    };

    for competition in competitions {
        println!(
            "\n=== {} ({}) ===",
            competition.name, competition.nation_name
        );
        // `kind` já diz qual consulta faz sentido para esta competição —
        // poupa perguntar a outra, que sempre devolveria vazio/`None`
        // (`app::CompetitionKind`).
        if competition.kind == app::CompetitionKind::Cup {
            match session.query(app::Query::CupChampion {
                competition: competition.id,
            }) {
                app::QueryResult::CupChampion(Some(champion)) => println!(
                    "campeão: {}  ({} partida(s) em {} rodada(s) de mata-mata)",
                    champion.champion_club_name, champion.matches_played, champion.rounds_played
                ),
                _ => println!("(copa ainda não rodou nenhuma partida)"),
            }
            continue;
        }
        let app::QueryResult::Standings(Some(table)) = session.query(app::Query::Standings {
            competition: competition.id,
        }) else {
            println!("(sem tabela — competição não rodou nenhuma partida)");
            continue;
        };
        println!("pos  clube                  J   V   E   D   GP  GC  PTS");
        for row in table {
            println!(
                "{:>3}  {:<20}  {:>2}  {:>2}  {:>2}  {:>2}  {:>3} {:>3}  {:>3}",
                row.position,
                row.club_name,
                row.played,
                row.wins,
                row.draws,
                row.losses,
                row.goals_for,
                row.goals_against,
                row.points
            );
        }
    }
}

/// Implementação de `save` — avança `seasons` temporadas como `play`, mas em
/// vez de imprimir a tabela, grava a sessão em `path` via
/// `GameSession::save_to_path` (`persist`, escrita atômica).
fn save_command(
    seasons: u32,
    seed: u64,
    pack_path: Option<PathBuf>,
    path: &std::path::Path,
) -> ExitCode {
    let pack_dir = pack_path.unwrap_or_else(default_example_pack_path);
    let mut session = match app::GameSession::new(&pack_dir, seed) {
        Ok(session) => session,
        Err(err) => {
            eprintln!(
                "managerfc-cli: falha ao iniciar sessão com pack em {}: {err}",
                pack_dir.display()
            );
            return ExitCode::from(2);
        }
    };

    for _ in 0..seasons {
        if let Err(err) = session.dispatch(app::Command::AdvanceSeason) {
            eprintln!("managerfc-cli: falha ao avançar temporada: {err}");
            return ExitCode::from(1);
        }
    }

    if let Err(err) = session.save_to_path(path) {
        eprintln!(
            "managerfc-cli: falha ao gravar save em {}: {err}",
            path.display()
        );
        return ExitCode::from(2);
    }

    println!(
        "save gravado em {} — seed={seed}, {seasons} temporada(s), pack '{}'",
        path.display(),
        session.to_save().pack_id
    );
    ExitCode::SUCCESS
}

/// Implementação de `load` — o inverso de `save_command`: lê o arquivo,
/// reconstrói a `GameSession` por replay (`GameSession::load_from_path`) e
/// imprime a mesma tabela que `play` imprimiria se tivesse rodado do zero
/// até este ponto — prova que save e replay produzem o mesmo mundo.
fn load_command(pack_path: Option<PathBuf>, path: &std::path::Path) -> ExitCode {
    let pack_dir = pack_path.unwrap_or_else(default_example_pack_path);
    let session = match app::GameSession::load_from_path(&pack_dir, path) {
        Ok(session) => session,
        Err(err) => {
            eprintln!(
                "managerfc-cli: falha ao carregar save de {} com pack em {}: {err}",
                path.display(),
                pack_dir.display()
            );
            return ExitCode::from(2);
        }
    };

    let app::QueryResult::CurrentSeason(season) = session.query(app::Query::CurrentSeason) else {
        unreachable!("Query::CurrentSeason sempre devolve QueryResult::CurrentSeason")
    };
    println!(
        "save carregado de {} — {season} temporada(s) reproduzida(s) por replay",
        path.display()
    );
    print_all_standings(&session);
    ExitCode::SUCCESS
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
    let neutral_profile = engine::TeamMatchProfile {
        strength: engine::TeamStrength::new(domain::Fixed::from_int(100)),
        finishing: engine::FinishingQuality::new(domain::Fixed::from_int(10)),
        goalkeeping: engine::GoalkeepingQuality::new(domain::Fixed::from_int(10)),
    };
    let home = neutral_profile;
    let away = neutral_profile;

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
