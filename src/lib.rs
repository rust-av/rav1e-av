extern crate av_codec as codec;
extern crate av_data as data;
extern crate rav1e;
extern crate log;

use crate::codec::encoder::{Descr, Descriptor, Encoder};
use crate::codec::error::{Error as AvCodecError, Result as AvCodecResult};
use crate::data::frame::{ArcFrame, FrameBufferConv};
use crate::data::params::{CodecParams, MediaKind, VideoInfo};
use crate::data::value::Value;
use rav1e::prelude::{Config, Context, EncoderStatus, FrameType, Rational};
use std::sync::Arc;
// use log::debug;

// Encoded should be handled
fn rav1e_status_into_av_error(status: EncoderStatus) -> AvCodecError {
    match status {
        EncoderStatus::LimitReached => AvCodecError::Unsupported("LimitReached".to_owned()),
        EncoderStatus::NeedMoreData => AvCodecError::MoreDataNeeded,
        EncoderStatus::EnoughData => AvCodecError::Unsupported("EnoughData".to_owned()),
        EncoderStatus::NotReady => AvCodecError::Unsupported("NotReady".to_owned()),
        EncoderStatus::Failure => AvCodecError::Unsupported("Failure".to_owned()),
        EncoderStatus::Encoded => AvCodecError::Unsupported("Encoded".to_owned())
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
        // TODO
        Ok(())
    }

    // TODO: have it as default impl?
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
        //         let enc = self.enc.as_mut().unwrap();
        match self.ctx.receive_packet() {
            Ok(packet) => {
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

    fn flush(&mut self) -> AvCodecResult<()> {
        self.ctx.flush();
        // TODO: this cannot fail?
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
            // TODO: complete options
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
            bit_rate: 0, // TODO: expose the information
            convergence_window: 0,
            delay: 0,
        })
    }

    fn set_params(&mut self, params: &CodecParams) -> AvCodecResult<()> {
        if let Some(MediaKind::Video(ref info)) = params.kind {
            self.cfg.enc.width = info.width;
            self.cfg.enc.height = info.height;
        }
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
