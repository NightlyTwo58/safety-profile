//! Comparison tables and CSV files for the `compare` binary.

use crate::{stats::Summary, STAGE_NAMES};
use std::{fmt::Write as _, fs, path::Path};

/// Yosys measurements for one circuit.
#[derive(Debug, Clone, Copy)]
pub struct YosysRow {
    pub pipeline: Summary,
    pub rename: Summary,
    /// Rename pairs run per rename sample.
    pub rename_rounds: usize,
}

/// All measurements for one circuit.
#[derive(Debug)]
pub struct Row {
    pub name: String,
    /// Per-stage summaries in [`STAGE_NAMES`] order.
    pub stages: Vec<Summary>,
    pub ours: Summary,
    pub rename_ours: Summary,
    /// `None` when Yosys is disabled.
    pub yosys: Option<YosysRow>,
}

/// Collected rows and the Yosys startup baseline.
///
/// `startup` is `None` when Yosys is disabled, which drops every Yosys column.
pub struct Report {
    startup: Option<f64>,
    /// Include per-stage columns in the terminal table (CSVs always have them).
    stages: bool,
    rows: Vec<Row>,
}

const STAGES_CSV_HEADER: &str = "circuit,stage,median_us,min_us,sd_us,pct_of_total\n";

fn ratio(num: f64, den: f64) -> Option<f64> {
    (den > 0.0).then(|| num / den)
}

/// Format `v` with `decimals` places, or `missing` when absent.
fn cell(v: Option<f64>, decimals: usize, missing: &str) -> String {
    v.map_or_else(|| missing.into(), |v| format!("{v:.decimals$}"))
}

/// Format a speedup as `1.23x`, or `n/a`.
fn times(v: Option<f64>) -> String {
    v.map_or_else(|| "n/a".into(), |v| format!("{v:.2}x"))
}

impl Report {
    pub fn new(startup: Option<f64>, stages: bool) -> Self {
        Self {
            startup,
            stages,
            rows: Vec::new(),
        }
    }

    pub fn startup(&self) -> Option<f64> {
        self.startup
    }

