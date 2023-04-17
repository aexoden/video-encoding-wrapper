use std::process;

use clap::Parser;
use tracing::error;

use video_encoding_wrapper::config;
use video_encoding_wrapper::util;

fn main() {
    util::install_tracing().unwrap_or_else(|err| {
        eprintln!("FATAL: Could not initialize tracing: {err}");
        process::exit(1);
    });

    let config = config::Config::parse();

    video_encoding_wrapper::run(&config).unwrap_or_else(|err| {
        error!("Unexpected error while running application: {err}");
        process::exit(2);
    });
}
