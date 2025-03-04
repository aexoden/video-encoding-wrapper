use std::cmp::min;
use std::fmt::{Display, Formatter, Result, Write};
use std::fs::{File, create_dir_all};
use std::io::{BufWriter, Write as IoWrite};
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, anyhow};
use ffmpeg::util::log::level::Level as FFmpegLogLevel;
use ffmpeg::util::log::set_level as ffmpeg_set_log_level;
use indicatif::{HumanDuration, ProgressState, ProgressStyle};
use number_prefix::NumberPrefix;
use plotters::prelude::*;
use prettytable::{format::consts, row, table};
use statrs::statistics::{Data, Distribution, Max, Min, OrderStatistics};
use tracing::{error, level_filters::LevelFilter};
use tracing_error::ErrorLayer;
use tracing_subscriber::EnvFilter;
use tracing_subscriber::fmt::layer;
use tracing_subscriber::prelude::*;

pub const MINUS_THREE_SIGMA: f64 = 0.001_349_898;
pub const MINUS_TWO_SIGMA: f64 = 0.022_750_132;
pub const MINUS_ONE_SIGMA: f64 = 0.158_655_254;
pub const PLUS_ONE_SIGMA: f64 = 0.841_344_746;
pub const PLUS_TWO_SIGMA: f64 = 0.977_249_868;
pub const PLUS_THREE_SIGMA: f64 = 0.998_650_102;

#[expect(clippy::cast_possible_truncation)]
#[expect(clippy::cast_precision_loss)]
#[expect(clippy::cast_sign_loss)]
pub fn create_progress_style(template: &str) -> anyhow::Result<ProgressStyle> {
    let progress_style = ProgressStyle::with_template(template)
        .with_context(|| format!("Unable to create progress bar style with template '{template}'"))?
        .with_key("smooth_eta", |s: &ProgressState, w: &mut dyn Write| {
            match (s.pos(), s.len()) {
                (pos, Some(len)) if pos > 0 => write!(
                    w,
                    "{:#}",
                    HumanDuration(Duration::from_millis(
                        (s.elapsed().as_millis() as f64 * (len as f64 - pos as f64) / pos as f64)
                            .round() as u64
                    ))
                ),
                _ => write!(w, "-"),
            }
            .unwrap_or_else(|err| {
                error!("Unexpected error while formatting smooth_eta in progress bar: {err}");
            });
        })
        .with_key("smooth_per_sec", |s: &ProgressState, w: &mut dyn Write| {
            match (s.pos(), s.elapsed().as_millis()) {
                (pos, elapsed_ms) if elapsed_ms > 0 => {
                    write!(w, "{:.2}", pos as f64 * 1000_f64 / elapsed_ms as f64)
                }
                _ => write!(w, "-"),
            }
            .unwrap_or_else(|err| {
                error!("Unexpected error while formatting smooth_per_sec in progress bar: {err}");
            });
        });

    Ok(progress_style)
}

pub fn install_tracing() -> anyhow::Result<()> {
    ffmpeg_set_log_level(FFmpegLogLevel::Fatal);

    let env_filter = EnvFilter::builder()
        .with_default_directive(LevelFilter::WARN.into())
        .from_env_lossy();

    let fmt_layer = layer();

    tracing_subscriber::registry()
        .with(ErrorLayer::default())
        .with(fmt_layer.with_filter(env_filter))
        .try_init()
        .context("Unable to initialize global default subscriber")?;

    Ok(())
}

#[expect(clippy::cast_possible_truncation)]
#[expect(clippy::cast_precision_loss)]
#[expect(clippy::cast_sign_loss)]
pub fn print_histogram(data: &[f64]) -> anyhow::Result<()> {
    let min_value = data.iter().copied().fold(f64::INFINITY, f64::min);
    let max_value = data.iter().copied().fold(f64::NEG_INFINITY, f64::max);

    let bucket_size = ((max_value - min_value) / 16.0).ceil();
    let min_value = (min_value / bucket_size).floor() * bucket_size;
    let max_value = (max_value / bucket_size).ceil() * bucket_size;

    let num_buckets = ((max_value - min_value) / bucket_size).round() as usize;

    let mut buckets = vec![0; num_buckets];

    for &value in data {
        let index = min(
            ((value - min_value) / bucket_size).floor() as usize,
            num_buckets - 1,
        );

        if let Some(count) = buckets.get_mut(index) {
            *count += 1;
        }
    }

    let max_length = min(70, data.len());
    let digits = max_value.log10().ceil() as usize;

    for (i, &count) in buckets.iter().enumerate() {
        let lower_bound = (i as f64).mul_add(bucket_size, min_value);
        let upper_bound = lower_bound + bucket_size;

        #[expect(clippy::integer_division)]
        #[expect(clippy::integer_division_remainder_used)]
        let bar = "*".repeat(max_length * count / data.len());

        println!("{lower_bound:digits$} - {upper_bound:digits$} {count:6} {bar}");
    }

    Ok(())
}

