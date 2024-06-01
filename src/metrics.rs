use std::borrow::ToOwned;
use std::fs;
use std::fs::File;
use std::io::BufReader;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::str;
use std::time::Duration;

use anyhow::{anyhow, Context};
use ffmpeg::{ffi, format, media, Error};
use indicatif::{HumanCount, ProgressBar};
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::ffmpeg::get_metadata;
use crate::ssimulacra2;
use crate::util::{
    create_progress_style, generate_bitrate_chart, generate_stat_chart, generate_stat_log,
    print_stats, verify_directory, verify_filename, HumanBitrate,
};

#[allow(clippy::module_name_repetitions)]
#[derive(Serialize, Deserialize)]
pub struct ClipMetrics {
    #[serde(skip)]
    path: PathBuf,

    #[serde(skip)]
    original_path: PathBuf,

    #[serde(skip)]
    json_path: PathBuf,

    #[serde(skip)]
    original_filter: Option<String>,

    // Single Values
    duration: Option<f64>,

    // Frame Values
    sizes: Option<Vec<usize>>,
    vmaf: Option<Vec<f64>>,
    psnr: Option<Vec<f64>>,
    ssim: Option<Vec<f64>>,
    ssimulacra2: Option<Vec<f64>>,
}

#[derive(Deserialize)]
struct FFmpegLogMetrics {
    psnr_y: f64,
    float_ssim: f64,
    vmaf: f64,
}

#[derive(Deserialize)]
struct FFmpegLogFrame {
    metrics: FFmpegLogMetrics,
}

#[derive(Deserialize)]
struct FFmpegLog {
    frames: Vec<FFmpegLogFrame>,
}

