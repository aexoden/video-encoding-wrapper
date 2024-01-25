# video-encoding-wrapper

![Rust](https://github.com/aexoden/video-encoding-wrapper/actions/workflows/ci.yml/badge.svg)

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
[ssimulacra2_rs](https://github.com/rust-av/ssimulacra2_bin) crate, licensed
under the following license:

> BSD 2-Clause License
>
> Copyright (c) 2022-2022, the rav1e contributors
> All rights reserved.
>
> Redistribution and use in source and binary forms, with or without
> modification, are permitted provided that the following conditions are met:
>
>- Redistributions of source code must retain the above copyright notice, this
>  list of conditions and the following disclaimer.
>
>- Redistributions in binary form must reproduce the above copyright notice,
>  this list of conditions and the following disclaimer in the documentation
>  and/or other materials provided with the distribution.
>
>THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS"
>AND ANY EXPRESS OR IMPLIED WARRANTIES, INCLUDING, BUT NOT LIMITED TO, THE
>IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
>DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE
>FOR ANY DIRECT, INDIRECT, INCIDENTAL, SPECIAL, EXEMPLARY, OR CONSEQUENTIAL
>DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
>SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER
>CAUSED AND ON ANY THEORY OF LIABILITY, WHETHER IN CONTRACT, STRICT LIABILITY,
>OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE USE
>OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

Additional code for decoding YUV4MPEG and interfacing with the SSIMULACRA2 code
was derived from the
[av_metrics_decoders](https://github.com/rust-av/av-metrics/) crate, licensed
under the following license:

>The MIT License (MIT)
>Copyright (c) 2019 Joshua Holmer
>
>Permission is hereby granted, free of charge, to any person obtaining a copy of
>this software and associated documentation files (the "Software"), to deal in
>the Software without restriction, including without limitation the rights to
>use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies
>of the Software, and to permit persons to whom the Software is furnished to do
>so, subject to the following conditions:
>
>The above copyright notice and this permission notice shall be included in all
>copies or substantial portions of the Software.
>
>THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
>IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
>FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
>AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
>LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
>OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
>SOFTWARE.