    pub fn push(&mut self, row: Row) {
        self.rows.push(row);
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    /// Yosys median minus startup, floored at zero.
    fn yosys_net(&self, row: &Row) -> Option<f64> {
        Some((row.yosys?.pipeline.median - self.startup?).max(0.0))
    }

    fn speedup_raw(row: &Row) -> Option<f64> {
        ratio(row.yosys?.pipeline.median, row.ours.median)
    }

    fn speedup_net(&self, row: &Row) -> Option<f64> {
        ratio(self.yosys_net(row)?, row.ours.median)
    }

    fn rename_speedup(row: &Row) -> Option<f64> {
        ratio(row.yosys?.rename.median, row.rename_ours.median)
    }

    /// Title, column header, and rule for the pipeline table.
    pub fn table_header(&self) -> String {
        let mut line = format!("{:<18}", "circuit");
        if self.stages {
            for s in STAGE_NAMES {
                let _ = write!(line, " {s:>9}");
            }
        }
        if self.startup.is_some() {
            let _ = write!(
                line,
                " {:>10} {:>10} {:>10} {:>8} {:>8}",
                "ours", "yosys", "yosys-net", "x-raw", "x-net"
            );
        } else {
            let _ = write!(line, " {:>10}", "ours");
        }
        let rule = "─".repeat(line.chars().count());
        format!("=== Pipeline Comparison (median µs over timed iterations) ===\n\n{line}\n{rule}\n")
    }

    pub fn table_row(&self, row: &Row) -> String {
        let mut line = format!("{:<18}", row.name);
        if self.stages {
            for s in &row.stages {
                let _ = write!(line, " {:>9.1}", s.median);
            }
        }
        let _ = write!(line, " {:>10.1}", row.ours.median);
        if self.startup.is_some() {
            let _ = write!(
                line,
                " {:>10} {:>10} {:>8} {:>8}",
                cell(row.yosys.map(|y| y.pipeline.median), 1, "n/a"),
                cell(self.yosys_net(row), 1, "n/a"),
                times(Self::speedup_raw(row)),
                times(self.speedup_net(row)),
            );
        }
        line.push('\n');
        line
    }

    pub fn summary_csv_header(&self) -> String {
        let mut h = String::from("circuit");
        for s in STAGE_NAMES {
            let _ = write!(h, ",{s}_us");
        }
        h.push_str(",ours_median_us,ours_min_us,ours_sd_us");
        if self.startup.is_some() {
            h.push_str(",yosys_median_us,yosys_sd_us,yosys_startup_us,speedup_raw,speedup_net");
        }
        h.push_str(",iters\n");
        h
    }

    pub fn summary_csv_row(&self, row: &Row) -> String {
        let mut line = row.name.clone();
        for s in &row.stages {
            let _ = write!(line, ",{:.1}", s.median);
        }
        let _ = write!(
            line,
            ",{:.1},{:.1},{:.1}",
            row.ours.median, row.ours.min, row.ours.stddev
        );
        if let Some(startup) = self.startup {
            let _ = write!(
                line,
                ",{},{},{startup:.1},{},{}",
                cell(row.yosys.map(|y| y.pipeline.median), 1, ""),
                cell(row.yosys.map(|y| y.pipeline.stddev), 1, ""),
                cell(Self::speedup_raw(row), 3, ""),
                cell(self.speedup_net(row), 3, ""),
            );
        }
        let _ = writeln!(line, ",{}", row.ours.n);
        line
    }

    /// Per-stage CSV rows with each stage's share of the measured total.
    fn stages_csv_rows(row: &Row) -> String {
        let sum: f64 = row.stages.iter().map(|s| s.median).sum();
        let mut out = String::new();
        for (name, s) in STAGE_NAMES.iter().zip(&row.stages) {
            let pct = if sum > 0.0 {
                100.0 * s.median / sum
            } else {
                0.0
            };
            let _ = writeln!(
                out,
                "{},{name},{:.1},{:.1},{:.1},{pct:.1}",
                row.name, s.median, s.min, s.stddev
            );
        }
        out
    }

    /// Table comparing sequential renaming across all collected rows.
    pub fn rename_table(&self) -> String {
        let mut out =
            String::from("\n=== Sequential RenameNets comparison (µs per operation) ===\n");
        let _ = write!(
            out,
            "{:<18} {:>12} {:>10} {:>10}",
            "circuit", "ours median", "ours min", "ours sd"
        );
        if self.startup.is_some() {
            let _ = write!(
                out,
                " {:>16} {:>10} {:>10} {:>11}",
                "Yosys median", "Yosys min", "Yosys sd", "Yosys/ours"
            );
        }
        out.push('\n');
        for row in &self.rows {
            let _ = write!(
                out,
                "{:<18} {:>12.1} {:>10.1} {:>10.1}",
                row.name, row.rename_ours.median, row.rename_ours.min, row.rename_ours.stddev
            );
            if let Some(y) = row.yosys {
                let _ = write!(
                    out,
                    " {:>16.1} {:>10.1} {:>10.1} {:>11}",
                    y.rename.median,
                    y.rename.min,
                    y.rename.stddev,
                    times(Self::rename_speedup(row)),
                );
            }
            out.push('\n');
        }
        out
    }

    fn rename_csv_header(&self) -> &'static str {
        if self.startup.is_some() {
            "circuit,ours_median_us,ours_min_us,ours_sd_us,yosys_hide_enumerate_median_us,yosys_min_us,yosys_sd_us,yosys_over_ours,iters,yosys_pairs_per_sample\n"
        } else {
            "circuit,ours_median_us,ours_min_us,ours_sd_us,iters\n"
        }
    }

    fn rename_csv_row(&self, row: &Row) -> String {
        let ours = &row.rename_ours;
        let mut line = format!(
            "{},{:.1},{:.1},{:.1}",
            row.name, ours.median, ours.min, ours.stddev
        );
        if let Some(y) = row.yosys {
            let _ = write!(
                line,
                ",{:.1},{:.1},{:.1},{},{},{}",
                y.rename.median,
                y.rename.min,
                y.rename.stddev,
                cell(Self::rename_speedup(row), 3, ""),
                ours.n,
                y.rename_rounds,
            );
        } else {
            let _ = write!(line, ",{}", ours.n);
        }
        line.push('\n');
        line
    }

    /// Write `compare.csv`, `compare_stages.csv`, and `compare_rename.csv` into `dir`.
    pub fn write_csvs(&self, dir: &Path) -> Result<(), String> {
        let mut summary = self.summary_csv_header();
        let mut stages = String::from(STAGES_CSV_HEADER);
        let mut rename = String::from(self.rename_csv_header());
        for row in &self.rows {
            summary.push_str(&self.summary_csv_row(row));
            stages.push_str(&Self::stages_csv_rows(row));
            rename.push_str(&self.rename_csv_row(row));
        }
        for (file, body) in [
            ("compare.csv", &summary),
            ("compare_stages.csv", &stages),
            ("compare_rename.csv", &rename),
        ] {
            let path = dir.join(file);
            fs::write(&path, body).map_err(|e| format!("{}: {e}", path.display()))?;
        }
        Ok(())
    }
}
