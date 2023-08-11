use std::io::{BufRead, BufReader};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{anyhow, Context};
use crossbeam_queue::ArrayQueue;
use indicatif::{MultiProgress, ProgressBar, ProgressStyle};
use statrs::statistics::{Data, OrderStatistics};
use tracing::error;

use crate::config::{Config, Metric, QualityRule};
use crate::ffmpeg::{create_child_read, Metadata};
use crate::metrics::ClipMetrics;
use crate::scenes::Scene;
use crate::util::{create_progress_style, verify_directory, HumanBitrate};

fn update_worker_message(progress_bar: &ProgressBar, scene_index: usize, message: &str) {
    progress_bar.set_message(format!("[Scene {scene_index:05}] {message}"));
}

fn clear_worker_message(progress_bar: &ProgressBar) {
    progress_bar.set_message("[Idle       ]");
}

pub struct Encoder {
    config: Config,
    scenes: Vec<Scene>,
    metadata: Metadata,
    encode_directory: PathBuf,
}

impl Encoder {
    pub fn new(config: &Config) -> anyhow::Result<Self> {
        let mut scenes = crate::scenes::get(config).context("Unable to fetch scene data")?;
        scenes.sort_by_key(|a| std::cmp::Reverse(a.length()));

        let encode_directory = config
            .output_directory
            .join("encode")
            .join(config.encode_identifier(false));

        Ok(Self {
            config: config.clone(),
            scenes,
            metadata: crate::ffmpeg::get_metadata(config).with_context(|| {
                format!("Unable to fetch video metadata for {:?}", &config.source)
            })?,
            encode_directory,
        })
    }

    #[must_use]
    pub const fn passes(&self) -> usize {
        match self.config.encoder {
            crate::config::Encoder::Aomenc => 2,
            crate::config::Encoder::X264 | crate::config::Encoder::X265 => 1,
        }
    }

    #[allow(clippy::print_stdout)]
    #[allow(clippy::too_many_lines)]
    pub fn encode(&self) -> anyhow::Result<(PathBuf, Vec<ClipMetrics>)> {
        let scene_queue: ArrayQueue<Scene> = ArrayQueue::new(self.scenes.len());
        let result_queue: ArrayQueue<ClipMetrics> = ArrayQueue::new(self.scenes.len());

        for scene in &self.scenes {
            if scene_queue.push(*scene).is_err() {
                return Err(anyhow!("Encoding worker queue was unexpectedly full"));
            }
        }

        let multi_progress = MultiProgress::new();

        let worker_progress_style = ProgressStyle::with_template("{msg}")
            .context("Unable to create worker progress style")?;

        let worker_progress_bars = (0..rayon::current_num_threads())
            .map(|_thread_index| {
                multi_progress.add(ProgressBar::new(1).with_style(worker_progress_style.clone()))
            })
            .collect::<Vec<_>>();

        let progress_bar =
            ProgressBar::new(self.metadata.frame_count.try_into().unwrap_or(u64::MAX));

        progress_bar.set_style(
            create_progress_style(
                "{spinner:.green} [{elapsed_precise}] Encoding scenes...         [{wide_bar:.cyan/blue}] {percent:>3}% {human_pos:>8}/{human_len:>8} ({smooth_per_sec:>6} FPS, {msg:>12}, ETA: {smooth_eta:>3})"
            ).context("Unable to create encoding progress bar style")?
        );

        let progress_bar = multi_progress.add(progress_bar);
        progress_bar.reset();
        progress_bar.enable_steady_tick(std::time::Duration::from_secs(1));

        let mut clips: Vec<ClipMetrics> = vec![];

        std::thread::scope(|scope| -> anyhow::Result<()> {
            let threads = (0..self.config.workers)
                .map(|thread_index| -> anyhow::Result<_> {
                    let worker_progress_bar = worker_progress_bars
                        .get(thread_index)
                        .ok_or_else(|| anyhow!("Unable to access encoding worker progress bar"))?;

                    Ok(scope.spawn(|| -> anyhow::Result<()> {
                        while let Some(scene) = &scene_queue.pop() {
                            let result =
                                self.encode_scene(scene, worker_progress_bar).with_context(
                                    || format!("Unable to encode scene {}", scene.index()),
                                )?;

                            let input_filename = self
                                .config
                                .output_directory
                                .join("source")
                                .join(format!("scene-{:05}.mkv", scene.index()));

                            let metrics = ClipMetrics::new(&result, &input_filename, None)
                                .with_context(|| {
                                    format!(
                                        "Unable to calculate metrics for scene {}",
                                        scene.index()
                                    )
                                })?;

                            if result_queue.push(metrics).is_err() {
                                return Err(anyhow!("Encoding result queue was unexpectedly full"));
                            }

                            clear_worker_message(worker_progress_bar);
                        }

                        worker_progress_bar.finish();

                        Ok(())
                    }))
                })
                .collect::<Result<Vec<_>, _>>()
                .context("Unable to spawn encoding workers")?;

            let mut current_bytes = 0;
            let mut current_duration = 0.0_f64;

            while threads
                .iter()
                .any(|thread| -> bool { !thread.is_finished() })
                || !result_queue.is_empty()
            {
                while let Some(mut clip) = result_queue.pop() {
                    current_bytes += clip
                        .sizes()
                        .context("Unable to read clip size")?
                        .iter()
                        .sum::<usize>();

                    current_duration += clip.duration().context("Unable to read clip duration")?;

                    #[allow(clippy::as_conversions)]
                    #[allow(clippy::cast_precision_loss)]
                    progress_bar.set_message(format!(
                        "{}",
                        HumanBitrate(current_bytes as f64 * 8.0 / current_duration)
                    ));

                    progress_bar.inc(
                        (clip.frames().context("Unable to read clip frame count")?)
                            .try_into()
                            .unwrap_or(u64::MAX),
                    );

                    clips.push(clip);
                }
            }

            for thread in threads {
                let result = thread.join();

                match result {
                    Ok(result) => {
                        result.context("Unable to encode scene")?;
                    }
                    Err(error) => {
                        return Err(anyhow!("Encoding worker panicked: {:?}", error));
                    }
                }
            }

            progress_bar.finish();

            if !result_queue.is_empty() {
                return Err(anyhow!(
                    "BUG: Result queue was not empty after joining encoding threads"
                ));
            }

            Ok(())
        })
        .context("Unable to execute encoding workers")?;

        clips.sort_by(|a, b| a.path().cmp(b.path()));

        let output_path = self
            .merge_scenes(&clips)
            .context("Unable to merge scenes")?;

        Ok((output_path, clips))
    }

