//! Reads compressed sound: Ogg Vorbis, a packet at a time.
//!
//! Everything here asks for memory and takes its time, so none of it runs
//! on the thread that mixes: a short sound is decoded when it is loaded,
//! and a long one by the [`stream`](crate::stream) thread as it plays.
//!
//! Vorbis is the one codec for now. Opus (for packaged games) would be a
//! second kind of [`Decoder`], chosen in [`kind_of`] by the `OpusHead` its
//! first packet starts with; it sits in the same Ogg pages.

use std::io::Cursor;

use lewton::inside_ogg::OggStreamReader;
use lewton::samples::InterleavedSamples;

use crate::AudioError;

/// What a sound file's first bytes say it is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Kind {
    Wav,
    Vorbis,
    /// An Ogg file holding something that is not Vorbis: what it holds,
    /// as the error message says it.
    OtherOgg(&'static str),
    Unknown,
}

/// Looks at the start of a file to see what kind of sound it holds. The
/// file's name is never asked: a WAV saved as `.ogg` still plays.
pub(crate) fn kind_of(bytes: &[u8]) -> Kind {
    if bytes.starts_with(b"RIFF") {
        return Kind::Wav;
    }
    if !bytes.starts_with(b"OggS") {
        return Kind::Unknown;
    }
    // An Ogg page: 27 bytes, then one byte for each piece of what it
    // carries, then the first packet, which starts with its codec's name.
    let pieces = bytes.get(26).copied().map_or(0, usize::from);
    let packet = bytes.get(27 + pieces..).unwrap_or(&[]);
    if packet.starts_with(b"\x01vorbis") {
        Kind::Vorbis
    } else if packet.starts_with(b"OpusHead") {
        Kind::OtherOgg("Opus sound")
    } else if packet.starts_with(b"\x7fFLAC") {
        Kind::OtherOgg("FLAC sound")
    } else if packet.starts_with(b"Speex") {
        Kind::OtherOgg("Speex sound")
    } else {
        Kind::OtherOgg("something else")
    }
}

/// What an Ogg Vorbis file says about itself before any sound is decoded.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct About {
    pub rate: u32,
    pub channels: u32,
    /// How many moments of sound the file holds.
    pub frames: u64,
}

/// Decodes one Ogg Vorbis file from its bytes.
pub(crate) struct Decoder<T: AsRef<[u8]>> {
    reader: OggStreamReader<Cursor<T>>,
    channels: usize,
    /// How many moments of sound the file says it holds.
    frames: u64,
    /// How many of them are still to come. An encoder pads the end of a
    /// file, and what is past its stated length must not be played: a
    /// loop would have a gap in it.
    left: u64,
}

impl<T: AsRef<[u8]>> Decoder<T> {
    /// Reads the file's headers, ready to decode from its beginning.
    pub(crate) fn open(bytes: T) -> Result<Self, AudioError> {
        match kind_of(bytes.as_ref()) {
            Kind::Vorbis => {}
            Kind::OtherOgg(holds) => {
                return Err(not_ogg(format!("it holds {holds}, not Vorbis")));
            }
            Kind::Wav | Kind::Unknown => {
                return Err(not_ogg("it does not start as an Ogg file does".to_owned()));
            }
        }
        let frames = match last_position(bytes.as_ref()) {
            // No page says where the sound ends: take all there is.
            0 => u64::MAX,
            frames => frames,
        };
        let reader =
            OggStreamReader::new(Cursor::new(bytes)).map_err(|error| not_ogg(error.to_string()))?;
        let channels = usize::from(reader.ident_hdr.audio_channels);
        Ok(Self {
            reader,
            channels,
            frames,
            left: frames,
        })
    }

    /// How many moments of sound the file says it holds, or zero when it
    /// does not say.
    pub(crate) fn frames(&self) -> u64 {
        if self.frames == u64::MAX {
            0
        } else {
            self.frames
        }
    }

    pub(crate) fn rate(&self) -> u32 {
        self.reader.ident_hdr.audio_sample_rate
    }

    pub(crate) fn channels(&self) -> u32 {
        self.channels as u32
    }

    /// Replaces what is in `out` with the next stretch of sound, one
    /// number per channel per moment. False when the file has ended.
    pub(crate) fn next(&mut self, out: &mut Vec<f32>) -> Result<bool, AudioError> {
        out.clear();
        if self.left == 0 {
            return Ok(false);
        }
        loop {
            let packet = self
                .reader
                .read_dec_packet_generic::<InterleavedSamples<f32>>()
                .map_err(|error| not_ogg(error.to_string()))?;
            let Some(packet) = packet else {
                return Ok(false);
            };
            if packet.channel_count != self.channels {
                return Err(not_ogg(
                    "it changes its number of channels part way".to_owned(),
                ));
            }
            // The first packet of a file decodes to nothing.
            if !packet.samples.is_empty() && self.channels > 0 {
                let frames = (packet.samples.len() / self.channels) as u64;
                let kept = frames.min(self.left);
                self.left -= kept;
                // `kept` is no more than the packet holds.
                #[allow(clippy::cast_possible_truncation)]
                out.extend_from_slice(&packet.samples[..kept as usize * self.channels]);
                return Ok(true);
            }
        }
    }
}

/// Reads what an Ogg Vorbis file says about itself.
pub(crate) fn about(bytes: &[u8]) -> Result<About, AudioError> {
    let decoder = Decoder::open(bytes)?;
    Ok(About {
        rate: decoder.rate(),
        channels: decoder.channels(),
        frames: decoder.frames(),
    })
}

/// How many moments of sound an Ogg file holds: the position its last
/// page says it ends at. Zero when no page says.
fn last_position(bytes: &[u8]) -> u64 {
    let mut end = bytes.len();
    while let Some(start) = bytes[..end].windows(4).rposition(|four| four == b"OggS") {
        let position = bytes
            .get(start + 6..start + 14)
            .and_then(|eight| <[u8; 8]>::try_from(eight).ok())
            .map(u64::from_le_bytes);
        match position {
            // A page on which no packet ends says "none": look further back.
            Some(u64::MAX) | None => end = start,
            Some(position) => return position,
        }
    }
    0
}

fn not_ogg(reason: String) -> AudioError {
    AudioError::NotOgg { reason }
}
