use std::fs::File;
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

#[derive(Serialize, Deserialize)]
pub struct Scene {
    start_frame: usize,
    end_frame: usize,
}

fn get_scenes(config: &Config) -> anyhow::Result<Vec<Scene>> {
    let json_path = config.output_directory.join("config").join("scenes.json");
    verify_filename(&json_path).context("Could not verify scene JSON cache file")?;

    let metadata = get_metadata(config).context("Could not get video metadata")?;

    let progress_bar = ProgressBar::new(metadata.frame_count as u64);

    progress_bar.set_style(
        create_progress_style(
            "{spinner:.green} [{elapsed_precise}] Detecting scene changes... [{wide_bar:.cyan/blue}] {percent:>3}% {human_pos:>8}/{human_len:>8} ({smooth_per_sec:>6} FPS, ETA: {smooth_eta:>3})"
        ).context("Could not create progress style")?
    );

    let scenes = if json_path.exists() {
        let file =
            File::open(&json_path).with_context(|| format!("Could not open {json_path:?}"))?;
        let reader = BufReader::new(file);

        progress_bar.set_position(metadata.frame_count as u64);
        progress_bar.reset_eta();
        progress_bar.finish();

        serde_json::from_reader(reader).context("Could not read scene JSON cache")?
    } else {
        let mut decoder = y4m::Decoder::new(
            create_child_read(
                &config.source,
                metadata.crop_filter.as_deref(),
                Stdio::null(),
                Stdio::piped(),
                Stdio::null(),
            )
            .context("Could not spawn FFmpeg decoder subprocess")?
            .stdout
            .ok_or(anyhow!("FFmpeg decoder process unexpectedly had no stdout"))
            .context("Could not access FFmpeg decoder process stdout")?,
        )
        .context("Could not create YUV4MPEG decoder")?;

        let opts = DetectionOptions {
            analysis_speed: SceneDetectionSpeed::Standard,
            detect_flashes: true,
            min_scenecut_distance: None,
            max_scenecut_distance: None,
            lookahead_distance: 5,
        };

        let progress_callback = |frames: usize, _keyframes: usize| {
            progress_bar.set_position(frames as u64);
        };

        let results =
            detect_scene_changes::<_, u16>(&mut decoder, opts, None, Some(&progress_callback));

        progress_bar.finish();

        if results.frame_count != metadata.frame_count {
            warn!(
                "Source video had {} frames but only {} were processed by the scene detector.",
                metadata.frame_count, results.frame_count
            );
        }

        let mut scenes = vec![];

        (0..results.scene_changes.len()).for_each(|i| {
            scenes.push(Scene {
                start_frame: results.scene_changes[i],
                end_frame: if i == results.scene_changes.len() - 1 {
                    metadata.frame_count - 1
                } else {
                    results.scene_changes[i + 1] - 1
                },
            });
        });

        serde_json::to_writer_pretty(
            &File::create(&json_path).with_context(|| format!("Could not create {json_path:?}"))?,
            &scenes,
        )
        .context("Could not write scene JSON cache")?;

        scenes
    };

    Ok(scenes)
}

pub fn split(config: &Config) -> anyhow::Result<()> {
    let output_path = config.output_directory.join("source");
    verify_directory(&output_path)
        .with_context(|| format!("Could not verify or create output path {output_path:?}"))?;

    let scenes = get_scenes(config).context("Could not get scene information")?;

    let metadata = get_metadata(config).context("Could not get video metadata")?;

    let progress_bar = ProgressBar::new(metadata.frame_count as u64);

    progress_bar.set_style(
        create_progress_style(
            "{spinner:.green} [{elapsed_precise}] Splitting scenes...        [{wide_bar:.cyan/blue}] {percent:>3}% {human_pos:>8}/{human_len:>8} ({smooth_per_sec:>6} FPS, ETA: {smooth_eta:>3})"
        ).context("Could not create progress bar style")?
    );

    let mut decoder = y4m::Decoder::new(
        create_child_read(
            &config.source,
            metadata.crop_filter.as_deref(),
            Stdio::null(),
            Stdio::piped(),
            Stdio::null(),
        )
        .context("Could not spawn FFmpeg decoder process")?
        .stdout
        .ok_or(anyhow!("FFmpeg decoder process unexpectedly had no stdout"))
        .context("Could not access FFmpeg decoder process stdout")?,
    )
    .context("Failed to create YUV4MPEG decoder")?;

    for (scene_index, scene) in scenes.iter().enumerate() {
        let final_output_filename = output_path.join(format!("scene-{scene_index:05}.mkv"));
        let temporary_output_filename = output_path.join(format!("scene-{scene_index:05}.tmp.mkv"));

        if final_output_filename.exists() {
            for _ in scene.start_frame..=scene.end_frame {
                decoder
                    .read_frame()
                    .context("Could not read frame from FFmpeg decoder process")?;
                progress_bar.inc(1);
                progress_bar.reset_eta();
            }
        } else {
            if temporary_output_filename.exists() {
                std::fs::remove_file(&temporary_output_filename).with_context(|| {
                    format!(
                        "Could not remove preexisting temporary file {temporary_output_filename:?}"
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
                .context("Could not spawn FFmpeg FFV1 encoder process")?;

            let mut encoder = y4m::EncoderBuilder::new(
                decoder.get_width(),
                decoder.get_height(),
                decoder.get_framerate(),
            )
            .with_colorspace(decoder.get_colorspace())
            .with_pixel_aspect(decoder.get_pixel_aspect())
            .write_header(
                ffmpeg_pipe
                    .stdin
                    .ok_or(anyhow!("FFmpeg encoder process unexpectedly has no stdin"))
                    .context("Could not access FFmpeg encoder process stdin")?,
            )?;

            for _ in scene.start_frame..=scene.end_frame {
                encoder
                    .write_frame(
                        &decoder
                            .read_frame()
                            .context("Could not read frame from FFmpeg decoder process")?,
                    )
                    .context("Could not write frame to FFmpeg FFV1 encoder process")?;
                progress_bar.inc(1);
            }
        }

        if temporary_output_filename.exists() {
            std::fs::rename(&temporary_output_filename, &final_output_filename).with_context(
                || {
                    format!(
                        "Could not rename {temporary_output_filename:?} to {final_output_filename:?}"
                    )
                },
            )?;
        }
    }

    progress_bar.finish();

    Ok(())
}
