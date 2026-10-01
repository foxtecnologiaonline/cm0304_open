//! Checagem de tolerância de `calibrate` contra os alvos mensuráveis de
//! `docs/04-motor-de-partida.md §4.1`. Separado de `main.rs` para que a
//! lógica de comparação (pura, sem I/O nem `world::run_seasons`) tenha
//! testes unitários próprios, no mesmo espírito de `golden.rs`.
//!
//! Dos 11 alvos da tabela de docs/04 §4.1, só **5** são mensuráveis hoje:
//! gols/partida, vitórias do mandante, empates, finalizações/time e
//! conversão de finalizações — os únicos números que o motor v0.5 e
//! `world::run_seasons` de fato produzem. Os outros 6 (posse do vencedor,
//! cartões amarelos/vermelhos, lesões por 1.000 minutos, placares 0×0,
//! correlação CA médio × pontos) **não são checados aqui**, de propósito:
//! dependem de dados que não existem ainda (posse e cartões são motor
//! v1/v2; lesões hoje são sorteadas por temporada, não por partida; 0×0 e
//! a correlação exigem análise que ninguém implementou). Checar um alvo
//! sem o dado real por trás seria fingir uma garantia que não existe.

use std::fs::OpenOptions;
use std::io::Write as _;
use std::path::Path;

/// Um alvo numérico com tolerância, nas mesmas unidades inteiras
/// (per-mille ou pontos percentuais) que `main::calibrate` já calcula —
/// sem ponto flutuante, pelo mesmo motivo de `domain::Fixed`: determinismo
/// bit-a-bit entre plataformas não é opcional aqui.
pub struct Target {
    pub target: i64,
    pub tolerance: i64,
}

impl Target {
    #[must_use]
    pub fn check(&self, value: u64) -> bool {
        (value as i64 - self.target).abs() <= self.tolerance
    }
}

pub const GOALS_PER_MATCH_PERMILLE: Target = Target {
    target: 2700,
    tolerance: 150,
};
pub const HOME_WIN_PCT: Target = Target {
    target: 44,
    tolerance: 3,
};
pub const DRAW_PCT: Target = Target {
    target: 25,
    tolerance: 3,
};
pub const SHOTS_PER_TEAM_PERMILLE: Target = Target {
    target: 12_500,
    tolerance: 1_500,
};
pub const CONVERSION_PERMILLE: Target = Target {
    target: 105,
    tolerance: 15,
};

/// As métricas de uma rodada de `calibrate`, nas mesmas unidades que
/// `main::calibrate` já imprime (per-mille para gols/finalizações/conversão,
/// pontos percentuais inteiros para vitórias/empates).
pub struct Metrics {
    pub matches: u64,
    pub avg_goals_permille: u64,
    pub home_win_pct: u64,
    pub draw_pct: u64,
    pub avg_shots_per_team_permille: u64,
    pub conversion_permille: u64,
}

impl Metrics {
    #[must_use]
    pub fn all_within_tolerance(&self) -> bool {
        GOALS_PER_MATCH_PERMILLE.check(self.avg_goals_permille)
            && HOME_WIN_PCT.check(self.home_win_pct)
            && DRAW_PCT.check(self.draw_pct)
            && SHOTS_PER_TEAM_PERMILLE.check(self.avg_shots_per_team_permille)
            && CONVERSION_PERMILLE.check(self.conversion_permille)
    }
}

/// Acrescenta uma linha de CSV com as métricas desta rodada em `path`,
/// escrevendo o cabeçalho primeiro se o arquivo ainda não existir —
/// `docs/04 §4.2`: "`calibrate` roda temporadas headless e emite CSV".
/// Histórico entre rodadas (seeds/datas diferentes), nunca sobrescrito.
pub fn append_csv_row(
    path: &Path,
    seed: u64,
    seasons: u32,
    metrics: &Metrics,
) -> std::io::Result<()> {
    let write_header = !path.exists();
    let mut file = OpenOptions::new().create(true).append(true).open(path)?;
    if write_header {
        writeln!(
            file,
            "seed,seasons,matches,goals_per_match_permille,home_win_pct,draw_pct,shots_per_team_permille,conversion_permille"
        )?;
    }
    writeln!(
        file,
        "{seed},{seasons},{},{},{},{},{},{}",
        metrics.matches,
        metrics.avg_goals_permille,
        metrics.home_win_pct,
        metrics.draw_pct,
        metrics.avg_shots_per_team_permille,
        metrics.conversion_permille,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn metrics_no_alvo() -> Metrics {
        Metrics {
            matches: 1000,
            avg_goals_permille: 2700,
            home_win_pct: 44,
            draw_pct: 25,
            avg_shots_per_team_permille: 12_500,
            conversion_permille: 105,
        }
    }

    #[test]
    fn valor_exatamente_no_alvo_passa() {
        assert!(GOALS_PER_MATCH_PERMILLE.check(2700));
    }

    #[test]
    fn valor_na_borda_da_tolerancia_ainda_passa() {
        assert!(GOALS_PER_MATCH_PERMILLE.check(2700 + 150));
        assert!(GOALS_PER_MATCH_PERMILLE.check(2700 - 150));
    }

    #[test]
    fn valor_um_a_mais_que_a_borda_falha() {
        assert!(!GOALS_PER_MATCH_PERMILLE.check(2700 + 151));
        assert!(!GOALS_PER_MATCH_PERMILLE.check(2700 - 151));
    }

    #[test]
    fn metrics_dentro_do_alvo_passam_em_todas_as_checagens() {
        assert!(metrics_no_alvo().all_within_tolerance());
    }

    #[test]
    fn uma_unica_metrica_fora_do_alvo_falha_a_checagem_agregada() {
        let mut metrics = metrics_no_alvo();
        metrics.conversion_permille = 500;
        assert!(!metrics.all_within_tolerance());
    }

    #[test]
    fn append_csv_row_escreve_cabecalho_so_na_primeira_vez() {
        let dir = std::env::temp_dir().join(format!(
            "managerfc-cli-calibration-test-{}-{}",
            std::process::id(),
            line!()
        ));
        std::fs::create_dir_all(&dir).expect("cria diretório temporário do teste");
        let path = dir.join("calib.csv");

        let metrics = metrics_no_alvo();
        append_csv_row(&path, 1, 5, &metrics).expect("primeira escrita");
        append_csv_row(&path, 2, 5, &metrics).expect("segunda escrita, mesmo arquivo");

        let content = std::fs::read_to_string(&path).expect("lê o CSV de volta");
        let lines: Vec<&str> = content.lines().collect();
        assert_eq!(
            lines.len(),
            3,
            "cabeçalho + 2 linhas de dados, não 2 cabeçalhos"
        );
        assert!(lines[0].starts_with("seed,seasons,matches"));
        assert!(lines[1].starts_with("1,5,1000,"));
        assert!(lines[2].starts_with("2,5,1000,"));

        std::fs::remove_dir_all(&dir).expect("limpa o diretório temporário do teste");
    }
}
