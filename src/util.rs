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
                (0, _) => write!(w, "-").unwrap(),
                (pos, Some(len)) => write!(
                    w,
                    "{:#}",
                    HumanDuration(Duration::from_millis(
                        (s.elapsed().as_millis() * (len as u128 - pos as u128) / (pos as u128))
                            as u64
                    ))
                )
                .unwrap(),
                _ => write!(w, "-").unwrap(),
            },
        )
        .with_key(
            "smooth_per_sec",
            |s: &ProgressState, w: &mut dyn Write| match (s.pos(), s.elapsed().as_millis()) {
                (pos, elapsed_ms) if elapsed_ms > 0 => {
                    write!(w, "{:.2}", pos as f64 * 1000.0 / elapsed_ms as f64).unwrap()
                }
                _ => write!(w, "-").unwrap(),
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
        .try_init()?;

    Ok(())
}

pub fn verify_filename(path: &Path) -> anyhow::Result<()> {
    if !path.parent().unwrap().exists() {
        std::fs::create_dir_all(path.parent().unwrap())
            .with_context(|| format!("Failed to create path {}", path.display()))?;
    }

    Ok(())
}

pub fn verify_directory(path: &Path) -> anyhow::Result<()> {
    if path.exists() {
        if !path.is_dir() {
            return Err(anyhow!(
                "{} exists but is not a directory",
                path.to_string_lossy()
            ));
        }
    } else {
        std::fs::create_dir_all(path)
            .with_context(|| format!("Failed to create path {}", path.display()))?;
    }

    Ok(())
}
