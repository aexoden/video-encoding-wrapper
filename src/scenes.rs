use std::fs::{remove_file, rename, File};
use std::io::BufReader;
use std::process::{Command, Stdio};

use anyhow::{anyhow, Context};
use av_scenechange::{detect_scene_changes, DetectionOptions, SceneDetectionSpeed};
use indicatif::ProgressBar;
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::config::Config;
use crate::ffmpeg::{create_child_read, get_metadata};
use crate::util::{create_progress_style, verify_directory, verify_filename};

#[derive(Copy, Clone, Serialize, Deserialize)]
pub struct Scene {
    index: usize,
    start_frame: usize,
    end_frame: usize,
}

impl Scene {
    #[must_use]
    pub const fn index(&self) -> usize {
        self.index
    }

    #[must_use]
    pub const fn length(&self) -> usize {
        self.end_frame - self.start_frame + 1
    }
}

pub fn get(config: &Config) -> anyhow::Result<Vec<Scene>> {
    let json_path = config.output_directory.join("config").join("scenes.json");
    verify_filename(&json_path)
        .with_context(|| format!("Unable to verify scene cache path {json_path:?}"))?;

    let metadata = get_metadata(config).context("Unable to fetch video metadata")?;

    let progress_bar = ProgressBar::new(
        metadata
            .frame_count
            .try_into()
            .context("Unable to convert video frame count to u64")?,
    );

    progress_bar.set_style(
        create_progress_style(
            "{spinner:.green} [{elapsed_precise}] Detecting scene changes... [{wide_bar:.cyan/blue}] {percent:>3}% {human_pos:>8}/{human_len:>8} ({smooth_per_sec:>6} FPS, ETA: {smooth_eta:>3})"
        ).context("Unable to create scene change detection progress bar style")?
    );

    let scenes = if json_path.exists() {
        let file = File::open(&json_path)
            .with_context(|| format!("Unable to open scene cache {json_path:?}"))?;
        let reader = BufReader::new(file);

        progress_bar.set_position(
            metadata
                .frame_count
                .try_into()
                .context("Unable to convert video frame count to u64")?,
        );
        progress_bar.reset_eta();
        progress_bar.finish();

        serde_json::from_reader(reader).context("Unable to deserialize scene cache")?
    } else {
        let mut decoder = y4m::Decoder::new(
            create_child_read(
                &config.source,
                metadata.crop_filter.as_deref(),
                Stdio::null(),
                Stdio::piped(),
                Stdio::null(),
            )
            .context("Unable to spawn video decoder subprocess")?
            .stdout
            .ok_or_else(|| anyhow!("Unable to access stdout of video decoder subprocess"))?,
        )
        .context("Unable to create scene change detection YUV4MPEG decoder")?;

        let opts = DetectionOptions {
            analysis_speed: SceneDetectionSpeed::Standard,
            detect_flashes: true,
            min_scenecut_distance: None,
            max_scenecut_distance: None,
            lookahead_distance: 5,
        };

        let progress_callback = |frames: usize, _keyframes: usize| {
            progress_bar.set_position(frames.try_into().unwrap_or(u64::MAX));
        };

        let results =
            detect_scene_changes::<_, u16>(&mut decoder, opts, None, Some(&progress_callback));

        progress_bar.finish();

        if results.frame_count != metadata.frame_count {
            warn!(
                "Source video had {} frames but {} were processed by the scene detector.",
                metadata.frame_count, results.frame_count
            );
        }

        let mut scene_changes = results.scene_changes;
        scene_changes.push(metadata.frame_count);

        let scenes: Vec<Scene> = scene_changes
            .iter()
            .zip(scene_changes.iter().skip(1))
            .enumerate()
            .map(|(index, (start_frame, next_start_frame))| Scene {
                index,
                start_frame: *start_frame,
                end_frame: next_start_frame - 1,
            })
            .collect();

        serde_json::to_writer_pretty(
            &File::create(&json_path)
                .with_context(|| format!("Unable to create scene cache file {json_path:?}"))?,
            &scenes,
        )
        .with_context(|| format!("Unable to serialize scene cache to {json_path:?}"))?;

        scenes
    };

    Ok(scenes)
}

