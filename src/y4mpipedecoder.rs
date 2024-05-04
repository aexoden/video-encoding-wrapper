// This file is derived from the Y4M decoder in the av_metrics_decoders crate, released under an MIT license:
//
// The MIT License (MIT)
// Copyright (c) 2019 Joshua Holmer
//
// Permission is hereby granted, free of charge, to any person obtaining a copy of
// this software and associated documentation files (the "Software"), to deal in
// the Software without restriction, including without limitation the rights to
// use, copy, modify, merge, publish, distribute, sublicense, and/or sell copies
// of the Software, and to permit persons to whom the Software is furnished to do
// so, subject to the following conditions:
//
// The above copyright notice and this permission notice shall be included in all
// copies or substantial portions of the Software.
//
// THE SOFTWARE IS PROVIDED "AS IS", WITHOUT WARRANTY OF ANY KIND, EXPRESS OR
// IMPLIED, INCLUDING BUT NOT LIMITED TO THE WARRANTIES OF MERCHANTABILITY,
// FITNESS FOR A PARTICULAR PURPOSE AND NONINFRINGEMENT. IN NO EVENT SHALL THE
// AUTHORS OR COPYRIGHT HOLDERS BE LIABLE FOR ANY CLAIM, DAMAGES OR OTHER
// LIABILITY, WHETHER IN AN ACTION OF CONTRACT, TORT OR OTHERWISE, ARISING FROM,
// OUT OF OR IN CONNECTION WITH THE SOFTWARE OR THE USE OR OTHER DEALINGS IN THE
// SOFTWARE.

use std::io::{BufReader, Read};
use std::path::Path;
use std::process::{ChildStdout, Stdio};

use anyhow::{anyhow, Context};
use av_metrics::video::{
    decode::{convert_chroma_data, Rational},
    ChromaSamplePosition, Frame, Pixel,
};
use av_metrics_decoders::{ChromaSampling, Decoder, VideoDetails};

use crate::ffmpeg::create_child_read;

pub struct Y4MPipeDecoder<R: Read + Send> {
    inner: y4m::Decoder<R>,
}

#[allow(clippy::min_ident_chars)]
#[allow(clippy::unimplemented)]
fn map_y4m_color_space(color_space: y4m::Colorspace) -> (ChromaSampling, ChromaSamplePosition) {
    use av_metrics::video::ChromaSamplePosition as CSP;
    use av_metrics_decoders::ChromaSampling as CS;
    use y4m::Colorspace as C;

    match color_space {
        C::Cmono | C::Cmono12 => (CS::Cs400, CSP::Unknown),
        C::C420jpeg => (CS::Cs420, CSP::Bilateral),
        C::C420paldv => (CS::Cs420, CSP::Interpolated),
        C::C420mpeg2 => (CS::Cs420, CSP::Vertical),
        C::C420 | C::C420p10 | C::C420p12 => (CS::Cs420, CSP::Colocated),
        C::C422 | C::C422p10 | C::C422p12 => (CS::Cs422, CSP::Vertical),
        C::C444 | C::C444p10 | C::C444p12 => (CS::Cs444, CSP::Colocated),
        _ => unimplemented!(),
    }
}

pub fn new(path: &Path) -> anyhow::Result<Y4MPipeDecoder<BufReader<ChildStdout>>> {
    let decoder = y4m::Decoder::new(BufReader::new(
        create_child_read(path, None, Stdio::null(), Stdio::piped(), Stdio::null())
            .context("Unable to spawn SSIMULACRA2 video decoder subprocess")?
            .stdout
            .ok_or_else(|| {
                anyhow!("Unable to access stdout for SSIMULACRA2 video decoder subprocess")
            })?,
    ))
    .context("Unable to create SSIMULACRA2 YUV4MPEG decoder")?;

    Ok(Y4MPipeDecoder { inner: decoder })
}

#[allow(clippy::missing_trait_methods)]
impl<R> Decoder for Y4MPipeDecoder<R>
where
    R: Read + Send,
{
    fn get_video_details(&self) -> VideoDetails {
        let width = self.inner.get_width();
        let height = self.inner.get_height();
        let color_space = self.inner.get_colorspace();
        let bit_depth = color_space.get_bit_depth();
        let (chroma_sampling, chroma_sample_position) = map_y4m_color_space(color_space);
        let framerate = self.inner.get_framerate();
        #[allow(clippy::as_conversions)]
        let time_base = Rational::new(framerate.den as u64, framerate.num as u64);
        let luma_padding = 0;

        VideoDetails {
            width,
            height,
            bit_depth,
            chroma_sampling,
            chroma_sample_position,
            time_base,
            luma_padding,
        }
    }

    fn read_video_frame<T: Pixel>(&mut self) -> Option<Frame<T>> {
        let bit_depth = self.inner.get_bit_depth();
        let color_space = self.inner.get_colorspace();
        let (chroma_sampling, chroma_sample_pos) = map_y4m_color_space(color_space);
        let width = self.inner.get_width();
        let height = self.inner.get_height();
        let bytes = self.inner.get_bytes_per_sample();
        self.inner.read_frame().ok().map(|frame| {
            let mut new_frame: Frame<T> =
                Frame::new_with_padding(width, height, chroma_sampling, 0);

            let (chroma_width, _) = chroma_sampling.get_chroma_dimensions(width, height);
            new_frame.planes[0].copy_from_raw_u8(frame.get_y_plane(), width * bytes, bytes);
            convert_chroma_data(
                &mut new_frame.planes[1],
                chroma_sample_pos,
                bit_depth,
                frame.get_u_plane(),
                chroma_width * bytes,
                bytes,
            );
            convert_chroma_data(
                &mut new_frame.planes[2],
                chroma_sample_pos,
                bit_depth,
                frame.get_v_plane(),
                chroma_width * bytes,
                bytes,
            );

            new_frame
        })
    }

    fn get_bit_depth(&self) -> usize {
        self.inner.get_bit_depth()
    }
}
