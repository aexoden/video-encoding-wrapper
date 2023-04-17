use std::fmt::Write;
use std::path::Path;
use std::time::Duration;

use anyhow::{anyhow, Context};
use indicatif::{HumanDuration, ProgressState, ProgressStyle};
use tracing::level_filters::LevelFilter;
use tracing_error::ErrorLayer;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

pub fn create_progress_style(template: &str) -> anyhow::Result<ProgressStyle> {
    let progress_style = ProgressStyle::with_template(template)
        .context("Failed to create progress style with template")?
        .with_key(
            "smooth_eta",
            |s: &ProgressState, w: &mut dyn Write| match (s.pos(), s.len()) {
                (0, _) => write!(w, "-").unwrap_or_default(),
                (pos, Some(len)) => write!(
                    w,
                    "{:#}",
                    HumanDuration(Duration::from_millis(
                        (s.elapsed().as_millis() * (u128::from(len) - u128::from(pos))
                            / u128::from(pos))
                        .try_into()
                        .unwrap_or(u64::MAX)
                    ))
                )
                .unwrap_or_default(),
                _ => write!(w, "-").unwrap_or_default(),
            },
        )
        .with_key(
            "smooth_per_sec",
            |s: &ProgressState, w: &mut dyn Write| match (s.pos(), s.elapsed().as_millis()) {
                (pos, elapsed_ms) if elapsed_ms > 0 => {
                    #[allow(clippy::cast_precision_loss)]
                    write!(w, "{:.2}", pos as f64 * 1000.0 / elapsed_ms as f64).unwrap_or_default();
                }
                _ => write!(w, "-").unwrap_or_default(),
            },
        );

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
        .context("Could not set global default tracing subscriber")?;

    Ok(())
}

pub fn verify_filename(path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("Could not create path {parent:?}"))?;
    }

    Ok(())
}

pub fn verify_directory(path: &Path) -> anyhow::Result<()> {
    if path.exists() {
        if !path.is_dir() {
            return Err(anyhow!("{path:?} exists but is not a directory"));
        }
    } else {
        std::fs::create_dir_all(path).with_context(|| format!("Could not create path {path:?}"))?;
    }

    Ok(())
}
