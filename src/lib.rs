use anyhow::Context;

pub mod config;
pub mod ffmpeg;
pub mod scenes;
pub mod util;

pub fn run(config: &config::Config) -> anyhow::Result<()> {
    util::verify_directory(&config.output_directory)
        .with_context(|| format!("Could not verify or create {:?}", config.output_directory))?;

    let _metadata = ffmpeg::get_metadata(config);

    scenes::split(config).context("Could not split scenes")?;

    Ok(())
}
