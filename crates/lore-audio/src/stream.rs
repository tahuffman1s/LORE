//! Decodes long sounds as they play, away from the thread that mixes.
//!
//! Each play of a streamed sound has a line of decoded sound a second
//! long. A [`Feeder`] keeps it full from the decoding thread; the mixer's
//! voice takes from it through a [`Reader`], which never waits, locks or
//! asks for memory. When the decoding falls behind, the voice is silent
//! until it catches up.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::mpsc;
use std::time::Duration;

use crate::Sound;
use crate::decode::Decoder;

/// How much decoded sound is kept ahead of where a stream is playing.
/// Long enough to ride out a busy computer, at a cost of 400 KB a stream.
const AHEAD_SECONDS: u32 = 1;

/// How long the decoding thread rests when every stream is full.
const REST: Duration = Duration::from_millis(5);

/// What the two ends of a stream both see.
#[derive(Debug, Default)]
struct Shared {
    /// No more sound will come: the file ended, or could not be read.
    ended: AtomicBool,
    /// How many times the voice ran out of decoded sound and fell silent.
    gaps: AtomicU32,
}

/// The mixer's end of a stream: decoded sound, left and right.
#[derive(Debug)]
pub(crate) struct Reader {
    frames: rtrb::Consumer<[f32; 2]>,
    shared: Arc<Shared>,
}

/// What a voice got when it asked a stream for sound.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Taken {
    Frame([f32; 2]),
    /// Nothing is decoded yet, but more will come.
    Waiting,
    /// The sound is over.
    Over,
}

impl Reader {
    /// The next moment of sound.
    pub(crate) fn take(&mut self) -> Taken {
        // Asked before looking, so that sound which arrives together with
        // the end is not missed.
        let ended = self.shared.ended.load(Ordering::Acquire);
        match self.frames.pop() {
            Ok(frame) => Taken::Frame(frame),
            Err(_) if ended => Taken::Over,
            Err(_) => Taken::Waiting,
        }
    }

    /// Notes that the sound fell silent for want of decoding.
    pub(crate) fn note_gap(&self) {
        self.shared.gaps.fetch_add(1, Ordering::Relaxed);
    }
}

/// The decoding end of a stream.
pub(crate) struct Feeder {
    sound: Sound,
    /// Opened by the decoding thread, not by the game's.
    decoder: Option<Decoder<Packed>>,
    frames: rtrb::Producer<[f32; 2]>,
    shared: Arc<Shared>,
    looping: bool,
    /// Decoded sound not yet handed over, and how far into it we are.
    decoded: Vec<f32>,
    at: usize,
    /// Whether this pass through the file has given any sound.
    any: bool,
    gaps_told: u32,
}

/// A streamed sound's file, as the decoder wants it.
struct Packed(Sound);

impl AsRef<[u8]> for Packed {
    fn as_ref(&self) -> &[u8] {
        self.0.packed()
    }
}

/// How a feeder is doing after being asked to fill its line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Fed {
    /// It handed over some sound, and may have more room soon.
    Some,
    /// The line was already full.
    Full,
    /// There is nothing more to do: the sound ended or nobody is listening.
    Done,
}

/// Makes the two ends of a stream for one play of `sound`.
pub(crate) fn open(sound: &Sound, looping: bool) -> (Reader, Feeder) {
    let ahead = (sound.rate() * AHEAD_SECONDS) as usize;
    let (producer, consumer) = rtrb::RingBuffer::new(ahead.max(1));
    let shared = Arc::new(Shared::default());
    let reader = Reader {
        frames: consumer,
        shared: Arc::clone(&shared),
    };
    let feeder = Feeder {
        sound: sound.clone(),
        decoder: None,
        frames: producer,
        shared,
        looping,
        decoded: Vec::new(),
        at: 0,
        any: false,
        gaps_told: 0,
    };
    (reader, feeder)
}