    fn merge_scenes(&self, files: &[ClipMetrics]) -> anyhow::Result<PathBuf> {
        let output_path = self.config.output_directory.join("output");

        verify_directory(&output_path).with_context(|| {
            format!("Unable to verify merging output directory {output_path:?}")
        })?;

        let temporary_output_path =
            output_path.join(format!("{}.tmp.mkv", self.config.encode_identifier(true)));

        let output_path = output_path.join(format!("{}.mkv", self.config.encode_identifier(true)));

        let progress_bar = ProgressBar::new_spinner();
        progress_bar.enable_steady_tick(std::time::Duration::from_millis(120));
        progress_bar.set_style(
            create_progress_style("{spinner:.green} [{elapsed_precise}] {msg}")
                .context("Unable to create scene merging progress bar style")?,
        );
        progress_bar.set_message("Merging scenes...");

        if !output_path.exists() {
            let file_args = files
                .iter()
                .enumerate()
                .map(|(index, metrics)| {
                    if index > 0 {
                        format!("+{}", metrics.path().to_string_lossy())
                    } else {
                        metrics.path().to_string_lossy().to_string()
                    }
                })
                .collect::<Vec<_>>();

            let merge_pipe = Command::new("mkvmerge")
                .arg("-o")
                .arg(&temporary_output_path)
                .args(file_args)
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .spawn()
                .context("Unable to spawn mkvmerge")?;

            let result = merge_pipe
                .wait_with_output()
                .context("Unable to wait for mkvmerge to finish")?;

            if !result.status.success() {
                progress_bar.set_message("Merging scenes...failed!");
                progress_bar.finish();

                return Err(anyhow!(
                    "mkvmerge returned error code {} and the following output:\n{}\n{}",
                    result.status,
                    std::str::from_utf8(&result.stdout)
                        .context("Unable to parse mkvmerge output as UTF-8")?,
                    std::str::from_utf8(&result.stderr)
                        .context("Unable to parse mkvmerge output as UTF-8")?
                ));
            }
        }

        if temporary_output_path.exists() {
            std::fs::rename(&temporary_output_path, &output_path).with_context(|| {
                format!("Unable to rename {temporary_output_path:?} to {output_path:?}")
            })?;
        }

        progress_bar.set_message("Merging scenes...done!");
        progress_bar.finish();

        Ok(output_path)
    }

