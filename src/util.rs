use std::cmp::min;
use std::fmt::Write;
use std::path::Path;
use std::time::Duration;

use anyhow::{anyhow, Context};
use indicatif::{HumanDuration, ProgressState, ProgressStyle};
use number_prefix::NumberPrefix;
use prettytable::{row, table};
use statrs::statistics::{Data, Distribution, Max, Min, OrderStatistics};
use tracing::{error, level_filters::LevelFilter};
use tracing_error::ErrorLayer;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

#[allow(clippy::as_conversions)]
#[allow(clippy::cast_possible_truncation)]
#[allow(clippy::cast_precision_loss)]
#[allow(clippy::cast_sign_loss)]
pub fn create_progress_style(template: &str) -> anyhow::Result<ProgressStyle> {
    let progress_style = ProgressStyle::with_template(template)
        .with_context(|| format!("Unable to create progress bar style with template '{template}'"))?
        .with_key("smooth_eta", |s: &ProgressState, w: &mut dyn Write| {
            match (s.pos(), s.len()) {
                (pos, Some(len)) if pos > 0 => write!(
                    w,
                    "{:#}",
                    HumanDuration(Duration::from_millis(
                        (s.elapsed().as_millis() as f64 * (len as f64 - pos as f64) / pos as f64)
                            .round() as u64
                    ))
                ),
                _ => write!(w, "-"),
            }
            .unwrap_or_else(|err| {
                error!("Unexpected error while formatting smooth_eta in progress bar: {err}");
            });
        })
        .with_key("smooth_per_sec", |s: &ProgressState, w: &mut dyn Write| {
            match (s.pos(), s.elapsed().as_millis()) {
                (pos, elapsed_ms) if elapsed_ms > 0 => {
                    write!(w, "{:.2}", pos as f64 * 1000_f64 / elapsed_ms as f64)
                }
                _ => write!(w, "-"),
            }
            .unwrap_or_else(|err| {
                error!("Unexpected error while formatting smooth_per_sec in progress bar: {err}");
            });
        });

    Ok(progress_style)
}

pub fn install_tracing() -> anyhow::Result<()> {
    ffmpeg::util::log::set_level(ffmpeg::util::log::level::Level::Fatal);

    let env_filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::WARN.into())
        .from_env_lossy();

    let fmt_layer = tracing_subscriber::fmt::layer();

    tracing_subscriber::registry()
        .with(ErrorLayer::default())
        .with(fmt_layer.with_filter(env_filter))
        .try_init()
        .context("Unable to initialize global default subscriber")?;

    Ok(())
}

#[allow(clippy::as_conversions)]
#[allow(clippy::cast_possible_truncation)]
#[allow(clippy::cast_precision_loss)]
#[allow(clippy::cast_sign_loss)]
#[allow(clippy::print_stdout)]
pub fn print_histogram(data: &[f64]) -> anyhow::Result<()> {
    let min_value = data.iter().copied().fold(f64::INFINITY, f64::min);
    let max_value = data.iter().copied().fold(f64::NEG_INFINITY, f64::max);

    let bucket_size = ((max_value - min_value) / 16.0).ceil();
    let min_value = (min_value / bucket_size).floor() * bucket_size;
    let max_value = (max_value / bucket_size).ceil() * bucket_size;

    let num_buckets = ((max_value - min_value) / bucket_size).round() as usize;

    let mut buckets = vec![0; num_buckets];

    for &value in data {
        let index = min(
            ((value - min_value) / bucket_size).floor() as usize,
            num_buckets - 1,
        );

        if let Some(count) = buckets.get_mut(index) {
            *count += 1;
        }
    }

    let max_length = min(70, data.len());

    for (i, &count) in buckets.iter().enumerate() {
        let lower_bound = (i as f64).mul_add(bucket_size, min_value);
        let upper_bound = lower_bound + bucket_size;

        #[allow(clippy::integer_division)]
        let bar = "*".repeat(max_length * count / data.len());

        println!("{lower_bound:3} - {upper_bound:3} {count:5} {bar}");
    }

    Ok(())
}

#[allow(clippy::print_stdout)]
pub fn print_stats(stats: &mut Vec<(String, Vec<f64>)>) -> anyhow::Result<()> {
    #[allow(clippy::str_to_string)]
    let mut table = table!([
        "",
        "Minimum",
        "-3\u{3c3}",
        "-2\u{3c3}",
        "-1\u{3c3}",
        "Median",
        "1\u{3c3}",
        "2\u{3c3}",
        "3\u{3c3}",
        "Maximum",
        "Mean",
        "Std Dev"
    ]);

    table.set_format(*prettytable::format::consts::FORMAT_BOX_CHARS);

    for (name, ref mut data) in stats {
        let mut data = Data::new(data);

        #[allow(clippy::string_to_string)]
        table.add_row(row![
            format!("{name:12}"),
            format!("{:8.3}", data.min()),
            format!("{:8.3}", data.quantile(0.001_349_898)),
            format!("{:8.3}", data.quantile(0.022_750_132)),
            format!("{:8.3}", data.quantile(0.158_655_254)),
            format!("{:8.3}", data.median()),
            format!("{:8.3}", data.quantile(0.841_344_746)),
            format!("{:8.3}", data.quantile(0.977_249_868)),
            format!("{:8.3}", data.quantile(0.998_650_102)),
            format!("{:8.3}", data.max()),
            format!(
                "{:8.3}",
                data.mean()
                    .with_context(|| format!("Unable to calculate mean for {name}"))?
            ),
            format!(
                "{:8.3}",
                data.std_dev().with_context(|| format!(
                    "Unable to calculate standard deviation for {name}"
                ))?
            ),
        ]);
    }

    table.printstd();

    Ok(())
}

pub fn verify_filename(path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Unable to create directory {parent:?}"))?;
    }

    Ok(())
}

pub fn verify_directory(path: &Path) -> anyhow::Result<()> {
    if path.exists() {
        if !path.is_dir() {
            return Err(anyhow!("{path:?} exists but is not a directory"));
        }
    } else {
        std::fs::create_dir_all(path)
            .with_context(|| format!("Unable to create directory {path:?}"))?;
    }

    Ok(())
}

pub struct HumanBitrate(pub f64);

impl std::fmt::Display for HumanBitrate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match NumberPrefix::decimal(self.0) {
            NumberPrefix::Standalone(number) => write!(f, "{number:.0} bps"),
            NumberPrefix::Prefixed(prefix, number) => write!(f, "{number:.3} {prefix}bps"),
        }
    }
}
