use std::path::Path;

use anyhow::anyhow;
use thiserror::Error;
use tracing::level_filters::LevelFilter;
use tracing_error::ErrorLayer;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

#[derive(Error, Debug)]
pub enum DirectoryError {
    #[error("{0} exists but is not a directory")]
    ExistsNotDirectory(String),
}

pub fn install_tracing() -> anyhow::Result<()> {
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

pub fn verify_directory(path: &Path) -> anyhow::Result<()> {
    if path.exists() {
        if !path.is_dir() {
            return Err(anyhow!(
                "{} exists but is not a directory",
                path.to_string_lossy()
            ));
        }
    } else {
        std::fs::create_dir_all(path)?;
    }

    Ok(())
}
