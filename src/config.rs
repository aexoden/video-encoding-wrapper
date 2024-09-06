use std::fmt;
use std::path::{Path, PathBuf};

use base16ct::lower::encode_string;
use clap::{Parser, ValueEnum};
use sha2::{Digest, Sha256};

#[derive(Debug)]
pub struct QualityRange {
    minimum: i64,
    maximum: i64,
    divisor: i64,
    bitrate: bool,
}

impl QualityRange {
    #[must_use]
    pub const fn new(minimum: i64, maximum: i64, divisor: i64, bitrate: bool) -> Self {
        if bitrate {
            Self {
                minimum: minimum / divisor,
                maximum: maximum / divisor,
                divisor,
                bitrate,
            }
        } else {
            Self {
                minimum: minimum * divisor,
                maximum: maximum * divisor,
                divisor,
                bitrate,
            }
        }
    }

    #[must_use]
    const fn midpoint(&self) -> i64 {
        (self.minimum + self.maximum) / 2
    }

    #[must_use]
    #[expect(clippy::cast_precision_loss)]
    pub fn current(&self) -> Option<f64> {
        if self.minimum > self.maximum {
            None
        } else if self.bitrate {
            Some(self.midpoint() as f64 * self.divisor as f64)
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
    pub const fn integer(&self) -> bool {
        self.bitrate || self.divisor == 1
    }

    #[must_use]
    #[expect(clippy::cast_precision_loss)]
    pub fn minimum(&self) -> f64 {
        if self.bitrate {
            self.minimum as f64 * self.divisor as f64
        } else {
            self.minimum as f64 / self.divisor as f64
        }
    }

    #[must_use]
    #[expect(clippy::cast_precision_loss)]
    pub fn maximum(&self) -> f64 {
        if self.bitrate {
            self.maximum as f64 * self.divisor as f64
        } else {
            self.maximum as f64 / self.divisor as f64
        }
    }
}

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub enum QualityRule {
    Maximum,
    Minimum,
    Target,
}

#[expect(clippy::min_ident_chars)]
impl fmt::Display for QualityRule {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
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
    Bitrate,
}

#[expect(clippy::min_ident_chars)]
impl fmt::Display for Mode {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::QP => write!(f, "qp"),
            Self::CRF => write!(f, "crf"),
            Self::Bitrate => write!(f, "bitrate"),
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

#[expect(clippy::min_ident_chars)]
impl fmt::Display for Metric {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
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
    Rav1e,
    SvtAv1,
    Vpxenc,
    X264,
    X265,
}

#[expect(clippy::min_ident_chars)]
impl fmt::Display for Encoder {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            Self::Aomenc => write!(f, "aomenc"),
            Self::Rav1e => write!(f, "rav1e"),
            Self::SvtAv1 => write!(f, "svt-av1"),
            Self::Vpxenc => write!(f, "vpxenc"),
            Self::X264 => write!(f, "x264"),
            Self::X265 => write!(f, "x265"),
        }
    }
}

impl Encoder {
    #[must_use]
    pub fn extension(&self) -> String {
        match self {
            Self::Aomenc | Self::Rav1e | Self::SvtAv1 | Self::Vpxenc => "ivf",
            Self::X264 => "mkv",
            Self::X265 => "hevc",
        }
        .to_owned()
    }

    #[must_use]
    pub fn command(&self) -> String {
        match self {
            Self::Aomenc | Self::Rav1e | Self::Vpxenc | Self::X264 | Self::X265 => self.to_string(),
            Self::SvtAv1 => "SvtAv1EncApp".to_owned(),
        }
    }