pub fn generate_bitrate_chart(
    output_filename: &PathBuf,
    title: &str,
    offset: usize,
    series: &Vec<(String, &Vec<f64>)>,
) -> anyhow::Result<()> {
    let mut y_min = f64::MAX;
    let mut y_max = f64::MIN;
    let mut length = 0;

    for (_, data) in series {
        let stats = Data::new((*data).clone());
        let min = stats.min();
        let max = stats.max();

        if data.len() > length {
            length = data.len();
        }

        if min < y_min {
            y_min = min;
        }

        if max > y_max {
            y_max = max;
        }
    }

    length += offset;
    let y_range = y_max - y_min;
    let y_min = y_range.mul_add(-0.01, y_min);
    let y_max = y_range.mul_add(0.01, y_max);

    verify_filename(output_filename).with_context(|| {
        format!("Unable to verify {title} chart output filename {output_filename:?}")
    })?;

    let root = SVGBackend::new(output_filename, (1600, 800)).into_drawing_area();

    root.fill(&WHITE)
        .with_context(|| format!("Unable to fill {title} chart background"))?;

    let mut chart = ChartBuilder::on(&root)
        .caption(title, ("Arial", 32_i32).into_font())
        .margin(5_i32)
        .set_label_area_size(LabelAreaPosition::Top, 30_i32)
        .set_label_area_size(LabelAreaPosition::Bottom, 30_i32)
        .set_label_area_size(LabelAreaPosition::Left, 50_i32)
        .set_label_area_size(LabelAreaPosition::Right, 50_i32)
        .build_cartesian_2d(0..length, y_min..y_max)
        .with_context(|| format!("Unable to build {title} chart"))?;

    chart
        .configure_mesh()
        .draw()
        .with_context(|| format!("Unable to configure mesh for {title} chart"))?;

    for (i, (name, data)) in series.iter().enumerate() {
        let series_offset = length - data.len();

        chart
            .draw_series(LineSeries::new(
                data.iter()
                    .copied()
                    .enumerate()
                    .map(|(j, value)| (j + series_offset, value)),
                Palette99::pick(i),
            ))
            .with_context(|| format!("Unable to draw data series {name} for {title} chart"))?
            .label(name)
            .legend(move |(x, y)| {
                PathElement::new(vec![(x, y), (x + 20_i32, y)], Palette99::pick(i))
            });
    }

    chart
        .configure_series_labels()
        .background_style(WHITE.mix(0.8))
        .border_style(BLACK)
        .draw()?;

    root.present()
        .with_context(|| format!("Unable to finalize {title} chart"))?;

    Ok(())
}

pub fn generate_stat_chart(
    output_filename: &PathBuf,
    title: &str,
    data: &[f64],
) -> anyhow::Result<()> {
    let mut stats = Data::new(data.to_owned());

    let y_range = stats.max() - stats.min();
    let y_min = y_range.mul_add(-0.01, stats.min());
    let y_max = y_range.mul_add(0.01, stats.max());

    verify_filename(output_filename).with_context(|| {
        format!("Unable to verify {title} chart output filename {output_filename:?}")
    })?;

    let root = SVGBackend::new(output_filename, (1600, 800)).into_drawing_area();

    root.fill(&WHITE)
        .with_context(|| format!("Unable to fill {title} chart background"))?;

    let mut chart = ChartBuilder::on(&root)
        .caption(title, ("Arial", 32_i32).into_font())
        .margin(5_i32)
        .set_label_area_size(LabelAreaPosition::Top, 30_i32)
        .set_label_area_size(LabelAreaPosition::Bottom, 30_i32)
        .set_label_area_size(LabelAreaPosition::Left, 50_i32)
        .set_label_area_size(LabelAreaPosition::Right, 50_i32)
        .build_cartesian_2d(0..data.len(), y_min..y_max)
        .with_context(|| format!("Unable to build {title} chart"))?;

    chart
        .configure_mesh()
        .draw()
        .with_context(|| format!("Unable to configure mesh for {title} chart"))?;

    chart
        .draw_series(LineSeries::new(
            data.iter().copied().enumerate(),
            Palette99::pick(0),
        ))
        .with_context(|| format!("Unable to draw data series for {title} chart"))?;

    let mean = stats
        .mean()
        .with_context(|| format!("Unable to calculate mean for {title} chart"))?;

    let minus_one_sigma = stats.quantile(MINUS_ONE_SIGMA);
    let minus_two_sigma = stats.quantile(MINUS_TWO_SIGMA);
    let minus_three_sigma = stats.quantile(MINUS_THREE_SIGMA);

    chart
        .draw_series(LineSeries::new(
            (0..=data.len()).map(|x| (x, mean)),
            Palette99::pick(1),
        ))?
        .label(format!("Mean: {mean:0.3}"))
        .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20_i32, y)], Palette99::pick(1)));

    chart
        .draw_series(LineSeries::new(
            (0..=data.len()).map(|x| (x, minus_one_sigma)),
            Palette99::pick(2),
        ))?
        .label(format!("-1\u{3c3}: {minus_one_sigma:0.3}"))
        .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20_i32, y)], Palette99::pick(2)));

    chart
        .draw_series(LineSeries::new(
            (0..=data.len()).map(|x| (x, minus_two_sigma)),
            Palette99::pick(3),
        ))?
        .label(format!("-2\u{3c3}: {minus_two_sigma:0.3}"))
        .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20_i32, y)], Palette99::pick(3)));

    chart
        .draw_series(LineSeries::new(
            (0..=data.len()).map(|x| (x, minus_three_sigma)),
            Palette99::pick(4),
        ))?
        .label(format!("-3\u{3c3}: {minus_three_sigma:0.3}"))
        .legend(|(x, y)| PathElement::new(vec![(x, y), (x + 20_i32, y)], Palette99::pick(4)));

    chart
        .configure_series_labels()
        .background_style(WHITE.mix(0.8))
        .border_style(BLACK)
        .draw()?;

    root.present()
        .with_context(|| format!("Unable to finalize {title} chart"))?;

    Ok(())
}

