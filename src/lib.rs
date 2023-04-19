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
#![allow(clippy::integer_arithmetic)]
#![allow(clippy::missing_docs_in_private_items)]
#![allow(clippy::missing_errors_doc)]
#![allow(clippy::missing_inline_in_public_items)]
#![allow(clippy::pattern_type_mismatch)]
#![allow(clippy::separated_literal_suffix)]
#![allow(clippy::shadow_reuse)]
#![allow(clippy::std_instead_of_core)]

use anyhow::Context;

pub mod config;
pub mod ffmpeg;
pub mod scenes;
pub mod util;

pub fn run(config: &config::Config) -> anyhow::Result<()> {
    util::verify_directory(&config.output_directory).with_context(|| {
        format!(
            "Unable to verify or create output directory {:?}",
            &config.output_directory
        )
    })?;

    let _metadata = ffmpeg::get_metadata(config);

    scenes::split(config)
        .with_context(|| format!("Unable to split scenes for file {:?}", &config.source))?;

    Ok(())
}