    #[allow(clippy::as_conversions)]
    #[allow(clippy::cast_precision_loss)]
    #[allow(clippy::too_many_lines)]
    fn encode_scene(&self, scene: &Scene, progress_bar: &ProgressBar) -> anyhow::Result<PathBuf> {
        let quality = if self.config.metric == Metric::Direct {
            self.config.quality
        } else {
            let mut quality_range = self.config.encoder.quality_range(&self.config.mode);

            let mut best_quality = if self.config.rule == QualityRule::Maximum {
                quality_range.maximum()
            } else {
                quality_range.minimum()
            };

            let mut best_score = f64::MIN;

            while !quality_range.complete() {
                let true_minimum = quality_range.minimum().min(best_quality);
                let true_maximum = quality_range.maximum().max(best_quality);

                let search_description = if quality_range.divisor() == 1_i32 {
                    format!(
                        "Quality Search {true_minimum:4} - {true_maximum:4} ({:4}): ",
                        quality_range.current()
                    )
                } else {
                    format!(
                        "Quality Search {true_minimum:7.2} - {true_maximum:0.2} ({:7.2}): ",
                        quality_range.current()
                    )
                };

                let input_filename = self
                    .config
                    .output_directory
                    .join("source")
                    .join(format!("scene-{:05}.mkv", scene.index()));

                let current_quality = quality_range.current();

                let output_filename = self
                    .encode_scene_single(
                        scene,
                        progress_bar,
                        &search_description,
                        self.passes(),
                        current_quality,
                    )
                    .context("Unable to encode scene")?;

                update_worker_message(
                    progress_bar,
                    scene.index(),
                    &format!("{search_description}Calculating metric..."),
                );

                let mut metrics = ClipMetrics::new(&output_filename, &input_filename, None)
                    .with_context(|| {
                        format!("Unable to calculate metrics for scene {:05}", scene.index())
                    })?;

                let metric_values = match self.config.metric {
                    Metric::Direct => vec![0.0_f64],
                    Metric::PSNR => metrics
                        .psnr(1)
                        .context("Unable to calculate PSNR values")?
                        .clone(),
                    Metric::SSIM => metrics
                        .ssim(1)
                        .context("Unable to calculate SSIM values")?
                        .clone(),
                    Metric::VMAF => metrics
                        .vmaf(1)
                        .context("Unable to calculate VMAF values")?
                        .clone(),
                    Metric::SSIMULACRA2 => metrics
                        .ssimulacra2(1)
                        .context("Unable to calculate SSIMULACRA2 values")?
                        .clone(),
                    Metric::Bitrate => {
                        let duration =
                            metrics.duration().context("Unable to calculate duration")?;

                        let frames = metrics
                            .frames()
                            .context("Unable to determine frame count")?;

                        let frame_duration = duration / frames as f64;

                        metrics
                            .sizes()
                            .context("Unable to calculate frame sizes")?
                            .iter()
                            .map(|x| *x as f64 * 8.0_f64 / frame_duration)
                            .collect()
                    }
                };

                let metric_value = Data::new(metric_values).quantile(self.config.percentile);

                match self.config.rule {
                    QualityRule::Maximum => {
                        if metric_value <= self.config.quality {
                            if current_quality < best_quality {
                                best_quality = current_quality;
                                best_score = metric_value;
                            }

                            quality_range.lower();
                        } else {
                            quality_range.higher();
                        }
                    }
                    QualityRule::Minimum => {
                        if metric_value >= self.config.quality {
                            if current_quality > best_quality {
                                best_quality = current_quality;
                                best_score = metric_value;
                            }

                            quality_range.higher();
                        } else {
                            quality_range.lower();
                        }
                    }
                    QualityRule::Target => {
                        let current_delta = (self.config.quality - best_score).abs();
                        let new_delta = (self.config.quality - metric_value).abs();

                        if new_delta < current_delta {
                            best_quality = current_quality;
                            best_score = metric_value;
                        }

                        if metric_value >= self.config.quality {
                            quality_range.higher();
                        } else {
                            quality_range.lower();
                        }
                    }
                }
            }

            best_quality
        };

        self.encode_scene_single(scene, progress_bar, "", self.passes(), quality)
    }