pub fn generate_stat_log(
    output_filename: &PathBuf,
    title: &str,
    data: &[f64],
) -> anyhow::Result<()> {
    verify_filename(output_filename).with_context(|| {
        format!("Unable to verify {title} log output filename {output_filename:?}")
    })?;

    let file = File::create(output_filename).with_context(|| {
        format!("Unable to create {title} log output filename {output_filename:?}")
    })?;

    let mut writer = BufWriter::new(file);

    writeln!(writer, "# {title}")
        .with_context(|| format!("Unable to write title {title} to log"))?;

    #[expect(clippy::cast_possible_truncation)]
    #[expect(clippy::cast_precision_loss)]
    #[expect(clippy::cast_sign_loss)]
    let index_width = ((data.len() - 1) as f64).log10().floor() as usize + 1;

    for (i, value) in data.iter().enumerate() {
        writeln!(writer, "{i:0index_width$}: {value}").context("Unable to write data to log")?;
    }

    Ok(())
}

pub fn print_stats(stats: &mut Vec<(String, Vec<f64>)>) -> anyhow::Result<()> {
    let mut table = table!([
        "",
        "Minimum",
        "-3\u{3c3}",
        "-2\u{3c3}",
        "-1\u{3c3}",
        "Median",
        "1\u{3c3}",
        "2\u{3c3}",
        "3\u{3c3}",
        "Maximum",
        "Mean",
        "Std Dev"
    ]);

    table.set_format(*consts::FORMAT_BOX_CHARS);

    for (name, data) in stats {
        let mut data = Data::new(data);

        table.add_row(row![
            format!("{name:12}"),
            format!("{:8.3}", data.min()),
            format!("{:8.3}", data.quantile(MINUS_THREE_SIGMA)),
            format!("{:8.3}", data.quantile(MINUS_TWO_SIGMA)),
            format!("{:8.3}", data.quantile(MINUS_ONE_SIGMA)),
            format!("{:8.3}", data.median()),
            format!("{:8.3}", data.quantile(PLUS_ONE_SIGMA)),
            format!("{:8.3}", data.quantile(PLUS_TWO_SIGMA)),
            format!("{:8.3}", data.quantile(PLUS_THREE_SIGMA)),
            format!("{:8.3}", data.max()),
            format!(
                "{:8.3}",
                data.mean()
                    .with_context(|| format!("Unable to calculate mean for {name}"))?
            ),
            format!(
                "{:8.3}",
                data.std_dev().with_context(|| format!(
                    "Unable to calculate standard deviation for {name}"
                ))?
            ),
        ]);
    }

    table.printstd();

    Ok(())
}

pub fn verify_filename(path: &Path) -> anyhow::Result<()> {
    if let Some(parent) = path.parent() {
        create_dir_all(parent).with_context(|| format!("Unable to create directory {parent:?}"))?;
    }

    Ok(())
}

pub fn verify_directory(path: &Path) -> anyhow::Result<()> {
    if path.exists() {
        if !path.is_dir() {
            return Err(anyhow!("{path:?} exists but is not a directory"));
        }
    } else {
        create_dir_all(path).with_context(|| format!("Unable to create directory {path:?}"))?;
    }

    Ok(())
}

pub struct HumanBitrate(pub f64);

#[expect(clippy::min_ident_chars)]
impl Display for HumanBitrate {
    fn fmt(&self, f: &mut Formatter<'_>) -> Result {
        match NumberPrefix::decimal(self.0) {
            NumberPrefix::Standalone(number) => write!(f, "{number:.0} bps"),
            NumberPrefix::Prefixed(prefix, number) => write!(f, "{number:.3} {prefix}bps"),
        }
    }
}
