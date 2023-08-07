use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use sha2::{Digest, Sha256};

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub enum Encoder {
    Aomenc,
    X264,
    X265,
}

impl std::fmt::Display for Encoder {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::Aomenc => write!(f, "aomenc"),
            Self::X264 => write!(f, "x264"),
            Self::X265 => write!(f, "x265"),
        }
    }
}

impl Encoder {
    #[must_use]
    pub fn extension(&self) -> String {
        match self {
            Self::Aomenc => "ivf",
            Self::X264 => "mkv",
            Self::X265 => "hevc",
        }
        .to_owned()
    }

    #[must_use]
    pub fn base_arguments(&self, preset: &str, key_frame_interval: usize) -> Vec<String> {
        match self {
            Self::Aomenc => vec![
                format!("--cpu-used={preset}"),
                "--bit-depth=10".to_owned(),
                "--threads=1".to_owned(),
                format!("--kf-max-dist={key_frame_interval}"),
            ],
            Self::X264 => vec![
                "--stitchable".to_owned(),
                "--demuxer".to_owned(),
                "y4m".to_owned(),
                "--preset".to_owned(),
                preset.to_owned(),
                "--output-depth".to_owned(),
                "10".to_owned(),
                "--threads".to_owned(),
                "1".to_owned(),
                "--keyint".to_owned(),
                format!("{key_frame_interval}"),
            ],
            Self::X265 => vec![
                "--y4m".to_owned(),
                "--preset".to_owned(),
                preset.to_owned(),
                "--output-depth".to_owned(),
                "10".to_owned(),
                "--pools".to_owned(),
                "1".to_owned(),
                "-F".to_owned(),
                "1".to_owned(),
                "--keyint".to_owned(),
                format!("{key_frame_interval}"),
            ],
        }
    }

    #[must_use]
    pub fn tune_arguments(&self) -> Vec<String> {
        match self {
            Self::Aomenc => {
                vec![
                    "--tune=ssim".to_owned(),
                    "--enable-qm=1".to_owned(),
                    "--lag-in-frames=48".to_owned(),
                    "--quant-b-adapt=1".to_owned(),
                    "--arnr-strength=1".to_owned(),
                    "--enable-keyframe-filtering=2".to_owned(),
                    "--dist-metric=qm-psnr".to_owned(),
                ]
            }
            Self::X264 | Self::X265 => {
                vec![]
            }
        }
    }

    #[must_use]
    pub fn arguments(
        &self,
        preset: &str,
        key_frame_interval: usize,
        pass: Option<usize>,
        stats_file: Option<&PathBuf>,
        qp: i64,
    ) -> Vec<String> {
        // Base Arguments
        let mut arguments = self.base_arguments(preset, key_frame_interval);

        // Tune Arguments
        arguments.extend(self.tune_arguments());

        // Quality Arguments
        match self {
            Self::Aomenc => {
                arguments.push("--end-usage=q".to_owned());
                arguments.push(format!("--cq-level={qp}"));
                arguments.push(format!("--min-q={qp}"));
                arguments.push(format!("--max-q={qp}"));
                arguments.push("-y".to_owned());
            }
            Self::X264 | Self::X265 => {
                let qp = format!("{qp:.1}");

                arguments.push("--qp".to_owned());
                arguments.push(qp);
            }
        };

        // Pass Arguments
        if let Some(pass) = pass {
            if let Some(stats_file) = stats_file {
                match self {
                    Self::Aomenc => {
                        arguments.push("--passes=2".to_owned());
                        arguments.push(format!("--pass={pass}"));
                        arguments.push(format!("--fpf={}", stats_file.to_string_lossy()));
                    }
                    Self::X264 | Self::X265 => {
                        arguments.push("--pass".to_owned());
                        arguments.push(format!("{pass}"));
                        arguments.push("--stats".to_owned());
                        arguments.push(stats_file.to_string_lossy().to_string());
                    }
                }
            }
        }

        arguments
    }
}

#[derive(Clone, Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Config {
    /// Video encoder to use
    #[arg(short, long, value_enum, default_value_t = Encoder::X264)]
    pub encoder: Encoder,

    // Encoder-specific preset to use
    #[arg(short, long, default_value = "ultrafast")]
    pub preset: String,

    /// Number of workers
    #[arg(short, long, value_parser = clap::value_parser!(usize), default_value_t = 0)]
    pub workers: usize,

    /// Quality (QP) value to pass to the encoder
    #[arg(short, long, value_parser = clap::value_parser!(i64), default_value_t = 24)]
    pub quality: i64,

    /// Source video file to encode
    pub source: PathBuf,

    /// Output directory
    pub output_directory: PathBuf,
}

impl Config {
    fn encode_arguments_hash(&self) -> String {
        let tune_arguments = self.encoder.tune_arguments();

        let mut hasher = Sha256::new();
        hasher.update(tune_arguments.join(" "));
        let result = hasher.finalize();

        base16ct::lower::encode_string(&result)
    }

    #[must_use]
    pub fn encode_identifier(&self, include_quality: bool) -> String {
        let encoder = self.encoder.to_string();
        let preset = self.preset.clone();
        let mode = "qp";
        let quality = self.quality;
        let constraint = "unconstrained";
        let hash = self.encode_arguments_hash();

        if include_quality {
            format!("{encoder}-{preset}-{mode}-{quality}-{constraint}-{hash}")
        } else {
            format!("{encoder}-{preset}-{mode}-{constraint}-{hash}")
        }
    }
}
