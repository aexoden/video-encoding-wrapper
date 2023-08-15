use std::path::PathBuf;

use clap::{Parser, ValueEnum};
use sha2::{Digest, Sha256};

pub struct QualityRange {
    minimum: i64,
    maximum: i64,
    divisor: i64,
}

impl QualityRange {
    #[must_use]
    pub const fn new(minimum: i64, maximum: i64, divisor: i64) -> Self {
        Self {
            minimum: minimum * divisor,
            maximum: maximum * divisor,
            divisor,
        }
    }

    #[must_use]
    #[allow(clippy::integer_division)]
    const fn midpoint(&self) -> i64 {
        (self.minimum + self.maximum) / 2
    }

    #[must_use]
    #[allow(clippy::as_conversions)]
    #[allow(clippy::cast_precision_loss)]
    pub fn current(&self) -> Option<f64> {
        if self.minimum > self.maximum {
            None
        } else {
            Some(self.midpoint() as f64 / self.divisor as f64)
        }
    }

    pub fn lower(&mut self) {
        self.maximum = self.midpoint() - 1;
    }

    pub fn higher(&mut self) {
        self.minimum = self.midpoint() + 1;
    }

    #[must_use]
    pub const fn divisor(&self) -> i64 {
        self.divisor
    }

    #[must_use]
    #[allow(clippy::as_conversions)]
    #[allow(clippy::cast_precision_loss)]
    pub fn minimum(&self) -> f64 {
        self.minimum as f64 / self.divisor as f64
    }

    #[must_use]
    #[allow(clippy::as_conversions)]
    #[allow(clippy::cast_precision_loss)]
    pub fn maximum(&self) -> f64 {
        self.maximum as f64 / self.divisor as f64
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub enum QualityRule {
    Maximum,
    Minimum,
    Target,
}

impl std::fmt::Display for QualityRule {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::Maximum => write!(f, "maximum"),
            Self::Minimum => write!(f, "minimum"),
            Self::Target => write!(f, "target"),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub enum Mode {
    QP,
    CRF,
}

impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::QP => write!(f, "qp"),
            Self::CRF => write!(f, "crf"),
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub enum Metric {
    Direct,
    PSNR,
    SSIM,
    VMAF,
    SSIMULACRA2,
    Bitrate,
}

impl std::fmt::Display for Metric {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Self::Direct => write!(f, "direct"),
            Self::PSNR => write!(f, "psnr"),
            Self::SSIM => write!(f, "ssim"),
            Self::VMAF => write!(f, "vmaf"),
            Self::SSIMULACRA2 => write!(f, "ssimulacra2"),
            Self::Bitrate => write!(f, "bitrate"),
        }
    }
}

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
    pub const fn quality_range(&self, mode: &Mode) -> QualityRange {
        match self {
            Self::Aomenc => QualityRange::new(0, 63, 1),
            Self::X264 => match mode {
                Mode::CRF => QualityRange::new(-12, 51, 4),
                Mode::QP => QualityRange::new(1, 81, 1),
            },
            Self::X265 => match mode {
                Mode::CRF => QualityRange::new(0, 51, 4),
                Mode::QP => QualityRange::new(0, 51, 1),
            },
        }
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
        mode: Mode,
        qp: f64,
    ) -> Vec<String> {
        // Base Arguments
        let mut arguments = self.base_arguments(preset, key_frame_interval);

        // Tune Arguments
        arguments.extend(self.tune_arguments());

        // Quality Arguments
        let qp_string = if self.quality_range(&mode).divisor == 1 {
            format!("{qp:0}")
        } else {
            format!("{qp:0.2}")
        };

        match self {
            Self::Aomenc => {
                arguments.push("--end-usage=q".to_owned());
                arguments.push(format!("--cq-level={qp_string}"));

                if mode == Mode::QP {
                    arguments.push(format!("--min-q={qp_string}"));
                    arguments.push(format!("--max-q={qp_string}"));
                }

                arguments.push("-y".to_owned());
            }
            Self::X264 | Self::X265 => {
                match mode {
                    Mode::CRF => {
                        arguments.push("--crf".to_owned());
                    }
                    Mode::QP => {
                        arguments.push("--qp".to_owned());
                    }
                }

                arguments.push(qp_string);
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

    /// Quality parameter in the encoder to adjust
    #[arg(short, long, value_enum, default_value_t = Mode::QP)]
    pub mode: Mode,

    /// Quality metric to target
    #[arg(long = "quality-metric", value_enum, default_value_t = Metric::Direct)]
    pub metric: Metric,

    /// Quality targeting rule
    #[arg(short, long = "quality-rule", value_enum, default_value_t = QualityRule::Minimum)]
    pub rule: QualityRule,

    /// Percentile to measure for target quality
    #[arg(long = "quality-percentile", value_parser = clap::value_parser!(f64), default_value_t = 0.05)]
    pub percentile: f64,

    /// Quality (QP or CRF) value to pass to the encoder
    #[arg(short, long, value_parser = clap::value_parser!(f64), default_value_t = 23.0)]
    pub quality: f64,

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
        let mode = self.mode.to_string();
        let metric = self.metric.to_string();
        let quality = self.quality;
        let percentile = self.percentile;
        let rule = self.rule.to_string();
        let constraint = "unconstrained";
        let hash = self.encode_arguments_hash();

        if include_quality {
            if self.metric == Metric::Direct {
                format!("{encoder}-{preset}-{mode}-{quality}-{constraint}-{hash}")
            } else {
                format!("{encoder}-{preset}-{mode}-{metric}-{rule}-{quality}-{percentile}-{constraint}-{hash}")
            }
        } else {
            format!("{encoder}-{preset}-{mode}-{constraint}-{hash}")
        }
    }

    #[must_use]
    pub fn mode_description(&self) -> String {
        match self.mode {
            Mode::CRF => "CRF".to_owned(),
            Mode::QP => "QP".to_owned(),
        }
    }
}
