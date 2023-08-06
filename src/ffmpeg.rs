use std::ffi::OsString;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;
use std::process::{Child, Command, Stdio};

use anyhow::{anyhow, Context};
use cached::{proc_macro::cached, UnboundCache};
use indicatif::{ProgressBar, ProgressStyle};
use serde::{Deserialize, Serialize};

use crate::config::Config;
use crate::util::verify_filename;

pub fn create_child_read(
    source: &Path,
    filter: Option<&str>,
    stdin: Stdio,
    stdout: Stdio,
    stderr: Stdio,
) -> anyhow::Result<Child> {
    let mut args: Vec<OsString> = vec![];

    args.push("-i".into());
    args.push(source.into());

    if let Some(filter) = filter {
        args.push("-vf".into());
        args.push(filter.into());
    }

    args.push("-pix_fmt".into());
    args.push("yuv420p10le".into());
    args.push("-f".into());
    args.push("yuv4mpegpipe".into());
    args.push("-strict".into());
    args.push("-1".into());
    args.push("-".into());

    let child = Command::new("ffmpeg")
        .args(&args)
        .stdin(stdin)
        .stdout(stdout)
        .stderr(stderr)
        .spawn()
        .context("Unable to spawn FFmpeg subprocess")?;

    Ok(child)
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct Metadata {
    pub frame_count: usize,
    pub duration: f64,
    pub crop_filter: Option<String>,
}

#[cached(
    result = true,
    type = "UnboundCache<String, Metadata>",
    create = "{ UnboundCache::with_capacity(1) }",
    convert = r#"{ format!("{}", config.source.to_string_lossy()) }"#
)]
pub fn get_metadata(config: &Config) -> anyhow::Result<Metadata> {
    let json_path = config.output_directory.join("config").join("metadata.json");

    verify_filename(&json_path)
        .with_context(|| format!("Unable to verify metadata cache file {json_path:?}"))?;

    let progress_bar = ProgressBar::new_spinner();
    let progress_template = "{spinner:.green} [{elapsed_precise}] Determining frame count and crop settings... Frames: {human_pos}  Crop: {msg}";
    progress_bar.set_style(
        ProgressStyle::with_template(progress_template)
            .context("Unable to create metadata progress bar style")?,
    );

    let metadata = if json_path.exists() {
        let file = File::open(&json_path)
            .with_context(|| format!("Unable to open metadata cache file {json_path:?}"))?;
        let reader = BufReader::new(file);

        let metadata: Metadata = serde_json::from_reader(reader)
            .with_context(|| format!("Unable to deserialize metadata cache from {json_path:?}"))?;

        progress_bar.set_position(
            metadata
                .frame_count
                .try_into()
                .context("Unable to convert frame count to u64")?,
        );

        progress_bar.finish_with_message(format!(
            "{} (cached)",
            metadata.crop_filter.as_deref().unwrap_or("None")
        ));

        metadata
    } else {
        let metadata =
            read_metadata(config, &progress_bar).context("Unable to read video metadata")?;

        serde_json::to_writer_pretty(
            &File::create(&json_path)
                .with_context(|| format!("Unable to create metadata cache file {json_path:?}"))?,
            &metadata,
        )
        .with_context(|| format!("Unable to serialize metadata cache to {json_path:?}"))?;

        metadata
    };

    Ok(metadata)
}

fn create_cropdetect_filter_graph(
    decoder: &ffmpeg::codec::decoder::Video,
    time_base: ffmpeg::Rational,
) -> anyhow::Result<ffmpeg::filter::Graph> {
    let mut filter = ffmpeg::filter::Graph::new();

    let args = format!(
        "width={}:height={}:pix_fmt={}:time_base={}:sar={}",
        decoder.width(),
        decoder.height(),
        decoder
            .format()
            .descriptor()
            .ok_or_else(|| anyhow!("Unable to determine pixel format"))?
            .name(),
        time_base,
        decoder.aspect_ratio(),
    );

    filter
        .add(
            &ffmpeg::filter::find("buffer")
                .ok_or_else(|| anyhow!("Unable to find FFmpeg buffer filter"))?,
            "in",
            &args,
        )
        .context("Unable to add FFmpeg buffer filter to filter graph")?;

    filter
        .add(
            &ffmpeg::filter::find("buffersink")
                .ok_or_else(|| anyhow!("Unable to find FFmpeg buffersink filter"))?,
            "out",
            "",
        )
        .context("Unable to add FFmpeg buffersink filter to filter graph")?;

    filter
        .output("in", 0)
        .context("Unable to initialize FFmpeg filter graph input")?
        .input("out", 0)
        .context("Unable to initialize FFmpeg filter graph output")?
        .parse("cropdetect=round=4")
        .context("Unable to add cropdetect filter to FFmpeg filter graph")?;

    filter
        .validate()
        .context("Unable to validate FFmpeg filter chain")?;

    Ok(filter)
}

