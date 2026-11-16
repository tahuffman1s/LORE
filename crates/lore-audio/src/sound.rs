//! A sound, ready to play: held in memory whole, or decoded as it plays.

use std::fmt;
use std::io::Cursor;
use std::path::Path;
use std::sync::Arc;

use crate::AudioError;
use crate::decode::{self, Decoder, Kind};

/// The slowest and fastest sample rates a sound may have.
const RATES: std::ops::RangeInclusive<u32> = 4_000..=384_000;

/// A sound, ready to play any number of times at once.
///
/// Cloning is cheap: every clone shares the same samples. Load each sound
/// once when the game starts and keep it.
///
/// Sounds that come from a place in the world should be mono. A stereo
/// sound played at a place is folded down to mono first.
///
/// A short sound is held in memory decoded, so that starting it costs
/// nothing. Music is too big for that (three minutes is about 60 MB), so
/// a long Ogg Vorbis file is [streamed](Self::streamed): only the file as
/// it is on disk is kept, and it is decoded a second ahead of where it is
/// playing, on a thread of its own. [`load`](Self::load) and
/// [`from_bytes`](Self::from_bytes) choose by length; either kind is
/// played the same way.
///
/// ```no_run
/// use lore_audio::{Play, Sound, Speakers};
///
/// # fn main() -> Result<(), lore_audio::AudioError> {
/// let mut speakers = Speakers::open_or_silent();
/// let theme = Sound::load("sounds/music/theme.ogg")?;
/// speakers.play(Play::everywhere(theme).music().looping());
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct Sound(Arc<Samples>);

struct Samples {
    data: Data,
    channels: usize,
    rate: u32,
    /// How many moments of sound there are.
    frames: usize,
}

enum Data {
    /// Decoded: one number per channel per moment, left before right.
    Whole(Box<[f32]>),
    /// The bytes of an Ogg Vorbis file, decoded as the sound plays.
    Packed(Box<[u8]>),
}

impl Sound {
    /// An Ogg Vorbis file longer than this many seconds is decoded as it
    /// plays; a shorter one is decoded when it is loaded.
    ///
    /// Ten seconds of stereo sound is about 4 MB decoded. Below that,
    /// holding the sound costs less than the half a megabyte and the
    /// decoding each streamed play needs; above it is music and weather.
    pub const STREAM_OVER_SECONDS: f32 = 10.0;

    /// A sound made from samples between -1 and 1.
    ///
    /// `rate` is how many moments there are a second, and `channels` is 1
    /// for mono or 2 for stereo (left sample, then right, for each moment).
    ///
    /// # Errors
    ///
    /// [`AudioError::TooManyChannels`], [`AudioError::BadRate`] or
    /// [`AudioError::Empty`] when the sound cannot be played.
    pub fn from_samples(
        rate: u32,
        channels: u32,
        mut samples: Vec<f32>,
    ) -> Result<Self, AudioError> {
        let channels = match channels {
            1 => 1,
            2 => 2,
            found => return Err(AudioError::TooManyChannels { found }),
        };
        if !RATES.contains(&rate) {
            return Err(AudioError::BadRate { found: rate });
        }
        samples.truncate(samples.len() - samples.len() % channels);
        if samples.is_empty() {
            return Err(AudioError::Empty);
        }
        Ok(Self::checked(rate, channels, samples))
    }

    /// A mono sound the engine made itself. Silence if `samples` is empty.
    pub(crate) fn mono(rate: u32, mut samples: Vec<f32>) -> Self {
        if samples.is_empty() {
            samples.push(0.0);
        }
        Self::checked(rate, 1, samples)
    }

    fn checked(rate: u32, channels: usize, mut samples: Vec<f32>) -> Self {
        for sample in &mut samples {
            *sample = if sample.is_finite() {
                sample.clamp(-1.0, 1.0)
            } else {
                0.0
            };
        }
        Self(Arc::new(Samples {
            frames: samples.len() / channels,
            data: Data::Whole(samples.into_boxed_slice()),
            channels,
            rate,
        }))
    }

