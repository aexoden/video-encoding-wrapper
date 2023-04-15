use clap::{Parser, ValueEnum};

#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
enum Encoder {
    Aomenc,
    X264,
    X265,
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Config {
    /// Video encoder to use
    #[arg(short, long, value_enum, default_value_t = Encoder::X264)]
    encoder: Encoder,

    /// Source video file to encode
    source: String,

    /// Output directory
    output_directory: String,
}
