use anyhow::Context;

pub mod config;
pub mod encoder;
pub mod ffmpeg;
pub mod metrics;
pub mod scenes;
pub mod ssimulacra2;
pub mod util;

#[allow(clippy::print_stdout)]
pub fn run(config: &config::Config) -> anyhow::Result<()> {
    // Prevent dependent libraries from modifying the rayon global pool with arbitrary thread counts.
    rayon::ThreadPoolBuilder::new()
        .num_threads(config.workers)
        .build_global()
        .context("Unable to initialize thread pool")?;

    util::verify_directory(&config.output_directory).with_context(|| {
        format!(
            "Unable to verify or create output directory {:?}",
            &config.output_directory
        )
    })?;

    let _metadata = ffmpeg::get_metadata(config);

    scenes::split(config)
        .with_context(|| format!("Unable to split scenes for file {:?}", &config.source))?;

    let encoder = encoder::Encoder::new(config).context("Unable to create scene encoder")?;
    let (_output_path, mut clips, statistics) =
        encoder.encode().context("Unable to encode video")?;

    metrics::print(config, &mut clips).context("Unable to print metrics")?;

    println!();

    statistics
        .print_quality_stats()
        .context("Unable to print encode quality statistics")?;

    metrics::bitrate_analysis(config, &mut clips).context("Unable to complete bitrate analysis")?;

    Ok(())
}