impl ClipMetrics {
    pub fn new(
        path: &Path,
        original_path: &Path,
        original_filter: Option<&str>,
    ) -> anyhow::Result<Self> {
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
            metrics.original_path = original_path.to_path_buf();
            metrics.json_path = json_path;
            metrics.original_filter = original_filter.map(ToOwned::to_owned);

            Ok(metrics)
        } else {
            Ok(Self {
                path: path.to_path_buf(),
                original_path: original_path.to_path_buf(),
                json_path,
                original_filter: original_filter.map(ToOwned::to_owned),
                sizes: None,
                duration: None,
                vmaf: None,
                psnr: None,
                ssim: None,
                ssimulacra2: None,
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

    pub fn psnr(&mut self, threads: usize) -> anyhow::Result<&Vec<f64>> {
        if self.psnr.is_none() {
            self.calculate_ffmpeg_metrics(threads)
                .with_context(|| format!("Unable to calculate PSNR for {:?}", &self.path))?;
        }

        self.psnr
            .as_ref()
            .ok_or_else(|| anyhow!("Unreachable code reached"))
    }

    pub fn ssim(&mut self, threads: usize) -> anyhow::Result<&Vec<f64>> {
        if self.ssim.is_none() {
            self.calculate_ffmpeg_metrics(threads)
                .with_context(|| format!("Unable to calculate SSIM for {:?}", &self.path))?;
        }

        self.ssim
            .as_ref()
            .ok_or_else(|| anyhow!("Unreachable code reached"))
    }

    pub fn vmaf(&mut self, threads: usize) -> anyhow::Result<&Vec<f64>> {
        if self.vmaf.is_none() {
            self.calculate_ffmpeg_metrics(threads)
                .with_context(|| format!("Unable to calculate VMAF for {:?}", &self.path))?;
        }

        self.vmaf
            .as_ref()
            .ok_or_else(|| anyhow!("Unreachable code reached"))
    }

    pub fn ssimulacra2(&mut self, threads: usize) -> anyhow::Result<&Vec<f64>> {
        if self.ssimulacra2.is_none() {
            self.calculate_ssimulacra2(threads)
                .with_context(|| format!("Unable to calculate SSIMULACRA2 for {:?}", &self.path))?;
        }

        self.ssimulacra2
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
        let (stream_index, duration, avg_frame_rate, mut input_context) = {
            let input_context = format::input(&self.path)
                .with_context(|| format!("Unable to open {:?} with FFmpeg", &self.path))?;

            let input = input_context
                .streams()
                .best(media::Type::Video)
                .ok_or(Error::StreamNotFound)
                .with_context(|| format!("Unable to find video stream in {:?}", self.path))?;

            (
                input.index(),
                input_context.duration(),
                if input.avg_frame_rate() > ffmpeg::Rational(0, 1) {
                    input.avg_frame_rate()
                } else {
                    input.rate()
                },
                input_context,
            )
        };

        let mut packet_sizes = vec![];

        for (_, packet) in input_context
            .packets()
            .filter(|(stream, _)| stream.index() == stream_index)
        {
            packet_sizes.push(packet.size());
        }

        if duration >= 0 {
            self.duration = Some(duration as f64 / f64::from(ffi::AV_TIME_BASE));
        } else {
            self.duration = Some(packet_sizes.len() as f64 / f64::from(avg_frame_rate));
        }

        self.sizes = Some(packet_sizes);

        self.update_cache()
            .with_context(|| format!("Unable to update metrics cache for {:?}", &self.path))?;

        Ok(())
    }

    fn calculate_ssimulacra2(&mut self, threads: usize) -> anyhow::Result<()> {
        self.ssimulacra2 = Some(
            ssimulacra2::calculate(&self.original_path, &self.path, threads)
                .context("Unable to calculate SSIMULACRA2 for clip")?,
        );

        self.update_cache()
            .with_context(|| format!("Unable to update metrics cache for {:?}", &self.path))?;

        Ok(())
    }

    fn calculate_ffmpeg_metrics(&mut self, threads: usize) -> anyhow::Result<()> {
        let log_path = self.path.with_extension("ffmpeg.metrics.json");

        let filters = [
            self.original_filter.as_ref().map_or_else(
                || "[0:v]setpts=PTS-STARTPTS[reference]".to_owned(),
                |filter| format!("[0:v]{filter},setpts=PTS-STARTPTS[reference]")
            ),
            "[1:v]setpts=PTS-STARTPTS[distorted]".to_owned(),
            format!("[distorted][reference]libvmaf=log_fmt=json:log_path={}:n_threads={threads}:feature=name=psnr|name=float_ssim", log_path.to_string_lossy())
        ];

        let child = Command::new("ffmpeg")
            .arg("-r")
            .arg("60")
            .arg("-i")
            .arg(&self.original_path)
            .arg("-r")
            .arg("60")
            .arg("-i")
            .arg(&self.path)
            .arg("-lavfi")
            .arg(filters.join(";"))
            .arg("-f")
            .arg("null")
            .arg("-")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::piped())
            .spawn()
            .context("Unable to spawn FFmpeg subprocess")?;

        let result = child
            .wait_with_output()
            .context("Unable to wait for FFmpeg subprocess")?;

        if !result.status.success() || !log_path.exists() {
            return Err(anyhow!(
                "FFmpeg metric subprocess did not complete successfully: {}",
                str::from_utf8(&result.stderr)
                    .context("Unable to decode FFmpeg error output as UTF-8")?
            ));
        }

        let log_file = File::open(&log_path)
            .with_context(|| format!("Unable to open FFmpeg metrics file {log_path:?}"))?;

        let log_reader = BufReader::new(log_file);

        let log: FFmpegLog = serde_json::from_reader(log_reader)
            .context("Unable to parse FFmpeg metrics JSON log file")?;

        let mut vmaf = vec![];
        let mut psnr = vec![];
        let mut ssim = vec![];

        for frame in log.frames {
            vmaf.push(frame.metrics.vmaf);
            psnr.push(frame.metrics.psnr_y);
            ssim.push(frame.metrics.float_ssim);
        }

        self.vmaf = Some(vmaf);
        self.psnr = Some(psnr);
        self.ssim = Some(ssim);

        fs::remove_file(&log_path).with_context(|| format!("Unable to remove {log_path:?}"))?;

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

        fs::rename(&temporary_path, &self.json_path).with_context(|| {
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
fn moving_sum(data: &[f64], window_size: usize) -> Vec<f64> {
    let mut result = Vec::new();

    for i in window_size..=data.len() {
        result.push(
            data.get(i - window_size..i)
                .unwrap_or_default()
                .iter()
                .sum::<f64>(),
        );
    }

    result
}

#[allow(clippy::as_conversions)]
#[allow(clippy::cast_possible_truncation)]
#[allow(clippy::cast_precision_loss)]
#[allow(clippy::cast_sign_loss)]
pub fn bitrate_analysis(config: &Config, clips: &mut [ClipMetrics]) -> anyhow::Result<()> {
    let metadata = get_metadata(config)
        .with_context(|| format!("Unable to fetch video metadata for {:?}", &config.source))?;

    let mut sizes: Vec<f64> = vec![];

    for clip_metrics in &mut *clips {
        sizes.extend(
            clip_metrics
                .sizes()
                .context("Unable to access clip sizes")?
                .iter()
                .map(|x| *x as f64),
        );
    }

    let avg_frame_rate = sizes.len() as f64 / metadata.duration;
    let window_sizes = [1.0_f64, 5.0_f64, 15.0_f64, 30.0_f64, 60.0_f64];

    let averages: Vec<Vec<f64>> = window_sizes
        .iter()
        .map(|&window_size| {
            moving_sum(&sizes, (window_size * avg_frame_rate).round() as usize)
                .iter()
                .map(|x| x * 8.0_f64 / window_size / 1_000_000_f64)
                .collect()
        })
        .collect();

    let output_path = config.output_directory.join("output");

    verify_directory(&output_path)
        .with_context(|| format!("Unable to verify merging output directory {output_path:?}"))?;

    let series = window_sizes
        .iter()
        .map(|x| format!("{x:.0}s"))
        .zip(&averages)
        .collect();

    generate_bitrate_chart(
        &output_path.join(format!("{}-bitrate.svg", config.encode_identifier(true))),
        "Bitrate (Mbps)",
        (1.0 * avg_frame_rate).round() as usize,
        &series,
    )
    .context("Unable to generate bitrate chart")?;

    Ok(())
}

#[allow(clippy::as_conversions)]
#[allow(clippy::cast_precision_loss)]
#[allow(clippy::integer_division)]
#[allow(clippy::print_stdout)]
#[allow(clippy::too_many_lines)]
pub fn print(config: &Config, clips: &mut [ClipMetrics]) -> anyhow::Result<()> {
    let metadata = get_metadata(config)
        .with_context(|| format!("Unable to fetch video metadata for {:?}", &config.source))?;

    let progress_bar = ProgressBar::new(metadata.frame_count.try_into().unwrap_or(u64::MAX));

    progress_bar.set_style(
        create_progress_style(
            "{spinner:.green} [{elapsed_precise}] Collecting metrics...      [{wide_bar:.cyan/blue}] {percent:>3}% {human_pos:>8}/{human_len:>8} ({smooth_per_sec:>6} FPS, ETA: {smooth_eta:>3})"
        ).context("Unable to create metrics progress bar style")?
    );

    progress_bar.enable_steady_tick(Duration::from_secs(1));

    let mut sizes: Vec<usize> = vec![];
    let mut duration = 0.0_f64;

    let mut psnr = vec![];
    let mut ssim = vec![];
    let mut vmaf = vec![];
    let mut ssimulacra2 = vec![];

    for clip_metrics in &mut *clips {
        duration += clip_metrics
            .duration()
            .context("Unable to access clip duration")?;

        let clip_sizes = clip_metrics.sizes().context("Unable to access clip size")?;
        sizes.extend(clip_sizes);

        let frame_count = clip_sizes.len().try_into().unwrap_or(u64::MAX);

        psnr.extend(
            clip_metrics
                .psnr(config.workers)
                .context("Unable to access clip PSNR")?,
        );

        ssim.extend(
            clip_metrics
                .ssim(config.workers)
                .context("Unable to access clip SSIM")?,
        );

        vmaf.extend(
            clip_metrics
                .vmaf(config.workers)
                .context("Unable to access clip VMAF")?,
        );

        ssimulacra2.extend(
            clip_metrics
                .ssimulacra2(config.workers)
                .context("Unable to access clip SSIMULACRA2")?,
        );

        progress_bar.inc(frame_count);
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

    let output_path = config.output_directory.join("output");

    verify_directory(&output_path)
        .with_context(|| format!("Unable to verify merging output directory {output_path:?}"))?;

    generate_stat_log(
        &output_path.join(format!("{}-psnr.txt", config.encode_identifier(true))),
        "PSNR",
        &psnr,
    )
    .context("Unable to generate PSNR log")?;

    generate_stat_chart(
        &output_path.join(format!("{}-psnr.svg", config.encode_identifier(true))),
        "PSNR",
        &psnr,
    )
    .context("Unable to generate PSNR chart")?;

    generate_stat_log(
        &output_path.join(format!("{}-ssim.txt", config.encode_identifier(true))),
        "SSIM",
        &ssim,
    )
    .context("Unable to generate SSIM log")?;

    generate_stat_chart(
        &output_path.join(format!("{}-ssim.svg", config.encode_identifier(true))),
        "SSIM",
        &ssim,
    )
    .context("Unable to generate SSIM chart")?;

    generate_stat_log(
        &output_path.join(format!("{}-vmaf.txt", config.encode_identifier(true))),
        "VMAF",
        &vmaf,
    )
    .context("Unable to generate VMAF log")?;

    generate_stat_chart(
        &output_path.join(format!("{}-vmaf.svg", config.encode_identifier(true))),
        "VMAF",
        &vmaf,
    )
    .context("Unable to generate VMAF chart")?;

    generate_stat_log(
        &output_path.join(format!(
            "{}-ssimulacra2.txt",
            config.encode_identifier(true)
        )),
        "SSIMULACRA2",
        &ssimulacra2,
    )
    .context("Unable to generate SSIMULACRA2 log")?;

    generate_stat_chart(
        &output_path.join(format!(
            "{}-ssimulacra2.svg",
            config.encode_identifier(true)
        )),
        "SSIMULACRA2",
        &ssimulacra2,
    )
    .context("Unable to generate SSIMULACRA2 chart")?;

    println!();

    let mut metrics = vec![
        ("PSNR".to_owned(), psnr),
        ("SSIM".to_owned(), ssim),
        ("VMAF".to_owned(), vmaf),
        ("SSIMULACRA2".to_owned(), ssimulacra2),
    ];

    print_stats(&mut metrics).context("Unable to output metrics")?;

    Ok(())
}
