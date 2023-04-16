use std::io::Read;
use std::path::PathBuf;
use std::process::{Command, Stdio};

use av_scenechange::{detect_scene_changes, DetectionOptions, SceneDetectionSpeed};
use indicatif::ProgressBar;
use y4m;

use crate::config::Config;
use crate::util::{create_progress_style, get_frame_count, verify_directory};

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

pub fn split_scenes(config: Config) -> anyhow::Result<()> {
    let output_path = config.output_directory.join("source");
    verify_directory(output_path.as_path())?;

    let mut decoder = create_decoder(&config.source)?;

    let opts = DetectionOptions {
        analysis_speed: SceneDetectionSpeed::Standard,
        detect_flashes: false,
        min_scenecut_distance: None,
        max_scenecut_distance: None,
        lookahead_distance: 1,
    };

    let frame_count = get_frame_count(&config.source)?;

    let progress_bar = ProgressBar::new(frame_count as u64);

    progress_bar.set_style(create_progress_style("{spinner:.green} [{elapsed_precise}] [{wide_bar:.cyan/blue}] {percent:>3}% {human_pos:>8}/{human_len:>8} ({smooth_per_sec} FPS, ETA: {smooth_eta})")?);

    let progress_callback = |frames: usize, _keyframes: usize| {
        progress_bar.set_position(frames as u64);
    };

    let results =
        detect_scene_changes::<_, u16>(&mut decoder, opts, None, Some(&progress_callback));

    progress_bar.finish_with_message("finished");

    for frame in results.scene_changes {
        println!("Scene begins at frame {frame}")
    }

    Ok(())
}
