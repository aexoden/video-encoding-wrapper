use std::path::PathBuf;

use clap::{Parser, ValueEnum};

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
pub enum Encoder {
    Aomenc,
    X264,
    X265,
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Config {
    /// Video encoder to use
    #[arg(short, long, value_enum, default_value_t = Encoder::X264)]
    pub encoder: Encoder,

    /// Source video file to encode
    pub source: String,

    /// Output directory
    pub output_directory: PathBuf,
}
