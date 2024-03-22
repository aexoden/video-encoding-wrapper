use anyhow::{anyhow, Context};
use clap::Parser;

use video_encoding_wrapper::config;
use video_encoding_wrapper::util;

fn main() -> anyhow::Result<()> {
    util::install_tracing().context("Unable to install tracing subsystem")?;

    let config = config::Config::parse();

    if config.encoder == config::Encoder::Rav1e && config.mode == config::Mode::CRF {
        return Err(anyhow!(
            "rav1e does not currently support CRF mode. Use QP mode instead."
        ));
    }

    video_encoding_wrapper::run(&config).context("Unable to run application")?;

    Ok(())
}