#[allow(clippy::print_stdout)]
#[allow(clippy::too_many_lines)]
pub fn split(config: &Config) -> anyhow::Result<()> {
    let output_path = config.output_directory.join("source");
    verify_directory(&output_path).with_context(|| {
        format!("Unable to verify split scene output directory {output_path:?}")
    })?;

    let scenes = get(config).context("Unable to fetch scene data")?;
    let metadata = get_metadata(config)
        .with_context(|| format!("Unable to fetch video metadata for {:?}", &config.source))?;

    let complete = scenes.iter().all(|scene| {
        let output_filename = output_path.join(format!("scene-{:05}.mkv", scene.index));
        output_filename.exists()
    });

    let progress_bar = ProgressBar::new(metadata.frame_count.try_into().unwrap_or(u64::MAX));

    progress_bar.set_style(
        create_progress_style(
            "{spinner:.green} [{elapsed_precise}] Splitting scenes...        [{wide_bar:.cyan/blue}] {percent:>3}% {human_pos:>8}/{human_len:>8} ({smooth_per_sec:>6} FPS, ETA: {smooth_eta:>3})"
        ).context("Unable to create scene splitting progress bar style")?
    );

    if complete {
        for scene in scenes {
            progress_bar.inc(
                (scene.end_frame - scene.start_frame + 1)
                    .try_into()
                    .unwrap_or(u64::MAX),
            );
        }
    } else {
        let mut decoder = y4m::Decoder::new(
            create_child_read(
                &config.source,
                metadata.crop_filter.as_deref(),
                Stdio::null(),
                Stdio::piped(),
                Stdio::null(),
            )
            .context("Unable to spawn scene splitting video decoder subprocess")?
            .stdout
            .ok_or_else(|| {
                anyhow!("Unable to access stdout for scene splitting video decoder subprocess")
            })?,
        )
        .context("Unable to create scene splitting YUV4MPEG decoder")?;

        for scene in scenes {
            let final_output_filename = output_path.join(format!("scene-{:05}.mkv", scene.index));
            let temporary_output_filename =
                output_path.join(format!("scene-{:05}.tmp.mkv", scene.index));

            if final_output_filename.exists() {
                for _ in scene.start_frame..=scene.end_frame {
                    decoder.read_frame().context(
                        "Unable to read frame from scene splitting video decoder subprocess",
                    )?;
                    progress_bar.inc(1);
                    progress_bar.reset_eta();
                }
            } else {
                if temporary_output_filename.exists() {
                    remove_file(&temporary_output_filename).with_context(|| {
                        format!(
                        "Unable to remove preexisting temporary file {temporary_output_filename:?}"
                    )
                    })?;
                }

                let ffmpeg_pipe = Command::new("ffmpeg")
                    .args(["-i", "-", "-c:v", "ffv1", "-level", "3"])
                    .arg(&temporary_output_filename)
                    .stdin(Stdio::piped())
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .spawn()
                    .context("Unable to spawn scene splitting video encoding subprocess")?;

                let mut encoder =
                y4m::EncoderBuilder::new(
                    decoder.get_width(),
                    decoder.get_height(),
                    decoder.get_framerate(),
                )
                .with_colorspace(decoder.get_colorspace())
                .with_pixel_aspect(decoder.get_pixel_aspect())
                .write_header(ffmpeg_pipe.stdin.ok_or_else(|| {
                    anyhow!("Unable to access stdin for video encoder subprocess")
                })?)
                .context("Unable to write YUV4MPEG header to video encoder subprocess and create YUV4MPEG encoder")?;

                for _ in scene.start_frame..=scene.end_frame {
                    encoder
                        .write_frame(
                            &decoder
                                .read_frame()
                                .context("Unable to read frame from video decoder subprocess")?,
                        )
                        .context("Unable to write frame to video encoder subprocess")?;
                    progress_bar.inc(1);
                }
            }

            if temporary_output_filename.exists() {
                rename(&temporary_output_filename, &final_output_filename).with_context(
                || {
                    format!(
                        "Unable to rename {temporary_output_filename:?} to {final_output_filename:?}"
                    )
                },
            )?;
            }
        }
    }

    progress_bar.finish();

    Ok(())
}
