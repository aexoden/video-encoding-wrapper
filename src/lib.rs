use anyhow::Context;

pub mod config;
pub mod ffmpeg;
pub mod scenes;
pub mod util;

pub fn run(config: config::Config) -> anyhow::Result<()> {
    util::verify_directory(config.output_directory.as_path())
        .context("Failed to verify output directory")?;

    let _metadata = ffmpeg::get_metadata(&config);

    scenes::split_scenes(&config).context("Failed to split scenes")?;

    Ok(())
}
