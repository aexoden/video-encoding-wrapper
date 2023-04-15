use clap::Parser;

use video_encoding_wrapper::config;

fn main() {
    let args = config::Config::parse();

    println!("Hello, world!");
}
