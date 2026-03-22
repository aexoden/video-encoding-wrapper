// Much of this code is derived from the ssimulacra2_rs crate, a binary interface to the ssimulacra2 crate, available at
// https://github.com/rust-av/ssimulacra2_bin, licensed under a 2 clause BSD license:
//
// BSD 2-Clause License
//
// Copyright (c) 2022-2022, the rav1e contributors
// All rights reserved.
//
// Redistribution and use in source and binary forms, with or without
// modification, are permitted provided that the following conditions are met:
//
// - Redistributions of source code must retain the above copyright notice, this
//   list of conditions and the following disclaimer.
//
// - Redistributions in binary form must reproduce the above copyright notice,
//   this list of conditions and the following disclaimer in the documentation
//   and/or other materials provided with the distribution.
//
// THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
// AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
// IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
// DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
// FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
// DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
// SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
// CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
// OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
// OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::collections::BTreeMap;
use std::path::Path;
use std::sync::{Arc, Mutex, mpsc};
use std::thread;

use anyhow::{Context, anyhow};
use av_decoders::{Decoder, DecoderError};
use ssimulacra2::{
    ColorPrimaries, MatrixCoefficients, TransferCharacteristic, compute_frame_ssimulacra2,
};
use v_frame::pixel::Pixel;
use yuvxyb::{ChromaSubsampling, Yuv, YuvConfig};

/// Converts a [`ChromaSubsampling`] to decimation shift amounts (x, y),
/// equivalent to the old `get_decimation()` API from `v_frame` 0.3.x.
const fn chroma_subsampling_decimation(cs: ChromaSubsampling) -> (usize, usize) {
    match cs {
        ChromaSubsampling::Yuv420 => (1, 1),
        ChromaSubsampling::Yuv422 => (1, 0),
        ChromaSubsampling::Yuv444 | ChromaSubsampling::Monochrome => (0, 0),
    }
}

const fn guess_matrix_coefficients(width: usize, height: usize) -> MatrixCoefficients {
    if width >= 1280 || height > 576 {
        MatrixCoefficients::BT709
    } else if height == 576 {
        MatrixCoefficients::BT470BG
    } else {
        MatrixCoefficients::ST170M
    }
}

fn guess_color_primaries(
    matrix: MatrixCoefficients,
    width: usize,
    height: usize,
) -> ColorPrimaries {
    if matrix == MatrixCoefficients::BT2020NonConstantLuminance
        || matrix == MatrixCoefficients::BT2020ConstantLuminance
    {
        ColorPrimaries::BT2020
    } else if matrix == MatrixCoefficients::BT709 || width >= 1280 || height > 576 {
        ColorPrimaries::BT709
    } else if height == 576 {
        ColorPrimaries::BT470BG
    } else if height == 480 || height == 488 {
        ColorPrimaries::ST170M
    } else {
        ColorPrimaries::BT709
    }
}

/// Converts a decoded frame to ssimulacra2's `LinearRgb` type, bridging the
/// version gap between yuvxyb 0.5.0 and ssimulacra2's pinned yuvxyb 0.4.2.
fn frame_to_ssim2_linear_rgb<T: Pixel>(
    frame: v_frame::frame::Frame<T>,
    config: YuvConfig,
) -> anyhow::Result<ssimulacra2::LinearRgb> {
    let yuv = Yuv::new(frame, config).context("Unable to construct YUV from frame")?;
    let linear: yuvxyb::LinearRgb =
        yuvxyb::LinearRgb::try_from(yuv).context("Unable to convert YUV to linear RGB")?;
    let w = linear.width().get();
    let h = linear.height().get();
    ssimulacra2::LinearRgb::new(linear.into_data(), w, h)
        .context("Unable to construct ssimulacra2 LinearRgb")
}

/// Reads the next frame from a decoder and converts it to ssimulacra2's
/// `LinearRgb`. Returns `Ok(None)` at end-of-file.
fn read_and_convert<T: Pixel>(
    decoder: &mut Decoder,
    config: YuvConfig,
) -> anyhow::Result<Option<ssimulacra2::LinearRgb>> {
    match decoder.read_video_frame::<T>() {
        Ok(frame) => Ok(Some(frame_to_ssim2_linear_rgb(frame, config)?)),
        Err(DecoderError::EndOfFile) => Ok(None),
        Err(e) => Err(e).context("Unable to read video frame"),
    }
}