    /// Reads a sound from the bytes of an Ogg Vorbis file, decoding all of
    /// it now. For music, use [`streamed`](Self::streamed).
    ///
    /// # Errors
    ///
    /// [`AudioError::NotOgg`] when the bytes are not an Ogg Vorbis file,
    /// and the errors of [`from_samples`](Self::from_samples).
    pub fn from_ogg(bytes: &[u8]) -> Result<Self, AudioError> {
        let mut decoder = Decoder::open(bytes)?;
        let (rate, channels) = (decoder.rate(), decoder.channels());
        let mut samples = Vec::new();
        let mut packet = Vec::new();
        while decoder.next(&mut packet)? {
            samples.extend_from_slice(&packet);
        }
        Self::from_samples(rate, channels, samples)
    }

    /// A sound that is decoded as it plays, from the bytes of an Ogg
    /// Vorbis file: what music wants. Only the file's own bytes are kept,
    /// about a tenth of what the decoded sound would take.
    ///
    /// The bytes of a WAV file are decoded now instead: they are no
    /// smaller than the sound they hold. So is the rare Ogg file that does
    /// not say how long it is.
    ///
    /// # Errors
    ///
    /// [`AudioError::NotOgg`] when the file's start cannot be read as Ogg
    /// Vorbis, [`AudioError::UnknownFormat`] when it is something else
    /// altogether, and the errors of [`from_samples`](Self::from_samples).
    /// Damage further into the file is only found when playing gets
    /// there: the sound ends early and the log says why.
    pub fn streamed(bytes: impl Into<Vec<u8>>) -> Result<Self, AudioError> {
        let bytes = bytes.into();
        match decode::kind_of(&bytes) {
            Kind::Wav => return Self::from_wav(&bytes),
            Kind::Unknown => return Err(AudioError::UnknownFormat),
            Kind::Vorbis | Kind::OtherOgg(_) => {}
        }
        let about = decode::about(&bytes)?;
        let channels = match about.channels {
            1 => 1,
            2 => 2,
            found => return Err(AudioError::TooManyChannels { found }),
        };
        if !RATES.contains(&about.rate) {
            return Err(AudioError::BadRate { found: about.rate });
        }
        let frames = usize::try_from(about.frames).unwrap_or(usize::MAX);
        if frames == 0 {
            // The file does not say how long it is, which a stream has to
            // know: decode it now and count.
            return Self::from_ogg(&bytes);
        }
        Ok(Self(Arc::new(Samples {
            data: Data::Packed(bytes.into_boxed_slice()),
            channels,
            rate: about.rate,
            frames,
        })))
    }

    /// Reads a sound from the bytes of a WAV or an Ogg Vorbis file,
    /// whichever they are. An Ogg Vorbis file longer than
    /// [`STREAM_OVER_SECONDS`](Self::STREAM_OVER_SECONDS) is
    /// [streamed](Self::streamed); anything else is decoded now.
    ///
    /// # Errors
    ///
    /// The errors of [`from_wav`](Self::from_wav),
    /// [`from_ogg`](Self::from_ogg) and [`streamed`](Self::streamed).
    pub fn from_bytes(bytes: impl Into<Vec<u8>>) -> Result<Self, AudioError> {
        let bytes = bytes.into();
        match decode::kind_of(&bytes) {
            Kind::Wav => Self::from_wav(&bytes),
            Kind::Unknown => Err(AudioError::UnknownFormat),
            Kind::Vorbis | Kind::OtherOgg(_) => {
                let about = decode::about(&bytes)?;
                let seconds = about.frames as f64 / f64::from(about.rate.max(1));
                if seconds > f64::from(Self::STREAM_OVER_SECONDS) {
                    Self::streamed(bytes)
                } else {
                    Self::from_ogg(&bytes)
                }
            }
        }
    }

