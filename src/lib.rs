pub mod config;
pub mod scenes;
pub mod util;

pub fn run(config: config::Config) -> anyhow::Result<()> {
    util::verify_directory(config.output_directory.as_path())?;

    let _frame_count = util::get_frame_count(&config);
    scenes::split_scenes(&config)?;

    Ok(())
}
