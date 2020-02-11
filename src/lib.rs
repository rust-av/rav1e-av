extern crate av_codec as codec;
extern crate av_data as data;
extern crate log;
extern crate rav1e;

use crate::codec::encoder::{Descr, Descriptor, Encoder};
use crate::codec::error::{Error as AvCodecError, Result as AvCodecResult};
use crate::data::frame::{ArcFrame, FrameBufferConv};
use crate::data::params::{CodecParams, MediaKind, VideoInfo};
use crate::data::pixel::FromPrimitive;
use crate::data::pixel::ToPrimitive;
use crate::data::value::Value;
use log::debug;
use rav1e::prelude::{
    ChromaSamplePosition, ChromaSampling, ColorDescription, Config, Context, EncoderStatus,
    FrameType, Rational,
};
use std::sync::Arc;

// Encoded should be handled
fn rav1e_status_into_av_error(status: EncoderStatus) -> AvCodecError {
    match status {
        EncoderStatus::LimitReached => AvCodecError::Unsupported("LimitReached".to_owned()),
        EncoderStatus::NeedMoreData => AvCodecError::MoreDataNeeded,
        EncoderStatus::EnoughData => AvCodecError::Unsupported("EnoughData".to_owned()),
        EncoderStatus::NotReady => AvCodecError::Unsupported("NotReady".to_owned()),
        EncoderStatus::Failure => AvCodecError::Unsupported("Failure".to_owned()),
        EncoderStatus::Encoded => AvCodecError::Unsupported("Encoded".to_owned()),
    }
}

/// AV1 Encoder
pub struct Rav1eEncoder {
    cfg: Config,
    ctx: Context<u8>,
}

impl Rav1eEncoder {}

struct Des {
    descr: Descr,
}

impl Descriptor for Des {
    fn create(&self) -> Box<dyn Encoder> {
        let cfg = Config::default();
        let ctx = cfg.new_context().unwrap();
        Box::new(Rav1eEncoder { cfg, ctx })
    }

    fn describe(&self) -> &Descr {
        &self.descr
    }
}

impl Encoder for Rav1eEncoder {
    fn configure(&mut self) -> AvCodecResult<()> {
        Ok(())
    }

    fn get_extradata(&self) -> Option<Vec<u8>> {
        None
    }

    fn send_frame(&mut self, frame_in: &ArcFrame) -> AvCodecResult<()> {
        // TODO: 10 and 12 bits formats use 2 bytes
        if let data::frame::MediaKind::Video(ref _info) = frame_in.kind {
            let mut frame_out = self.ctx.new_frame();

            for i in 0..frame_in.buf.count() {
                let s: &[u8] = frame_in.buf.as_slice(i).unwrap();
                let stride = frame_in.buf.linesize(i).unwrap();

                debug!("Send frame plane {} with stride {}", i, stride);

                frame_out.planes[i].copy_from_raw_u8(s, stride, 1usize);
            }

            self.ctx
                .send_frame(frame_out)
                .map_err(rav1e_status_into_av_error)?;
            Ok(())
        } else {
            unimplemented!()
        }
    }

    fn receive_packet(&mut self) -> AvCodecResult<av_data::packet::Packet> {
        match self.ctx.receive_packet() {
            Ok(packet) => {
                debug!("Received packed {:?}", packet);

                Ok(av_data::packet::Packet {
                    data: packet.data,
                    pos: None,
                    stream_index: packet.input_frameno as isize, // TODO: ?
                    t: av_data::timeinfo::TimeInfo::default(),   // TODO: time
                    is_key: packet.frame_type == FrameType::KEY,
                    is_corrupted: false,
                })
            }
            Err(e) => Err(rav1e_status_into_av_error(e)),
        }
    }

    /// In Rav1e flush cannot fail
    fn flush(&mut self) -> AvCodecResult<()> {
        self.ctx.flush();
        Ok(())
    }