impl Feeder {
    /// Decodes until the line is full or the sound is over.
    pub(crate) fn fill(&mut self) -> Fed {
        if self.frames.is_abandoned() || self.shared.ended.load(Ordering::Relaxed) {
            return Fed::Done;
        }
        let gaps = self.shared.gaps.load(Ordering::Relaxed);
        if gaps != self.gaps_told {
            self.gaps_told = gaps;
            log::debug!(
                "A streamed sound fell silent for a moment: it could not be decoded as fast \
                 as it plays."
            );
        }
        let stereo = self.sound.is_stereo();
        let mut fed = Fed::Full;
        loop {
            while self.at < self.decoded.len() {
                let frame = if stereo {
                    [
                        self.decoded[self.at],
                        self.decoded.get(self.at + 1).copied().unwrap_or(0.0),
                    ]
                } else {
                    [self.decoded[self.at]; 2]
                };
                if self.frames.push(frame.map(tidy)).is_err() {
                    return fed;
                }
                self.at += if stereo { 2 } else { 1 };
                fed = Fed::Some;
            }
            if let Err(error) = self.decode() {
                log::warn!("A sound stopped early. {error}");
                return self.end();
            }
            if self.decoded.is_empty() {
                return self.end();
            }
        }
    }

    /// Fills `decoded` with the next stretch of the file, going back to
    /// its beginning when a looped sound ends. Leaves it empty when there
    /// is no more.
    fn decode(&mut self) -> Result<(), crate::AudioError> {
        self.at = 0;
        self.decoded.clear();
        // At most twice: the end of one pass, then the start of the next.
        for _ in 0..2 {
            let decoder = match &mut self.decoder {
                Some(decoder) => decoder,
                empty => empty.insert(Decoder::open(Packed(self.sound.clone()))?),
            };
            if decoder.next(&mut self.decoded)? {
                self.any = true;
                return Ok(());
            }
            // A file with no sound in it would otherwise be read for ever.
            if !self.looping || !self.any {
                return Ok(());
            }
            self.decoder = None;
            self.any = false;
        }
        Ok(())
    }

    fn end(&mut self) -> Fed {
        self.shared.ended.store(true, Ordering::Release);
        Fed::Done
    }
}

impl Drop for Feeder {
    /// Whatever became of the decoding, the voice must not wait for sound
    /// that will never come.
    fn drop(&mut self) {
        self.shared.ended.store(true, Ordering::Release);
    }
}

/// A sample fit to mix: a number between -1 and 1.
fn tidy(sample: f32) -> f32 {
    if sample.is_finite() {
        sample.clamp(-1.0, 1.0)
    } else {
        0.0
    }
}

/// The decoding thread, started by the first streamed sound and ended
/// when its owner and the sounds it was decoding are gone.
#[derive(Debug, Default)]
pub(crate) struct Streamer {
    feeders: Option<mpsc::Sender<Feeder>>,
}

impl Streamer {
    /// Hands a stream to the decoding thread to keep full.
    pub(crate) fn feed(&mut self, feeder: Feeder) {
        let feeder = match &self.feeders {
            Some(feeders) => match feeders.send(feeder) {
                Ok(()) => return,
                // The thread is gone; start another.
                Err(mpsc::SendError(feeder)) => feeder,
            },
            None => feeder,
        };
        let (feeders, incoming) = mpsc::channel();
        let started = std::thread::Builder::new()
            .name("lore-audio-decode".to_owned())
            .spawn(move || decode_streams(&incoming));
        match started {
            Ok(_) => {
                // Dropping the feeder ends its sound, should this fail.
                let _ = feeders.send(feeder);
                self.feeders = Some(feeders);
            }
            Err(error) => log::warn!(
                "Music and other long sounds will not play: the thread that decodes them \
                 could not be started ({error}). Close some programs and start the game again."
            ),
        }
    }
}

/// What the decoding thread does: keeps every stream it is handed full.
fn decode_streams(incoming: &mpsc::Receiver<Feeder>) {
    let mut feeders: Vec<Feeder> = Vec::new();
    let mut open = true;
    loop {
        if feeders.is_empty() {
            if !open {
                return;
            }
            // Nothing to decode: sleep until there is.
            match incoming.recv() {
                Ok(feeder) => feeders.push(feeder),
                Err(mpsc::RecvError) => return,
            }
        }
        loop {
            match incoming.try_recv() {
                Ok(feeder) => feeders.push(feeder),
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    open = false;
                    break;
                }
            }
        }
        let mut busy = false;
        feeders.retain_mut(|feeder| match feeder.fill() {
            Fed::Some => {
                busy = true;
                true
            }
            Fed::Full => true,
            Fed::Done => false,
        });
        if !busy {
            std::thread::sleep(REST);
        }
    }
}
