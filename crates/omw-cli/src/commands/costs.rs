//! `omw costs` — render a cost rollup table from the local SQLite store.

use std::io::Write;

use anyhow::{anyhow, Context};
use chrono::{Duration, NaiveDate, TimeZone, Utc};
use clap::{Args, ValueEnum};

use crate::db::{self, GroupBy, RollupRow};

#[derive(Copy, Clone, Debug, ValueEnum)]
pub(crate) enum GroupByArg {
    Provider,
    Model,
    Day,
}

impl From<GroupByArg> for GroupBy {
    fn from(g: GroupByArg) -> Self {
        match g {
            GroupByArg::Provider => GroupBy::Provider,
            GroupByArg::Model => GroupBy::Model,
            GroupByArg::Day => GroupBy::Day,
        }
    }
}

#[derive(Args, Debug)]
pub struct CostsArgs {
    /// Start date (YYYY-MM-DD, UTC). Defaults to 30 days ago.
    #[arg(long)]
    pub since: Option<String>,
    /// Grouping axis.
    #[arg(long, value_enum, default_value_t = GroupByArg::Provider)]
    pub by: GroupByArg,
}

pub(crate) fn run(
    args: CostsArgs,
    stdout: &mut dyn Write,
    _stderr: &mut dyn Write,
) -> anyhow::Result<()> {
    // Default since = 30 days ago at 00:00 UTC.
    let since = match args.since {
        Some(s) => {
            let date = NaiveDate::parse_from_str(&s, "%Y-%m-%d")
                .with_context(|| format!("parsing --since `{s}` (expected YYYY-MM-DD)"))?;
            Some(
                Utc.from_utc_datetime(
                    &date
                        .and_hms_opt(0, 0, 0)
                        .ok_or_else(|| anyhow!("invalid date"))?,
                ),
            )
        }
        None => Some(Utc::now() - Duration::days(30)),
    };

    // If the data dir resolves to a non-directory or the db can't be
    // opened, treat it as an empty rollup. `omw costs` is a read-only
    // reporter and should not fail noisily on a missing db.
    let conn = match db::open() {
        Ok(c) => c,
        Err(_) => {
            writeln!(stdout, "no usage records found")?;
            return Ok(());
        }
    };

    let rows = db::cost_rollup(&conn, since, args.by.into())?;
    if rows.is_empty() {
        writeln!(stdout, "no usage records found")?;
        return Ok(());
    }

    render_table(stdout, &rows)?;
    Ok(())
}

fn render_table(stdout: &mut dyn Write, rows: &[RollupRow]) -> anyhow::Result<()> {
    // Compute totals.
    let mut total_prompt: i64 = 0;
    let mut total_completion: i64 = 0;
    let mut total_calls: i64 = 0;
    let mut total_cost: Option<i64> = None;
    for r in rows {
        total_prompt += r.total_prompt_tokens;
        total_completion += r.total_completion_tokens;
        total_calls += r.call_count;
        if let Some(c) = r.total_cost_cents {
            total_cost = Some(total_cost.unwrap_or(0) + c);
        }
    }

    // Render rows + Total into String form first so we can size columns.
    let header = ["KEY", "CALLS", "PROMPT_TOK", "COMPL_TOK", "COST"];
    let mut data: Vec<[String; 5]> = Vec::with_capacity(rows.len() + 1);
    for r in rows {
        data.push([
            r.key.clone(),
            r.call_count.to_string(),
            r.total_prompt_tokens.to_string(),
            r.total_completion_tokens.to_string(),
            format_cost(r.total_cost_cents),
        ]);
    }
    data.push([
        "Total".to_string(),
        total_calls.to_string(),
        total_prompt.to_string(),
        total_completion.to_string(),
        format_cost(total_cost),
    ]);

    let mut widths = [0usize; 5];
    for (i, h) in header.iter().enumerate() {
        widths[i] = h.len();
    }
    for row in &data {
        for (i, cell) in row.iter().enumerate() {
            if cell.len() > widths[i] {
                widths[i] = cell.len();
            }
        }
    }

    // Header.
    write_row(stdout, &header.map(String::from), &widths)?;
    // Separator.
    let sep: [String; 5] = std::array::from_fn(|i| "-".repeat(widths[i]));
    write_row(stdout, &sep, &widths)?;
    // Body, but render the Total row after a blank-ish separator.
    let body_len = data.len() - 1;
    for row in data.iter().take(body_len) {
        write_row(stdout, row, &widths)?;
    }
    // Trailing separator before Total.
    write_row(stdout, &sep, &widths)?;
    if let Some(total_row) = data.last() {
        write_row(stdout, total_row, &widths)?;
    }
    Ok(())
}

fn write_row(stdout: &mut dyn Write, row: &[String; 5], widths: &[usize; 5]) -> anyhow::Result<()> {
    // KEY left-aligned, all numeric columns right-aligned.
    writeln!(
        stdout,
        "{:<w0$}  {:>w1$}  {:>w2$}  {:>w3$}  {:>w4$}",
        row[0],
        row[1],
        row[2],
        row[3],
        row[4],
        w0 = widths[0],
        w1 = widths[1],
        w2 = widths[2],
        w3 = widths[3],
        w4 = widths[4],
    )?;
    Ok(())
}

fn format_cost(cents: Option<i64>) -> String {
    match cents {
        Some(c) => {
            // Render as $X.YY (negative impossible here, but be defensive).
            let dollars = c / 100;
            let frac = (c % 100).unsigned_abs();
            format!("${dollars}.{frac:02}")
        }
        None => "-".to_string(),
    }
}
