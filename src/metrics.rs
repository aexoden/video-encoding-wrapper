use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};

use anyhow::{anyhow, Context};
use indicatif::{HumanCount, ProgressBar};
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::util::{create_progress_style, verify_filename, HumanBitrate};

#[allow(clippy::module_name_repetitions)]
#[derive(Serialize, Deserialize)]
pub struct ClipMetrics {
    #[serde(skip)]
    path: PathBuf,

    #[serde(skip)]
    json_path: PathBuf,

    // Single Values
    duration: Option<f64>,

    // Frame Values
    sizes: Option<Vec<usize>>,
}

impl ClipMetrics {
    pub fn new(path: &Path) -> anyhow::Result<Self> {
        let json_path = path.with_extension("metrics.json");
        verify_filename(&json_path)
            .with_context(|| format!("Unable to verify clip metrics cache path {json_path:?}"))?;

        if json_path.exists() {
            let file = File::open(&json_path)
                .with_context(|| format!("Unable to open clip metrics cache {json_path:?}"))?;
            let reader = BufReader::new(file);
            let mut metrics: Self = serde_json::from_reader(reader)
                .context("Unable to deserialize clip metrics cache")?;

            metrics.path = path.to_path_buf();
            metrics.json_path = json_path;

            Ok(metrics)
        } else {
            Ok(Self {
                path: path.to_path_buf(),
                json_path,
                sizes: None,
                duration: None,
            })
        }
    }

    #[must_use]
    pub const fn path(&self) -> &PathBuf {
        &self.path
    }

    pub fn sizes(&mut self) -> anyhow::Result<&Vec<usize>> {
        if self.sizes.is_none() {
            self.calculate_duration_and_size().with_context(|| {
                format!("Unable to calculate duration or size for {:?}", &self.path)
            })?;
        }

        self.sizes
            .as_ref()
            .ok_or_else(|| anyhow!("Unreachable code reached"))
    }

    pub fn duration(&mut self) -> anyhow::Result<f64> {
        if self.duration.is_none() {
            self.calculate_duration_and_size().with_context(|| {
                format!("Unable to calculate duration or size for {:?}", &self.path)
            })?;
        }

        self.duration
            .ok_or_else(|| anyhow!("Unreachable code reached"))
    }

    pub fn frames(&mut self) -> anyhow::Result<usize> {
        if self.sizes.is_none() {
            self.calculate_duration_and_size().with_context(|| {
                format!("Unable to calculate duration or size for {:?}", &self.path)
            })?;
        }

        Ok(self
            .sizes
            .as_ref()
            .ok_or_else(|| anyhow!("Unreachable code reached"))?
            .len())
    }

    #[allow(clippy::as_conversions)]
    #[allow(clippy::cast_precision_loss)]
    fn calculate_duration_and_size(&mut self) -> anyhow::Result<()> {
        let (stream_index, avg_frame_rate, mut input_context) = {
            let input_context = ffmpeg::format::input(&self.path)
                .with_context(|| format!("Unable to open {:?} with FFmpeg", &self.path))?;

            let input = input_context
                .streams()
                .best(ffmpeg::media::Type::Video)
                .ok_or(ffmpeg::Error::StreamNotFound)
                .with_context(|| format!("Unable to find video stream in {:?}", self.path))?;

            (input.index(), input.avg_frame_rate(), input_context)
        };

        let mut packet_sizes = vec![];

        for (_, packet) in input_context
            .packets()
            .filter(|(stream, _)| stream.index() == stream_index)
        {
            packet_sizes.push(packet.size());
        }

        self.duration = Some(packet_sizes.len() as f64 / f64::from(avg_frame_rate));
        self.sizes = Some(packet_sizes);

        self.update_cache()
            .with_context(|| format!("Unable to update metrics cache for {:?}", &self.path))?;

        Ok(())
    }

    fn update_cache(&self) -> anyhow::Result<()> {
        let temporary_path = self.json_path.with_extension(".tmp.json");

        serde_json::to_writer_pretty(
            &File::create(&temporary_path).with_context(|| {
                format!(
                    "Unable to create clip metrics cache file {:?}",
                    &temporary_path
                )
            })?,
            &self,
        )
        .with_context(|| {
            format!(
                "Unable to serialize clip metrics cache to {:?}",
                &temporary_path
            )
        })?;

        std::fs::rename(&temporary_path, &self.json_path).with_context(|| {
            format!(
                "Unable to rename {temporary_path:?} to {:?}",
                self.json_path
            )
        })?;

        Ok(())
    }
}

#[allow(clippy::as_conversions)]
#[allow(clippy::cast_precision_loss)]
#[allow(clippy::integer_division)]
#[allow(clippy::print_stdout)]
pub fn print(config: &Config, clips: &mut [ClipMetrics]) -> anyhow::Result<()> {
    let metadata = crate::ffmpeg::get_metadata(config)
        .with_context(|| format!("Unable to fetch video metadata for {:?}", &config.source))?;

    let progress_bar = ProgressBar::new(metadata.frame_count.try_into().unwrap_or(u64::MAX));

    progress_bar.set_style(
        create_progress_style(
            "{spinner:.green} [{elapsed_precise}] Collecting metrics...      [{wide_bar:.cyan/blue}] {percent:>3}% {human_pos:>8}/{human_len:>8} ({smooth_per_sec:>6} FPS, ETA: {smooth_eta:>3})"
        ).context("Unable to create metrics progress bar style")?
    );

    let mut sizes: Vec<usize> = vec![];
    let mut duration = 0.0_f64;

    for metrics in clips.iter_mut() {
        duration += metrics
            .duration()
            .context("Unable to access clip duration")?;

        let clip_sizes = metrics.sizes().context("Unable to access clip size")?;
        sizes.extend(clip_sizes);

        progress_bar.inc(clip_sizes.len().try_into().unwrap_or(u64::MAX));
    }

    progress_bar.finish();

    println!();
    println!();

    println!(
        "Frames: {}{}",
        HumanCount(sizes.len().try_into().unwrap_or(u64::MAX)),
        if sizes.len() == metadata.frame_count {
            String::new()
        } else {
            format!(
                " (expected {})",
                HumanCount(metadata.frame_count.try_into().unwrap_or(u64::MAX))
            )
        }
    );

    println!(
        "Bitrate: {}",
        HumanBitrate((sizes.iter().sum::<usize>() * 8) as f64 / duration),
    );

    Ok(())
}
