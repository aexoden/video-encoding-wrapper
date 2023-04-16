use std::fs::File;
use std::io::{BufReader, Read};
use std::path::PathBuf;
use std::process::{Command, Stdio};

use av_scenechange::{detect_scene_changes, DetectionOptions, SceneDetectionSpeed};
use indicatif::ProgressBar;
use serde::{Deserialize, Serialize};
use tracing::warn;
use y4m;

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

pub fn get_scenes(config: &Config) -> anyhow::Result<Vec<Scene>> {
    let json_path = config.output_directory.join("config").join("scenes.json");
    verify_filename(&json_path)?;

    let frame_count = get_frame_count(config)?;

    let progress_bar = ProgressBar::new(frame_count as u64);

    progress_bar.set_style(create_progress_style("{spinner:.green} [{elapsed_precise}] Detecting scene changes... [{wide_bar:.cyan/blue}] {percent:>3}% {human_pos:>8}/{human_len:>8} ({smooth_per_sec} FPS, ETA: {smooth_eta})")?);

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

    for scene in scenes {
        println!("Scene begins at frame {}", scene.start_frame)
    }

    Ok(())
}