#[expect(clippy::too_many_arguments)]
#[expect(clippy::too_many_lines)]
fn compare_videos(
    reference_path: &Path,
    distorted_path: &Path,
    threads: usize,
    mut reference_matrix: MatrixCoefficients,
    mut reference_transfer: TransferCharacteristic,
    mut reference_primaries: ColorPrimaries,
    reference_full_range: bool,
    mut distorted_matrix: MatrixCoefficients,
    mut distorted_transfer: TransferCharacteristic,
    mut distorted_primaries: ColorPrimaries,
    distorted_full_range: bool,
) -> anyhow::Result<Vec<f64>> {
    let reference: Decoder = Decoder::from_file(reference_path)
        .context("Unable to create SSIMULACRA2 reference video decoder")?;

    let distorted: Decoder = Decoder::from_file(distorted_path)
        .context("Unable to create SSIMULACRA2 distorted video decoder")?;

    let reference_info = reference.get_video_details();
    let distorted_info = distorted.get_video_details();

    let reference_bit_depth = reference_info.bit_depth;
    let distorted_bit_depth = distorted_info.bit_depth;

    if reference_matrix == MatrixCoefficients::Unspecified {
        reference_matrix = guess_matrix_coefficients(reference_info.width, reference_info.height);
    }

    if distorted_matrix == MatrixCoefficients::Unspecified {
        distorted_matrix = guess_matrix_coefficients(distorted_info.width, distorted_info.height);
    }

    if reference_transfer == TransferCharacteristic::Unspecified {
        reference_transfer = TransferCharacteristic::BT1886;
    }

    if distorted_transfer == TransferCharacteristic::Unspecified {
        distorted_transfer = TransferCharacteristic::BT1886;
    }

    if reference_primaries == ColorPrimaries::Unspecified {
        reference_primaries = guess_color_primaries(
            reference_matrix,
            reference_info.width,
            reference_info.height,
        );
    }

    if distorted_primaries == ColorPrimaries::Unspecified {
        distorted_primaries = guess_color_primaries(
            distorted_matrix,
            distorted_info.width,
            distorted_info.height,
        );
    }

    let reference_subsampling = chroma_subsampling_decimation(reference_info.chroma_sampling);
    let distorted_subsampling = chroma_subsampling_decimation(distorted_info.chroma_sampling);

    let reference_config = YuvConfig {
        bit_depth: reference_info
            .bit_depth
            .try_into()
            .context("Unable to cast bit depth to u8")?,
        subsampling_x: reference_subsampling
            .0
            .try_into()
            .context("Unable to cast horizontal subsampling to u8")?,
        subsampling_y: reference_subsampling
            .1
            .try_into()
            .context("Unable to cast vertical subsampling to u8")?,
        full_range: reference_full_range,
        matrix_coefficients: reference_matrix,
        transfer_characteristics: reference_transfer,
        color_primaries: reference_primaries,
    };

    let distorted_config = YuvConfig {
        bit_depth: distorted_info
            .bit_depth
            .try_into()
            .context("Unable to cast bit depth to u8")?,
        subsampling_x: distorted_subsampling
            .0
            .try_into()
            .context("Unable to cast horizontal subsampling to u8")?,
        subsampling_y: distorted_subsampling
            .1
            .try_into()
            .context("Unable to cast vertical subsampling to u8")?,
        full_range: distorted_full_range,
        matrix_coefficients: distorted_matrix,
        transfer_characteristics: distorted_transfer,
        color_primaries: distorted_primaries,
    };

    let (result_tx, result_rx) = mpsc::channel();

    thread::scope(|scope| -> anyhow::Result<Vec<f64>> {
        // Workers receive (index, ref_rgb, dist_rgb) and compute scores.
        // Decoder is not Send, so frame reading stays on the main thread.
        let (frame_tx, frame_rx) =
            mpsc::sync_channel::<(usize, ssimulacra2::LinearRgb, ssimulacra2::LinearRgb)>(threads);
        let frame_rx = Arc::new(Mutex::new(frame_rx));

        for _ in 0..threads {
            let rx = Arc::clone(&frame_rx);
            let tx = result_tx.clone();

            scope.spawn(move || -> anyhow::Result<()> {
                loop {
                    let (idx, ref_rgb, dist_rgb) = {
                        let guard = rx.lock().map_err(|_err| {
                            anyhow!("Poison encountered when acquiring mutex lock")
                        })?;
                        match guard.recv() {
                            Ok(item) => item,
                            Err(_) => break,
                        }
                    };

                    let score = compute_frame_ssimulacra2(ref_rgb, dist_rgb)
                        .context("Unable to compute SSIMULACRA2 score")?;
                    tx.send((idx, score))
                        .context("Unable to send SSIMULACRA2 result to parent thread")?;
                }
                Ok(())
            });
        }
        drop(result_tx);

        // Main thread: read frames and convert to LinearRgb, then send to workers.
        let mut reference = reference;
        let mut distorted = distorted;
        let mut frame_index = 0_usize;
        loop {
            let (ref_rgb, dist_rgb) = match (reference_bit_depth, distorted_bit_depth) {
                (8, 8) => (
                    read_and_convert::<u8>(&mut reference, reference_config),
                    read_and_convert::<u8>(&mut distorted, distorted_config),
                ),
                (8, _) => (
                    read_and_convert::<u8>(&mut reference, reference_config),
                    read_and_convert::<u16>(&mut distorted, distorted_config),
                ),
                (_, 8) => (
                    read_and_convert::<u16>(&mut reference, reference_config),
                    read_and_convert::<u8>(&mut distorted, distorted_config),
                ),
                (_, _) => (
                    read_and_convert::<u16>(&mut reference, reference_config),
                    read_and_convert::<u16>(&mut distorted, distorted_config),
                ),
            };

            let ref_rgb = ref_rgb.context("Unable to read reference frame")?;
            let dist_rgb = dist_rgb.context("Unable to read distorted frame")?;

            match (ref_rgb, dist_rgb) {
                (Some(r), Some(d)) => {
                    frame_tx
                        .send((frame_index, r, d))
                        .context("Unable to send frame pair to worker thread")?;
                    frame_index += 1;
                }
                _ => break,
            }
        }
        drop(frame_tx);

        let mut results = BTreeMap::new();

        for score in result_rx {
            results.insert(score.0, score.1);
        }

        Ok(results.into_values().collect())
    })
    .context("Unable to calculate SSIMULACRA2 scores")
}

pub fn calculate(
    distorted_path: &Path,
    reference_path: &Path,
    threads: usize,
) -> anyhow::Result<Vec<f64>> {
    compare_videos(
        distorted_path,
        reference_path,
        threads,
        MatrixCoefficients::Unspecified,
        TransferCharacteristic::Unspecified,
        ColorPrimaries::Unspecified,
        false,
        MatrixCoefficients::Unspecified,
        TransferCharacteristic::Unspecified,
        ColorPrimaries::Unspecified,
        false,
    )
}
