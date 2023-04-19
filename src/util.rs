use std::fmt::Write;
use std::path::Path;
use std::time::Duration;

use anyhow::{anyhow, Context};
use indicatif::{HumanDuration, ProgressState, ProgressStyle};
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
