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
use std::io::Read;
use std::path::Path;
use std::process::ChildStdout;
use std::sync::{mpsc, Arc, Mutex};
use std::thread;

use anyhow::{anyhow, Context};
use av_scenechange::{decoder::Decoder, ffmpeg::FfmpegDecoder};
use ssimulacra2::{
    compute_frame_ssimulacra2, ColorPrimaries, MatrixCoefficients, Pixel, TransferCharacteristic,
    Yuv, YuvConfig,
};

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

#[expect(clippy::significant_drop_tightening)]
#[expect(clippy::type_complexity)]
fn calc_score<S: Pixel, D: Pixel>(
    mutex: &Mutex<(usize, (Decoder<impl Read>, Decoder<impl Read>))>,
    reference_yuv_config: YuvConfig,
    distorted_yuv_config: YuvConfig,
) -> anyhow::Result<Option<(usize, f64)>> {
    let (frame_index, (reference_frame, distorted_frame)) = {
        let mut guard = mutex
            .lock()
            .map_err(|_err| anyhow!("Poison encountered when acquiring mutex lock"))?;
        let frame_index = guard.0;

        let reference_info = guard
            .1
             .0
            .get_video_details()
            .context("Unable to retreive reference video details")?;

        let distorted_info = guard
            .1
             .1
            .get_video_details()
            .context("Unable to retrieve distorted video detials")?;

        let reference_frame = guard
            .1
             .0
            .read_video_frame::<S>(&reference_info)
            .context("Unable to read reference video frame")?;

        let distorted_frame = guard
            .1
             .1
            .read_video_frame::<D>(&distorted_info)
            .context("Unable to read distorted video frame")?;

        guard.0 += 1;
        (frame_index, (reference_frame, distorted_frame))
    };

    let reference_yuv = Yuv::new(reference_frame, reference_yuv_config)
        .context("Unable to extract reference frame YUV")?;
    let distorted_yuv = Yuv::new(distorted_frame, distorted_yuv_config)
        .context("Unable to extract distorted frame YUV")?;

    Ok(Some((
        frame_index,
        compute_frame_ssimulacra2(reference_yuv, distorted_yuv)
            .context("Unable to compute SSIMULACRA2 score")?,
    )))
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
    let reference: Decoder<ChildStdout> = Decoder::Ffmpeg(
        FfmpegDecoder::new(reference_path)
            .context("Unable to create SSIMULACRA2 reference YUV4MPEG decoder")?,
    );

    let distorted: Decoder<ChildStdout> = Decoder::Ffmpeg(
        FfmpegDecoder::new(distorted_path)
            .context("Unable to create SSIMULACRA2 distorted YUV4MPEG decoder")?,
    );

    let reference_info = reference
        .get_video_details()
        .context("Unable to retrieve reference video details")?;

    let distorted_info = distorted
        .get_video_details()
        .context("Unable to retrieve distorted video details")?;

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

    let reference_subsampling = reference_info
        .chroma_sampling
        .get_decimation()
        .unwrap_or((0, 0));

    let distorted_subsampling = distorted_info
        .chroma_sampling
        .get_decimation()
        .unwrap_or((0, 0));

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

    let current_frame = 0_usize;
    let decoders = Arc::new(Mutex::new((current_frame, (reference, distorted))));

    thread::scope(|scope| -> anyhow::Result<Vec<f64>> {
        for _ in 0..threads {
            let decoders = Arc::clone(&decoders);
            let result_tx = result_tx.clone();

            scope.spawn(move || -> anyhow::Result<()> {
                loop {
                    let score = match (reference_info.bit_depth, distorted_info.bit_depth) {
                        (8, 8) => {
                            calc_score::<u8, u8>(&decoders, reference_config, distorted_config)
                        }
                        (8, _) => {
                            calc_score::<u8, u16>(&decoders, reference_config, distorted_config)
                        }
                        (_, 8) => {
                            calc_score::<u16, u8>(&decoders, reference_config, distorted_config)
                        }
                        (_, _) => {
                            calc_score::<u16, u16>(&decoders, reference_config, distorted_config)
                        }
                    }
                    .context("Unable to calculate SSIMULACRA2 score")?;

                    if let Some(result) = score {
                        result_tx
                            .send(result)
                            .context("Unable to send SSIMULACRA2 result to parent thread")?;
                    } else {
                        break;
                    }
                }

                Ok(())
            });
        }

        drop(result_tx);

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
