use tracing::level_filters::LevelFilter;
use tracing_error::ErrorLayer;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

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
