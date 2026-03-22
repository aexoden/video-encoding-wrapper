# video-encoding-wrapper

![Rust](https://github.com/aexoden/video-encoding-wrapper/actions/workflows/ci.yaml/badge.svg)

A highly opinionated video encoding wrapper.

This tool is not intended for users who are limited on temporary disk space or
who are looking for a fast solution or who have a slow CPU. The vast majority of
users will be better served by Av1an.

## Overview

This tool, given a source video file on the command line and an output directory
(and perhaps some other configuration flags), executes the following steps:

1. Determines the frame count and optimal crop settings for the video.
2. Finds scene changes in the video.
3. Extracts and crops each scene to a separate FFV1-encoded file.
4. Encodes the scenes using the configured settings.
5. Merges the encoded scenes into a final combined output file.
6. Collects and prints various metrics of the final file.

There are a few caveats: First, it processes the single best video stream from
the original file and completely ignores any other streams. The user is expected
to manually add any additional streams they want in a postprocessing step.
Second, because the scenes are encoded to separate files, this does require
extra disk space. This is completely intentional to allow for repeated runs with
different encoding settings, and to make testing settings via manual use of the
relevant tools easier. This is the singlest biggest reason that Av1an will
probably better serve the majority of users.

Along with the above, all calculated data is cached and can be reused when
appropriate. Aborted encodes will automatically reuse previously existing
encoded video when it makes sense to do so. Notwithstanding a bug, and assuming
the user does not delete or modify the output directory, nothing will be
calculated twice, even in repeated runs.

More detailed instructions may appear here at a later date when the tool is more
mature, but then again, they may not.

## Usage

```sh
video-encoding-wrapper [OPTIONS] <SOURCE> <OUTPUT_DIRECTORY>
```

### Arguments

- `<SOURCE>` — Source video file to encode. Used for metadata extraction, scene
  detection, and as the reference for quality metric calculations.
- `<OUTPUT_DIRECTORY>` — Root directory for all output files and caches. Created
  if it does not exist. Cached data (metadata, scene splits, encoded scenes,
  metrics) is reused across runs with the same source file.

### Options

| Flag | Long                   | Default     | Description                                                                                                                                                                                                             |
| ---- | ---------------------- | ----------- | ----------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------------- |
| `-e` | `--encoder`            | `x264`      | Video encoder: `aomenc`, `rav1e`, `svt-av1`, `vpxenc`, `x264`, or `x265`. Note that `rav1e` does not support CRF mode.                                                                                                  |
| `-p` | `--preset`             | `ultrafast` | Encoder-specific speed/quality preset, passed directly to the encoder (e.g. `ultrafast`..`veryslow` for x264/x265, `0`-`13` for svt-av1, `0`-`10` for aomenc/rav1e).                                                    |
| `-w` | `--workers`            | `1`         | Number of concurrent scene encoding workers. Each worker encodes scenes independently in parallel.                                                                                                                      |
| `-m` | `--mode`               | `qp`        | Rate control mode: `qp` (fixed quantizer), `crf` (constant rate factor), or `bitrate` (target bitrate in kbps). Bitrate mode always uses 2-pass encoding.                                                               |
| `-q` | `--quality`            | `23.0`      | Quality target value. In `direct` metric mode this is the literal QP, CRF, or bitrate value passed to the encoder. In other metric modes this is the target metric score for the quality search (e.g. a VMAF of 95.0).  |
|      | `--quality-metric`     | `direct`    | Quality metric for automatic quality search. With `direct`, the quality value is used as-is. Other metrics (`psnr`, `ssim`, `vmaf`, `ssimulacra2`, `bitrate`) trigger a binary search over the encoder's quality range. |
| `-r` | `--quality-rule`       | `minimum`   | Search direction during quality search (ignored in `direct` mode). See [Quality Search](#quality-search) below.                                                                                                         |
| `-u` | `--quality-mean`       | `false`     | Use arithmetic mean of per-frame metric values instead of a percentile. When set, `--quality-percentile` is ignored.                                                                                                    |
|      | `--quality-percentile` | `0.05`      | Percentile (0.0–1.0) of per-frame metric values to use as the quality measurement. `0.05` means the 5th percentile (i.e. worst 5% of frames). Only used when `--quality-mean` is not set.                               |

### Quality Search

When `--quality-metric` is set to anything other than `direct`, the tool
performs a binary search over the encoder's quality parameter range to find the
value that achieves the desired metric score. The search works in three steps:

1. **Encode** a scene at a trial quality level.
2. **Measure** per-frame metric values (PSNR, SSIM, VMAF, SSIMULACRA2, or
   bitrate).
3. **Aggregate** the per-frame values into a single number using either the
   arithmetic mean (`--quality-mean`) or a percentile (`--quality-percentile`,
   default 5th percentile — representing the worst 5% of frames).
4. **Compare** the aggregated metric against the `--quality` target, then adjust
   the search range according to `--quality-rule`.

The `--quality-rule` flag controls the search direction:

- **`minimum`** — Find the smallest resource expenditure that still meets the
  quality target. In bitrate mode, this means the lowest bitrate where the
  metric is ≥ the target. In QP/CRF modes (where higher values mean lower
  quality), this means the highest QP/CRF value where the metric is ≥ the
  target. Use this when you want to save space while guaranteeing a quality
  floor.

- **`maximum`** — Find the largest resource expenditure that stays at or below
  the quality target. In bitrate mode, this means the highest bitrate where the
  metric is ≤ the target. In QP/CRF modes, this means the lowest QP/CRF value
  where the metric is ≤ the target. Use this when you want to cap quality and
  avoid wasting bits.

- **`target`** — Converge on the encoder setting whose metric value is closest
  to the target, regardless of direction.

#### Examples

Encode with SVT-AV1 at the lowest bitrate that achieves at least VMAF 95 on the
worst 5% of frames:

```sh
video-encoding-wrapper -e svt-av1 -p 6 -m bitrate \
    --quality-metric vmaf -r minimum -q 95 \
    input.mkv output/
```

Encode with x265 CRF, targeting the exact SSIMULACRA2 score of 80 using the
mean across all frames:

```sh
video-encoding-wrapper -e x265 -p slow -m crf \
    --quality-metric ssimulacra2 -r target -u -q 80 \
    input.mkv output/
```

## Notes

VMAF is currently calculated independently for each scene. Because VMAF does
take into account changes between frames, this means the VMAF values at the
beginning and end of each scene are potentially different from what they would
be if VMAF were calculated on the final encode in its entirety. Testing has
indicated that these differences are not extensive, but the user should be
aware, especially if they compare with VMAF values calculated by an external
tool. It was determined calculating VMAF on the entire clip was not worth the
extra required time, especially as the author considers VMAF a secondary metric
at best.

## Third-Party Licenses

Much of the SSIMULACRA2 code is derived from the
[ssimulacra2_rs](https://github.com/rust-av/ssimulacra2) crate, licensed
under the following license:

> BSD 2-Clause License
>
> Copyright (c) 2022-2022, the rav1e contributors
> All rights reserved.
>
> Redistribution and use in source and binary forms, with or without
> modification, are permitted provided that the following conditions are met:
>
> - Redistributions of source code must retain the above copyright notice, this
>   list of conditions and the following disclaimer.
> - Redistributions in binary form must reproduce the above copyright notice,
>   this list of conditions and the following disclaimer in the documentation
>   and/or other materials provided with the distribution.
>
> THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
> AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
> IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
> DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
> FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
> DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
> SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
> CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
> OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
> OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