    #[must_use]
    pub const fn quality_range(&self, mode: &Mode) -> QualityRange {
        match mode {
            Mode::Bitrate => QualityRange::new(100, 30000, 100, true),
            Mode::CRF => match self {
                Self::Aomenc | Self::Vpxenc => QualityRange::new(0, 63, 1, false),
                Self::Rav1e => QualityRange::new(1, 255, 1, false),
                Self::SvtAv1 => QualityRange::new(1, 63, 1, false),
                Self::X264 => QualityRange::new(-10, 51, 4, false),
                Self::X265 => QualityRange::new(0, 51, 4, false),
            },
            Mode::QP => match self {
                Self::Aomenc | Self::Vpxenc => QualityRange::new(0, 63, 1, false),
                Self::Rav1e => QualityRange::new(1, 255, 1, false),
                Self::SvtAv1 => QualityRange::new(1, 63, 1, false),
                Self::X264 => QualityRange::new(1, 81, 1, false),
                Self::X265 => QualityRange::new(0, 51, 1, false),
            },
        }
    }

    #[must_use]
    pub const fn passes(&self, config: &Config) -> usize {
        match config.mode {
            Mode::Bitrate => 2,
            Mode::CRF | Mode::QP => match self {
                Self::Aomenc | Self::Vpxenc => 2,
                Self::Rav1e | Self::SvtAv1 | Self::X264 | Self::X265 => 1,
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
            Self::Rav1e => vec![
                "--speed".to_owned(),
                preset.to_owned(),
                "--threads".to_owned(),
                "1".to_owned(),
                "--keyint".to_owned(),
                format!("{key_frame_interval}"),
            ],
            Self::SvtAv1 => vec![
                "--preset".to_owned(),
                preset.to_owned(),
                "--keyint".to_owned(),
                format!("{key_frame_interval}"),
                "--lp".to_owned(),
                "1".to_owned(),
                "--progress".to_owned(),
                "2".to_owned(),
            ],
            Self::Vpxenc => vec![
                format!("--cpu-used={preset}"),
                "--codec=vp9".to_owned(),
                "--bit-depth=10".to_owned(),
                "--profile=2".to_owned(),
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
    pub fn tune_arguments(&self, config: &Config) -> Vec<String> {
        match self {
            Self::Aomenc => {
                vec![
                    "--tune=ssim".to_owned(),
                    "--enable-qm=1".to_owned(),
                    "--lag-in-frames=48".to_owned(),
                    "--quant-b-adapt=1".to_owned(),
                    "--arnr-strength=1".to_owned(),
                    "--enable-keyframe-filtering=0".to_owned(),
                    "--dist-metric=qm-psnr".to_owned(),
                ]
            }
            Self::SvtAv1 => {
                if self.passes(config) > 1 {
                    vec!["--tune".to_owned(), "0".to_owned()]
                } else {
                    vec![
                        "--tune".to_owned(),
                        "0".to_owned(),
                        "--enable-overlays".to_owned(),
                        "1".to_owned(),
                    ]
                }
            }
            Self::Vpxenc => {
                vec!["--tune=ssim".to_owned()]
            }
            Self::Rav1e | Self::X264 | Self::X265 => {
                vec![]
            }
        }
    }

    #[must_use]
    #[expect(clippy::too_many_arguments)]
    #[expect(clippy::too_many_lines)]
    pub fn arguments(
        &self,
        config: &Config,
        preset: &str,
        key_frame_interval: usize,
        pass: Option<usize>,
        output_file: &Path,
        stats_file: Option<&PathBuf>,
        mode: Mode,
        qp: f64,
    ) -> Vec<String> {
        // Base Arguments
        let mut arguments = self.base_arguments(preset, key_frame_interval);

        // Tune Arguments
        arguments.extend(self.tune_arguments(config));

        // Quality Arguments
        let qp_string = if self.quality_range(&mode).integer() {
            format!("{qp:0}")
        } else {
            format!("{qp:0.2}")
        };

        #[expect(clippy::unreachable)]
        match self {
            Self::Aomenc | Self::Vpxenc => match mode {
                Mode::Bitrate => {
                    arguments.push("--end-usage=vbr".to_owned());
                    arguments.push(format!("--target-bitrate={qp_string}"));
                    arguments.push("--bias-pct=100".to_owned());
                }
                Mode::CRF | Mode::QP => {
                    arguments.push("--end-usage=q".to_owned());
                    arguments.push(format!("--cq-level={qp_string}"));

                    if mode == Mode::QP {
                        arguments.push(format!("--min-q={qp_string}"));
                        arguments.push(format!("--max-q={qp_string}"));
                        arguments.push("-y".to_owned());
                    }
                }
            },
            Self::Rav1e => match mode {
                Mode::Bitrate => {
                    arguments.push("--bitrate".to_owned());
                    arguments.push(qp_string);
                }
                Mode::CRF => {
                    unreachable!();
                }
                Mode::QP => {
                    arguments.push("--quantizer".to_owned());
                    arguments.push(qp_string);
                }
            },
            Self::SvtAv1 => {
                match mode {
                    Mode::Bitrate => {
                        arguments.push("--rc".to_owned());
                        arguments.push("1".to_owned());
                        arguments.push("--tbr".to_owned());
                    }
                    Mode::CRF => {
                        arguments.push("--crf".to_owned());
                    }
                    Mode::QP => {
                        arguments.push("--rc".to_owned());
                        arguments.push("0".to_owned());
                        arguments.push("--aq-mode".to_owned());
                        arguments.push("0".to_owned());
                        arguments.push("--qp".to_owned());
                    }
                }

                arguments.push(qp_string);
            }
            Self::X264 | Self::X265 => {
                match mode {
                    Mode::Bitrate => {
                        arguments.push("--bitrate".to_owned());
                    }
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
                    Self::Aomenc | Self::Vpxenc => {
                        arguments.push("--passes=2".to_owned());
                        arguments.push(format!("--pass={pass}"));
                        arguments.push(format!("--fpf={}", stats_file.to_string_lossy()));
                    }
                    Self::Rav1e => {
                        arguments.push(match pass {
                            1 => "--first-pass".to_owned(),
                            _ => "--second-pass".to_owned(),
                        });

                        arguments.push(stats_file.to_string_lossy().to_string());
                    }
                    Self::SvtAv1 | Self::X264 | Self::X265 => {
                        arguments.push("--pass".to_owned());
                        arguments.push(format!("{pass}"));
                        arguments.push("--stats".to_owned());
                        arguments.push(stats_file.to_string_lossy().to_string());
                    }
                }
            }
        }

        // Filename Arguments
        match self {
            Self::Aomenc | Self::Rav1e | Self::Vpxenc | Self::X264 | Self::X265 => {
                arguments.push("-o".to_owned());
                arguments.push(output_file.to_string_lossy().to_string());
                arguments.push("-".to_owned());
            }
            Self::SvtAv1 => {
                arguments.push("-b".to_owned());
                arguments.push(output_file.to_string_lossy().to_string());
                arguments.push("-i".to_owned());
                arguments.push("-".to_owned());
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

    /// Use mean instead of a percentile
    #[arg(short, long = "quality-mean", default_value_t = false)]
    pub use_mean: bool,

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
        let tune_arguments = self.encoder.tune_arguments(self);

        let mut hasher = Sha256::new();
        hasher.update(tune_arguments.join(" "));
        let result = hasher.finalize();

        encode_string(&result)
    }

    #[must_use]
    pub const fn passes(&self) -> usize {
        self.encoder.passes(self)
    }

    #[must_use]
    pub fn encode_identifier(&self, include_quality: bool) -> String {
        let encoder = self.encoder.to_string();
        let preset = self.preset.clone();
        let mode = self.mode.to_string();
        let metric = self.metric.to_string();
        let quality = self.quality;
        let rule = self.rule.to_string();
        let constraint = "unconstrained";
        let hash = self.encode_arguments_hash();

        let percentile = if self.use_mean {
            "mean".to_owned()
        } else {
            self.percentile.to_string()
        };

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
            Mode::Bitrate => "Bitrate".to_owned(),
            Mode::CRF => "CRF".to_owned(),
            Mode::QP => "QP".to_owned(),
        }
    }
}