    #[allow(clippy::too_many_lines)]
    fn encode_scene_single(
        &self,
        scene: &Scene,
        progress_bar: &ProgressBar,
        progress_prefix: &str,
        passes: usize,
        qp: f64,
    ) -> anyhow::Result<PathBuf> {
        if passes > 1 {
            self.encode_scene_single(scene, progress_bar, progress_prefix, passes - 1, qp)
                .with_context(|| {
                    format!(
                        "Unable to encode pass {} of scene {}",
                        passes - 1,
                        scene.index()
                    )
                })?;
        }

        let output_path = self
            .encode_directory
            .join(format!("scene-{:05}", scene.index()));

        verify_directory(&output_path).with_context(|| {
            format!("Unable to verify encoding output directory {output_path:?}")
        })?;

        let base_output_filename = if self
            .config
            .encoder
            .quality_range(&self.config.mode)
            .divisor()
            == 1_i32
        {
            format!("{}-{qp:03}", self.config.mode)
        } else {
            format!("{}-{qp:05.2}", self.config.mode)
        };

        let temporary_output_filename = output_path.join(format!(
            "{base_output_filename}.tmp.{}",
            self.config.encoder.extension()
        ));

        let output_filename = output_path.join(format!(
            "{base_output_filename}.{}",
            self.config.encoder.extension()
        ));

        let stats_filename = output_path.join(format!("{base_output_filename}.stats.log"));

        if temporary_output_filename.exists() {
            std::fs::remove_file(&temporary_output_filename).with_context(|| {
                format!("Unable to remove temporary encoding file {temporary_output_filename:?}")
            })?;
        }

        if !output_filename.exists() {
            let input_filename = self
                .config
                .output_directory
                .join("source")
                .join(format!("scene-{:05}.mkv", scene.index()));

            let mut decoder_pipe = create_child_read(
                &input_filename,
                None,
                Stdio::null(),
                Stdio::piped(),
                Stdio::null(),
            )
            .context("Unable to spawn encoding video decoder subprocess")?;

            let decoder_stdout = decoder_pipe.stdout.take().ok_or_else(|| {
                anyhow!("Unable to access stdout for encoding video decoder subprocess")
            })?;

            #[allow(clippy::as_conversions)]
            #[allow(clippy::cast_possible_truncation)]
            #[allow(clippy::cast_precision_loss)]
            #[allow(clippy::cast_sign_loss)]
            let key_frame_interval =
                (self.metadata.frame_count as f64 * 5.0 / self.metadata.duration).round() as usize;

            let mut encoder_pipe = Command::new(self.config.encoder.to_string())
                .args(self.config.encoder.arguments(
                    &self.config.preset,
                    key_frame_interval,
                    (self.passes() > 1).then_some(passes),
                    Some(&stats_filename),
                    self.config.mode,
                    qp,
                ))
                .arg("-o")
                .arg(&temporary_output_filename)
                .arg("-")
                .stdin(decoder_stdout)
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .context("Unable to spawn video encoding subprocess")?;

            let mut encoder_stderr =
                BufReader::new(encoder_pipe.stderr.take().ok_or_else(|| {
                    anyhow!("Unable to access stderr for video encoder subprocess")
                })?);

            let mut buffer = Vec::with_capacity(256);

            while let Ok(bytes) = encoder_stderr.read_until(b'\r', &mut buffer) {
                if bytes == 0 {
                    break;
                }

                if let Ok(line) = std::str::from_utf8(&buffer) {
                    if !line.contains('\n') {
                        update_worker_message(
                            progress_bar,
                            scene.index(),
                            &format!("{progress_prefix}{line}"),
                        );
                    }
                }

                buffer.clear();
            }

            let child_error_code = encoder_pipe
                .wait()
                .context("Unable to wait for video encoder subprocess")?;

            if !child_error_code.success() {
                error!("encoder exited with status {child_error_code}");
            }

            if temporary_output_filename.exists() {
                if child_error_code.success() {
                    std::fs::rename(&temporary_output_filename, &output_filename).with_context(
                        || {
                            format!(
                            "Unable to rename {temporary_output_filename:?} to {output_filename:?}"
                        )
                        },
                    )?;
                } else {
                    std::fs::remove_file(&temporary_output_filename).with_context(|| {
                        format!("Unable to remove temporary file {temporary_output_filename:?}")
                    })?;
                }
            }
        }

        if stats_filename.exists() && passes == self.passes() {
            std::fs::remove_file(stats_filename).context("Unable to remove encoding stats file")?;
        }

        Ok(output_filename)
    }
}