    /// Reads a sound from the bytes of a WAV file.
    ///
    /// # Errors
    ///
    /// [`AudioError::NotWav`] when the bytes are not a WAV file, and the
    /// errors of [`from_samples`](Self::from_samples).
    pub fn from_wav(bytes: &[u8]) -> Result<Self, AudioError> {
        let not_wav = |error: hound::Error| AudioError::NotWav {
            reason: error.to_string(),
        };
        let mut reader = hound::WavReader::new(Cursor::new(bytes)).map_err(not_wav)?;
        let spec = reader.spec();
        let samples = match spec.sample_format {
            hound::SampleFormat::Float => reader
                .samples::<f32>()
                .collect::<Result<Vec<f32>, _>>()
                .map_err(not_wav)?,
            hound::SampleFormat::Int => {
                // The largest value a sample of this many bits can hold.
                let full = 2.0_f32.powi(i32::from(spec.bits_per_sample.clamp(1, 32)) - 1);
                reader
                    .samples::<i32>()
                    .map(|sample| sample.map(|sample| sample as f32 / full))
                    .collect::<Result<Vec<f32>, _>>()
                    .map_err(not_wav)?
            }
        };
        Self::from_samples(spec.sample_rate, u32::from(spec.channels), samples)
    }

    /// Reads a sound from a WAV or an Ogg Vorbis file. What the file holds
    /// decides, not its name. A long Ogg Vorbis file is
    /// [streamed](Self::streamed), as [`from_bytes`](Self::from_bytes) says.
    ///
    /// # Errors
    ///
    /// [`AudioError::CannotRead`] when the file cannot be opened, and the
    /// errors of [`from_bytes`](Self::from_bytes).
    pub fn load(path: impl AsRef<Path>) -> Result<Self, AudioError> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(|error| AudioError::CannotRead {
            path: path.to_path_buf(),
            reason: error.to_string(),
        })?;
        Self::from_bytes(bytes)
    }

    /// Decodes a streamed sound from beginning to end, keeping nothing, to
    /// find damage that playing it would only find on the way. A sound
    /// held in memory was decoded when it was loaded and has none.
    ///
    /// # Errors
    ///
    /// [`AudioError::NotOgg`] when part of the file cannot be decoded.
    pub fn check(&self) -> Result<(), AudioError> {
        if !self.is_streamed() {
            return Ok(());
        }
        let mut decoder = Decoder::open(self.packed())?;
        let mut packet = Vec::new();
        while decoder.next(&mut packet)? {}
        Ok(())
    }

    /// Whether the sound is decoded as it plays, not held in memory whole.
    #[must_use]
    pub fn is_streamed(&self) -> bool {
        matches!(self.0.data, Data::Packed(_))
    }

    /// How long the sound lasts at normal pitch, in seconds.
    #[must_use]
    pub fn seconds(&self) -> f32 {
        self.frames() as f32 / self.0.rate as f32
    }

    /// How many samples a second the sound was recorded at.
    #[must_use]
    pub fn rate(&self) -> u32 {
        self.0.rate
    }

    /// 1 for mono, 2 for stereo.
    #[must_use]
    pub fn channels(&self) -> u32 {
        self.0.channels as u32
    }

    /// How many moments of sound there are.
    pub(crate) fn frames(&self) -> usize {
        self.0.frames
    }

    pub(crate) fn is_stereo(&self) -> bool {
        self.0.channels == 2
    }

    /// The decoded samples. A streamed sound has none to give.
    pub(crate) fn data(&self) -> &[f32] {
        match &self.0.data {
            Data::Whole(data) => data,
            Data::Packed(_) => &[],
        }
    }

    /// The bytes of the file a streamed sound is decoded from.
    pub(crate) fn packed(&self) -> &[u8] {
        match &self.0.data {
            Data::Whole(_) => &[],
            Data::Packed(bytes) => bytes,
        }
    }
}

impl fmt::Debug for Sound {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Sound")
            .field("seconds", &self.seconds())
            .field("rate", &self.0.rate)
            .field("channels", &self.0.channels)
            .field("streamed", &self.is_streamed())
            .finish()
    }
}