fn read_metadata(config: &Config, progress_bar: &ProgressBar) -> anyhow::Result<Metadata> {
    let mut input_context = ffmpeg::format::input(&config.source)
        .with_context(|| format!("Unable to open {:?} with FFmpeg", &config.source))?;

    let (stream_index, mut decoder, time_base, duration) = {
        let input = input_context
            .streams()
            .best(ffmpeg::media::Type::Video)
            .ok_or(ffmpeg::Error::StreamNotFound)
            .with_context(|| format!("Unable to find video stream in {:?}", config.source))?;
        let decoder_context = ffmpeg::codec::context::Context::from_parameters(input.parameters())
            .context("Unable to create FFmpeg decoder context")?;
        let decoder = decoder_context
            .decoder()
            .video()
            .context("Unable to access FFmpeg decoder video")?;

        (input.index(), decoder, input.time_base(), input.duration())
    };

    let mut filter = create_cropdetect_filter_graph(&decoder, time_base)
        .context("Unable to create FFmpeg crop detection filter graph")?;

    let mut frame_count: usize = 0;
    let mut crop_filter: Option<String> = None;

    for (_stream, packet) in input_context
        .packets()
        .filter(|(stream, _packet)| stream.index() == stream_index)
    {
        frame_count += 1;
        progress_bar.inc(1);

        if packet.is_key() {
            decoder
                .send_packet(&packet)
                .context("Unable to decode video packet")?;

            let mut frame = ffmpeg::frame::Video::empty();

            while decoder.receive_frame(&mut frame).is_ok() {
                filter
                    .get("in")
                    .ok_or(ffmpeg::error::Error::FilterNotFound)
                    .context("Unable to find FFmpeg filter graph input filter")?
                    .source()
                    .add(&frame)
                    .context("Unable to add video frame to filter graph")?;

                filter
                    .get("out")
                    .ok_or(ffmpeg::error::Error::FilterNotFound)
                    .context("Unable to find FFmpeg filter graph output filter")?
                    .sink()
                    .frame(&mut frame)
                    .context("Unable to retrieve video frame from filter graph")?;

                let metadata = frame.metadata();

                if let Some(w) = metadata.get("lavfi.cropdetect.w") {
                    crop_filter = Some(format!(
                        "crop={}:{}:{}:{}",
                        w,
                        metadata
                            .get("lavfi.cropdetect.h")
                            .ok_or(ffmpeg::error::Error::Bug)
                            .context("Unexpectedly missing lavfi.cropdetect.h metadata field")?,
                        metadata
                            .get("lavfi.cropdetect.x")
                            .ok_or(ffmpeg::error::Error::Bug)
                            .context("Unexpectedly missing lavfi.cropdetect.x metadata field")?,
                        metadata
                            .get("lavfi.cropdetect.y")
                            .ok_or(ffmpeg::error::Error::Bug)
                            .context("Unexpectedly m issing lavfi.cropdetect.y metadata field")?,
                    ));

                    if let Some(crop_filter) = &crop_filter {
                        progress_bar.set_message(crop_filter.to_string());
                    }
                }
            }
        }
    }

    progress_bar.finish();

    #[allow(clippy::as_conversions)]
    #[allow(clippy::cast_precision_loss)]
    Ok(Metadata {
        frame_count,
        duration: duration as f64 * f64::from(time_base),
        crop_filter,
    })
}
