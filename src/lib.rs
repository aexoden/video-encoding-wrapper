pub mod config;
pub mod util;

pub fn run(config: config::Config) -> anyhow::Result<()> {
    util::verify_directory(config.output_directory.as_path())?;

    Ok(())
}
