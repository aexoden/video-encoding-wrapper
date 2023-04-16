use std::fmt::Write;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::time::Duration;

use anyhow::anyhow;
use cached::{proc_macro::cached, SizedCache};
use indicatif::{
    HumanDuration, ProgressBar, ProgressFinish, ProgressIterator, ProgressState, ProgressStyle,
};
use tracing::level_filters::LevelFilter;
use tracing_error::ErrorLayer;
use tracing_subscriber::prelude::*;
use tracing_subscriber::EnvFilter;

extern crate ffmpeg_the_third as ffmpeg;

use crate::config::Config;

#[cached(
    result = true,
    type = "SizedCache<String, usize>",
    create = "{ SizedCache::with_size(100) }",
    convert = r#"{ format!("{}", config.source.to_string_lossy()) }"#
)]
pub fn get_frame_count(config: &Config) -> anyhow::Result<usize> {
    let json_path = config
        .output_directory
        .join("config")
        .join("frame_count.json");

    verify_filename(&json_path)?;

    let progress_bar = ProgressBar::new_spinner().with_finish(ProgressFinish::AndLeave);

    progress_bar.set_style(
        ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] Determining frame count... {human_pos} {msg}",
        )
        .unwrap(),
    );

    let frame_count = if json_path.exists() {
        let file = File::open(json_path)?;
        let reader = BufReader::new(file);

        let frame_count = serde_json::from_reader(reader)?;

        progress_bar.set_position(frame_count);
        progress_bar.finish_with_message("(cached)");

        frame_count as usize
    } else {
        let mut context = ffmpeg::format::input(&config.source)?;

        let video_stream_index = context
            .streams()
            .best(ffmpeg::media::Type::Video)
            .ok_or(ffmpeg::Error::StreamNotFound)?
            .index();

        let frame_count = context
            .packets()
            .filter(|(stream, _)| stream.index() == video_stream_index)
            .progress_with(progress_bar)
            .count();

        serde_json::to_writer_pretty(&File::create(json_path)?, &frame_count)?;

        frame_count
    };

    Ok(frame_count)
}

pub fn create_progress_style(template: &str) -> anyhow::Result<ProgressStyle> {
    let progress_style = ProgressStyle::with_template(template)?
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
        std::fs::create_dir_all(path.parent().unwrap())?;
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
        std::fs::create_dir_all(path)?;
    }

    Ok(())
}