    fn set_option<'a>(&mut self, key: &str, val: Value<'a>) -> AvCodecResult<()> {
        match (key, val) {
            ("w", Value::U64(v)) => self.cfg.enc.width = v as usize,
            ("h", Value::U64(v)) => self.cfg.enc.height = v as usize,
            ("qmin", Value::U64(v)) => self.cfg.enc.min_quantizer = v as u8,
            ("qmax", Value::U64(v)) => self.cfg.enc.quantizer = v as usize,
            ("timebase", Value::Pair(num, den)) => {
                self.cfg.enc.time_base = Rational::new(num as u64, den as u64)
            }
            ("lowlatency", Value::Bool(v)) => self.cfg.enc.low_latency = v,
            ("tilecols", Value::U64(v)) => self.cfg.enc.tile_cols = v as usize,
            ("tilerows", Value::U64(v)) => self.cfg.enc.tile_rows = v as usize,
            ("tiles", Value::U64(v)) => self.cfg.enc.tiles = v as usize,
            ("maxkeyframe", Value::U64(v)) => self.cfg.enc.max_key_frame_interval = v,
            ("minkeyframe", Value::U64(v)) => self.cfg.enc.min_key_frame_interval = v,
            ("lookaheadframes", Value::U64(v)) => self.cfg.enc.rdo_lookahead_frames = v as usize,
            ("psnr", Value::Bool(v)) => self.cfg.enc.show_psnr = v,
            // TODO: complete options: speed settings, mastering display, content light, still picture, tune
            _ => unimplemented!(),
        }

        Ok(())
    }

    fn get_params(&self) -> AvCodecResult<CodecParams> {
        Ok(CodecParams {
            kind: Some(MediaKind::Video(VideoInfo {
                height: self.cfg.enc.height,
                width: self.cfg.enc.width,
                format: Some(Arc::new(*data::pixel::formats::YUV420)),
            })),
            codec_id: Some("av1".to_owned()),
            extradata: None,
            bit_rate: self.cfg.enc.bitrate as usize,
            convergence_window: 0,
            delay: self.cfg.enc.reservoir_frame_delay.unwrap_or_default() as usize,
        })
    }

    /// Setup [EncoderConfig](rav1e):
    fn set_params(&mut self, params: &CodecParams) -> AvCodecResult<()> {
        // TODO: extradata

        self.cfg.enc.bitrate = params.bit_rate as i32;

        if let Some(MediaKind::Video(ref info)) = params.kind {
            debug!("set_params received video media info: {:?}", info);

            self.cfg.enc.width = info.width;
            self.cfg.enc.height = info.height;
            self.cfg.enc.reservoir_frame_delay = Some(params.delay as i32);

            if let Some(format) = info.format.as_ref() {
                self.cfg.enc.color_description = Some(ColorDescription {
                    color_primaries: FromPrimitive::from_u64(
                        format.get_primaries().to_u64().unwrap_or_else(|| 2),
                    )
                    .unwrap(),
                    matrix_coefficients: FromPrimitive::from_u64(
                        format.get_matrix().to_u64().unwrap_or_else(|| 2),
                    )
                    .unwrap(),
                    transfer_characteristics: FromPrimitive::from_u64(
                        format.get_xfer().to_u64().unwrap_or_else(|| 2),
                    )
                    .unwrap(),
                });

                if format.get_num_comp() > 0 {
                    let chromaton = format.get_chromaton(0).unwrap();
                    self.cfg.enc.bit_depth = chromaton.get_depth() as usize;
                    if chromaton.get_subsampling() == (1, 1) {
                        self.cfg.enc.chroma_sampling = ChromaSampling::Cs420;
                    }
                    self.cfg.enc.chroma_sample_position = ChromaSamplePosition::Unknown // Explicit default
                }
            }
            // TODO: pixel_range
        }

        debug!("Rav1eEncoder Config: {:#?}", self.cfg.enc);

        Ok(())
    }
}

/// AV1 Encoder
///
/// To be used with [av-codec](https://docs.rs/av-codec) `Encoder Context`.
pub const AV1_DESCR: &dyn Descriptor = &Des {
    descr: Descr {
        codec: "av1",
        name: "rav1e",
        desc: "rav1e AV1 encoder",
        mime: "video/AV1",
    },
};
