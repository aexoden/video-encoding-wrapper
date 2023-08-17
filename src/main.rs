#![warn(future_incompatible)]
#![warn(let_underscore)]
#![warn(clippy::cargo)]
#![warn(clippy::nursery)]
#![warn(clippy::pedantic)]
#![warn(clippy::restriction)]
#![allow(clippy::arithmetic_side_effects)]
#![allow(clippy::blanket_clippy_restriction_lints)]
#![allow(clippy::exhaustive_enums)]
#![allow(clippy::exhaustive_structs)]
#![allow(clippy::float_arithmetic)]
#![allow(clippy::implicit_return)]
#![allow(clippy::missing_docs_in_private_items)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_inline_in_public_items)]
#![allow(clippy::pattern_type_mismatch)]
#![allow(clippy::question_mark_used)]
#![allow(clippy::separated_literal_suffix)]
#![allow(clippy::shadow_reuse)]
#![allow(clippy::std_instead_of_core)]

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
