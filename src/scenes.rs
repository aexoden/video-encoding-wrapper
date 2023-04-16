use std::fs::File;
use std::io::{BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use av_scenechange::{detect_scene_changes, DetectionOptions, SceneDetectionSpeed};
use indicatif::ProgressBar;
use serde::{Deserialize, Serialize};
use tracing::warn;

use crate::config::Config;
use crate::util::{create_progress_style, get_frame_count, verify_directory, verify_filename};

#[derive(Serialize, Deserialize)]
pub struct Scene {
    start_frame: usize,
    end_frame: usize,
}

pub fn create_decoder(source: &PathBuf) -> anyhow::Result<y4m::Decoder<impl Read>> {
    let ffmpeg_pipe = Command::new("ffmpeg")
        .arg("-i")
        .arg(source)
        .args([
            "-pix_fmt",
            "yuv420p10le",
            "-f",
            "yuv4mpegpipe",
            "-strict",
            "-1",
            "-",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()?
        .stdout
        .unwrap();

    let decoder = y4m::Decoder::new(ffmpeg_pipe)?;

    Ok(decoder)
}

pub fn create_encoder(
    output: &PathBuf,
    width: usize,
    height: usize,
    framerate: y4m::Ratio,
    colorspace: y4m::Colorspace,
) -> anyhow::Result<y4m::Encoder<impl Write>> {
    let ffmpeg_pipe = Command::new("ffmpeg")
        .args(["-i", "-", "-c:v", "ffv1", "-level", "3"])
        .arg(output)
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()?
        .stdin
        .unwrap();

    let encoder = y4m::EncoderBuilder::new(width, height, framerate)
        .with_colorspace(colorspace)
        .write_header(ffmpeg_pipe)?;

    Ok(encoder)
}

pub fn get_scenes(config: &Config) -> anyhow::Result<Vec<Scene>> {
    let json_path = config.output_directory.join("config").join("scenes.json");
    verify_filename(&json_path)?;

    let frame_count = get_frame_count(config)?;

    let progress_bar = ProgressBar::new(frame_count as u64);

    progress_bar.set_style(create_progress_style("{spinner:.green} [{elapsed_precise}] Detecting scene changes... [{wide_bar:.cyan/blue}] {percent:>3}% {human_pos:>8}/{human_len:>8} ({smooth_per_sec:>6} FPS, ETA: {smooth_eta:>3})")?);

    let progress_callback = |frames: usize, _keyframes: usize| {
        progress_bar.set_position(frames as u64);
    };

    let scenes = if json_path.exists() {
        let file = File::open(json_path)?;
        let reader = BufReader::new(file);

        progress_bar.set_position(frame_count as u64);
        progress_bar.finish();

        serde_json::from_reader(reader)?
    } else {
        let mut decoder = create_decoder(&config.source)?;

        let opts = DetectionOptions {
            analysis_speed: SceneDetectionSpeed::Standard,
            detect_flashes: true,
            min_scenecut_distance: None,
            max_scenecut_distance: None,
            lookahead_distance: 24,
        };

        let results =
            detect_scene_changes::<_, u16>(&mut decoder, opts, None, Some(&progress_callback));

        progress_bar.finish();

        if frame_count != results.frame_count {
            warn!("Source video had {frame_count} frames but only {} were processed by the scene detector.", results.frame_count);
        }

        let mut scenes = vec![];

        (0..results.scene_changes.len()).for_each(|i| {
            scenes.push(Scene {
                start_frame: results.scene_changes[i],
                end_frame: if i == results.scene_changes.len() - 1 {
                    frame_count - 1
                } else {
                    results.scene_changes[i + 1] - 1
                },
            })
        });

        serde_json::to_writer_pretty(&File::create(json_path)?, &scenes)?;

        scenes
    };

    Ok(scenes)
}

pub fn split_scenes(config: &Config) -> anyhow::Result<()> {
    let output_path = config.output_directory.join("source");
    verify_directory(output_path.as_path())?;

    let scenes = get_scenes(config)?;

    let progress_bar = ProgressBar::new(get_frame_count(config)? as u64);

    progress_bar.set_style(create_progress_style("{spinner:.green} [{elapsed_precise}] Splitting scenes...        [{wide_bar:.cyan/blue}] {percent:>3}% {human_pos:>8}/{human_len:>8} ({smooth_per_sec:>6} FPS, ETA: {smooth_eta:>3})")?);

    let mut decoder = create_decoder(&config.source).expect("Couldn't create decoder");

    for (scene_index, scene) in scenes.iter().enumerate() {
        let final_output_filename = output_path.join(format!("scene-{scene_index:05}.mkv"));
        let temporary_output_filename = output_path.join(format!("scene-{scene_index:05}.tmp.mkv"));

        if !final_output_filename.exists() {
            if temporary_output_filename.exists() {
                std::fs::remove_file(&temporary_output_filename)?;
            }

            let mut encoder = create_encoder(
                &temporary_output_filename,
                decoder.get_width(),
                decoder.get_height(),
                decoder.get_framerate(),
                decoder.get_colorspace(),
            )?;

            for _ in scene.start_frame..(scene.end_frame + 1) {
                encoder.write_frame(&decoder.read_frame()?)?;
                progress_bar.inc(1);
            }
        } else {
            for _ in scene.start_frame..(scene.end_frame + 1) {
                decoder.read_frame()?;
                progress_bar.inc(1);
                progress_bar.reset_eta();
            }
        }

        if temporary_output_filename.exists() {
            std::fs::rename(temporary_output_filename, final_output_filename)?;
        }
    }

    progress_bar.finish();

    Ok(())
}
