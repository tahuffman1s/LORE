//! Tests for the sounds of water. Nobody here has ears, so they are judged
//! by what can be measured: how long each is, how loud, which pitches hold
//! its loudness, and how it dies away.

use std::f64::consts::TAU;

use super::*;

/// Every water sound by the name a game plays it by, with its takes.
fn every() -> Vec<(&'static str, Vec<Sound>)> {
    let takes = |set: SoundSet| set.takes().to_vec();
    vec![
        ("splash", takes(splashes())),
        ("splash-small", takes(small_splashes())),
        ("wade", takes(wading())),
        ("swim-stroke", takes(swim_strokes())),
        ("surface", takes(surfacing())),
        ("bubbles", takes(bubbles())),
        ("underwater", vec![underwater()]),
        ("drip", takes(drips())),
        ("sizzle", takes(sizzles())),
    ]
}

/// The takes of the sound called `name`.
fn sound(name: &str) -> Vec<Sound> {
    let found = every().into_iter().find(|(known, _)| *known == name);
    found.unwrap().1
}

fn loudest(samples: &[f32]) -> f32 {
    samples.iter().fold(0.0_f32, |a, b| a.max(b.abs()))
}

/// How loud `samples` are on the whole: the root of their mean square.
fn level(samples: &[f32]) -> f32 {
    let squares: f64 = samples.iter().map(|s| f64::from(*s).powi(2)).sum();
    (squares / samples.len() as f64).sqrt() as f32
}

/// How much of a sound's loudness is at each pitch.
struct Pitches {
    /// The loudness in each band of pitches, lowest first.
    bands: Vec<f64>,
    /// How many hertz wide each band is.
    width: f64,
}

impl Pitches {
    /// Of `sound`, by the fast Fourier transform.
    fn of(sound: &Sound) -> Self {
        let size = sound.data().len().next_power_of_two();
        let mut real: Vec<f64> = sound.data().iter().map(|s| f64::from(*s)).collect();
        real.resize(size, 0.0);
        let mut imaginary = vec![0.0; size];
        // Each sample goes where its index read backwards in binary says.
        let bits = size.trailing_zeros();
        for index in 0..size {
            let swapped = index.reverse_bits() >> (usize::BITS - bits);
            if swapped > index {
                real.swap(index, swapped);
            }
        }
        let mut span = 2;
        while span <= size {
            let step = -TAU / span as f64;
            for start in (0..size).step_by(span) {
                for offset in 0..span / 2 {
                    let (sin, cos) = (step * offset as f64).sin_cos();
                    let (a, b) = (start + offset, start + offset + span / 2);
                    let (re, im) = (
                        real[b] * cos - imaginary[b] * sin,
                        real[b] * sin + imaginary[b] * cos,
                    );
                    (real[b], imaginary[b]) = (real[a] - re, imaginary[a] - im);
                    (real[a], imaginary[a]) = (real[a] + re, imaginary[a] + im);
                }
            }
            span *= 2;
        }
        Self {
            bands: (0..size / 2)
                .map(|bin| real[bin].powi(2) + imaginary[bin].powi(2))
                .collect(),
            width: f64::from(sound.rate()) / size as f64,
        }
    }

    /// The share of the loudness that is between `from` and `to` hertz.
    fn share(&self, from: f64, to: f64) -> f64 {
        let all: f64 = self.bands.iter().sum();
        let within: f64 = (self.bands.iter().enumerate())
            .filter(|(bin, _)| (from..to).contains(&(*bin as f64 * self.width)))
            .map(|(_, loud)| loud)
            .sum();
        within / all
    }

    /// The pitch the loudness balances at, in hertz: low for a dark sound,
    /// high for a bright one.
    fn middle(&self) -> f64 {
        let all: f64 = self.bands.iter().sum();
        let weighed: f64 = (self.bands.iter().enumerate())
            .map(|(bin, loud)| bin as f64 * self.width * loud)
            .sum();
        weighed / all
    }
}

#[test]
fn the_measure_of_pitch_finds_a_plain_note() {
    let note: Vec<f32> = (0..4_096)
        .map(|frame| (TAU * 1_500.0 * f64::from(frame) / 48_000.0).sin() as f32)
        .collect();
    let pitches = Pitches::of(&Sound::mono(48_000, note));
    assert!(pitches.share(1_400.0, 1_600.0) > 0.99);
    assert!((pitches.middle() - 1_500.0).abs() < 10.0);
}

#[test]
fn the_sine_made_from_plain_arithmetic_is_right_to_five_places() {
    for step in -4_000..=4_000 {
        let turns = step as f32 / 1_000.0;
        let real = (f64::from(turns) * TAU).sin() as f32;
        assert!((wave(turns) - real).abs() < 1.0e-5, "{turns}");
    }
}

#[test]
fn each_water_sound_has_its_takes_and_its_length() {
    let expected: [(&str, usize, std::ops::RangeInclusive<f32>); 9] = [
        ("splash", 3, 1.1..=1.8),
        ("splash-small", 4, 0.25..=0.4),
        ("wade", 4, 0.45..=0.6),
        ("swim-stroke", 4, 0.55..=0.7),
        ("surface", 2, 0.75..=0.9),
        ("bubbles", 3, 1.1..=1.3),
        ("underwater", 1, 4.0..=6.0),
        ("drip", 5, 0.14..=0.16),
        ("sizzle", 3, 0.55..=0.76),
    ];
    let every = every();
    assert_eq!(every.len(), expected.len());
    for ((name, takes), (expected, count, seconds)) in every.iter().zip(expected) {
        assert_eq!(*name, expected);
        assert_eq!(takes.len(), count, "{name}");
        for take in takes {
            assert!(seconds.contains(&take.seconds()), "{name}: {take:?}");
            assert_eq!(take.channels(), 1, "{name}");
        }
    }
}

#[test]
fn water_sounds_are_loud_enough_to_hear_and_never_clip() {
    for (name, takes) in every() {
        for take in &takes {
            let loudest = loudest(take.data());
            assert!((0.3..=0.95).contains(&loudest), "{name} peaks at {loudest}");
            // Not one tick in a long silence: there is sound all through.
            assert!(level(take.data()) > loudest / 30.0, "{name}");
        }
    }
    // A stroke is a small sound next to a body falling in.
    let peak = |name| loudest(sound(name)[0].data());
    assert!(peak("swim-stroke") < peak("splash") / 2.0);
}

#[test]
fn water_sounds_start_and_end_in_silence() {
    for (name, takes) in every() {
        if name == "underwater" {
            continue;
        }
        for take in &takes {
            let data = take.data();
            assert!(data[0].abs() < 0.01, "{name} starts at {}", data[0]);
            let end = data[data.len() - 1].abs();
            assert!(end < 0.001, "{name} ends at {end}");
        }
    }
}

#[test]
fn every_take_of_a_water_sound_is_its_own() {
    for (name, takes) in every() {
        for (index, take) in takes.iter().enumerate() {
            for other in &takes[index + 1..] {
                assert_ne!(take.data(), other.data(), "{name}");
            }
        }
    }
}

#[test]
fn water_sounds_are_the_same_every_time_they_are_made() {
    assert_eq!(splash(1).data(), splashes().takes()[1].data());
    assert_eq!(drip(4).data(), drips().takes()[4].data());
    assert_eq!(rumble().data(), underwater().data());
    // And asking again makes nothing new: the samples are shared.
    assert!(std::ptr::eq(underwater().data(), underwater().data()));
}

#[test]
fn a_splash_is_every_pitch_at_once_and_dies_away() {
    for take in sound("splash") {
        let pitches = Pitches::of(&take);
        // Low thump, the body of the water, and bright spray: all there.
        for (from, to) in [(0.0, 300.0), (300.0, 1_500.0), (1_500.0, 24_000.0)] {
            let share = pitches.share(from, to);
            assert!(share > 0.1, "{share} of it between {from} and {to} Hz");
        }
        let data = take.data();
        let third = data.len() / 3;
        let (first, last) = (level(&data[..third]), level(&data[third * 2..]));
        assert!(last < first / 8.0, "{first} then {last}");
        // The slap is at the very start: the loudest moment comes at once.
        let peak = loudest(data);
        let at = data.iter().position(|s| s.abs() == peak).unwrap();
        assert!(at < 48_000 / 10, "the loudest moment is {at} samples in");
        // And the tail is not silence: bubbles are still coming up.
        assert!(last > peak / 2_000.0, "{last}");
    }
}

#[test]
fn a_small_splash_is_bright_and_quickly_over() {
    for take in sound("splash-small") {
        let pitches = Pitches::of(&take);
        assert!(pitches.share(300.0, 24_000.0) > 0.8);
        assert!((500.0..5_000.0).contains(&pitches.middle()));
        let data = take.data();
        let half = data.len() / 2;
        assert!(level(&data[half..]) < level(&data[..half]) / 3.0);
    }
}

#[test]
fn wading_and_swimming_swell_and_have_no_slap() {
    for name in ["wade", "swim-stroke"] {
        for take in sound(name) {
            let data = take.data();
            let peak = loudest(data);
            // Nothing sudden: a hundredth of a second in it is still soft,
            // and the loudest moment comes later.
            assert!(loudest(&data[..480]) < peak / 4.0, "{name}");
            let at = data.iter().position(|s| s.abs() == peak).unwrap();
            assert!(at > 48_000 / 20, "{name} is loudest {at} samples in");
            let pitches = Pitches::of(&take);
            assert!((250.0..4_000.0).contains(&pitches.middle()), "{name}");
        }
    }
    // A stroke is the lighter and brighter of the two.
    let middle = |name| Pitches::of(&sound(name)[0]).middle();
    assert!(middle("swim-stroke") > middle("wade"));
}

#[test]
fn surfacing_is_a_rush_of_water_and_then_drips() {
    for take in sound("surface") {
        let data = take.data();
        let third = data.len() / 3;
        assert!(level(&data[third * 2..]) < level(&data[..third]) / 2.0);
        // The drips at the end are bright, single notes.
        let end = Sound::mono(48_000, data[third * 2..].to_vec());
        assert!(Pitches::of(&end).share(1_000.0, 24_000.0) > 0.7);
    }
}

#[test]
fn bubbles_heard_under_water_are_dull() {
    for take in sound("bubbles") {
        let pitches = Pitches::of(&take);
        assert!(pitches.share(0.0, 2_000.0) > 0.9);
        assert!(pitches.share(4_000.0, 24_000.0) < 0.01);
        // But they are bubbles, not a rumble: there are notes in it.
        assert!(pitches.share(300.0, 2_000.0) > 0.25);
    }
}

#[test]
fn a_sizzle_is_a_bright_hiss_that_is_loudest_at_once() {
    for take in sound("sizzle") {
        let pitches = Pitches::of(&take);
        // Steam, not water: nearly all of it over 1.5 kHz, and no one note.
        assert!(pitches.share(1_500.0, 24_000.0) > 0.85, "{take:?}");
        assert!(pitches.share(3_000.0, 8_000.0) < 0.9, "{take:?}");
        // Loud as it begins and all but gone by its second half.
        let data = take.data();
        let half = data.len() / 2;
        assert!(level(&data[half..]) < level(&data[..half]) / 4.0);
    }
}

#[test]
fn a_drip_is_a_tiny_bright_note() {
    for take in sound("drip") {
        let pitches = Pitches::of(&take);
        assert!(pitches.share(800.0, 24_000.0) > 0.9);
        assert!((1_000.0..4_500.0).contains(&pitches.middle()));
        // Over almost as soon as it begins.
        let data = take.data();
        let half = data.len() / 2;
        assert!(level(&data[half..]) < level(&data[..half]) / 10.0);
    }
}

#[test]
fn the_rumble_under_water_is_low_and_dark() {
    let rumble = underwater();
    let pitches = Pitches::of(&rumble);
    assert!(pitches.share(0.0, 400.0) > 0.9);
    assert!(
        pitches.share(20.0, 400.0) > 0.85,
        "not below hearing either"
    );
    assert!(pitches.middle() < 200.0);
    // It is steady: no second of it is much louder than another.
    let seconds: Vec<f32> = rumble.data().chunks(12_000).map(level).collect();
    let (quietest, loudest) = seconds.iter().fold((f32::MAX, 0.0_f32), |(low, high), s| {
        (low.min(*s), high.max(*s))
    });
    assert!(loudest < quietest * 2.0, "{seconds:?}");
}

#[test]
fn the_rumble_under_water_loops_with_no_seam() {
    let rumble = underwater();
    let data = rumble.data();
    let count = data.len();
    // The steps and the changes of step anywhere in the sound, to compare
    // with the ones across the join of its end to its beginning.
    let step = |at: usize| data[(at + 1) % count] - data[at % count];
    let bend = |at: usize| step(at + 1) - step(at);
    let biggest = |measure: &dyn Fn(usize) -> f32| {
        (0..count - 2)
            .map(|at| measure(at).abs())
            .fold(0.0_f32, f32::max)
    };
    let (biggest_step, biggest_bend) = (biggest(&step), biggest(&bend));
    // Across the join: from the last sample to the first, and the bends
    // either side of it.
    assert!(step(count - 1).abs() <= biggest_step, "a jump at the join");
    for at in [count - 3, count - 2, count - 1] {
        assert!(bend(at).abs() <= biggest_bend, "a corner at the join");
    }
    // Not by ending and starting in silence: it is as loud there as
    // anywhere.
    let join: Vec<f32> = (count - 600..count + 600)
        .map(|at| data[at % count])
        .collect();
    assert!(level(&join) > level(data) / 3.0);

    // And heard through a mixer, round and round: no step from one moment
    // to the next is bigger across the join than within the sound.
    let mut mixer = crate::Mixer::new(12_000);
    mixer.play(crate::Play::everywhere(rumble.clone()).looping());
    let mut out = vec![0.0; count * 2 * 2 + 2_000];
    mixer.mix(&mut out);
    let jump = |at: usize| (out[at * 2 + 2] - out[at * 2]).abs();
    // The first moments are the mixer fading the sound in.
    let within = (200..count - 2).map(jump).fold(0.0_f32, f32::max);
    for at in count - 2..count + 2 {
        assert!(jump(at) <= within, "{} against {within}", jump(at));
    }
}

/// The samples are made of plain arithmetic alone, which every computer
/// does alike. If this fails on one kind of computer and not another,
/// something that differs between them has crept in. If it fails
/// everywhere, a sound was changed: listen to it (`water_sounds_to_files`)
/// and put the new number here.
#[test]
fn water_sounds_are_the_same_on_every_computer() {
    let mut sum = 0xCBF2_9CE4_8422_2325_u64;
    for (_, takes) in every() {
        for sample in takes.iter().flat_map(Sound::data) {
            for byte in sample.to_bits().to_le_bytes() {
                sum = (sum ^ u64::from(byte)).wrapping_mul(0x0000_0100_0000_01B3);
            }
        }
    }
    assert_eq!(sum, 0xDFBD_BB99_21FD_21C5, "{sum:#018X}");
}

/// Writes every take to a WAV file, for someone with ears, and prints what
/// was measured of each:
///
/// ```sh
/// cargo test -p lore-audio water_sounds_to_files -- --ignored --nocapture
/// ```
#[test]
#[ignore = "writes files for a person to listen to"]
fn water_sounds_to_files() {
    let folder = std::env::temp_dir().join("lore-water-sounds");
    std::fs::create_dir_all(&folder).unwrap();
    let began = std::time::Instant::now();
    let every = every();
    println!("Making them all took {:?}.", began.elapsed());
    println!(
        "{:<16}{:>6}{:>6}{:>7}{:>8}{:>8}{:>8}{:>8}{:>8}{:>8}",
        "sound", "secs", "peak", "level", "middle", "<300", "<800", "<2000", "<5000", "thirds"
    );
    for (name, takes) in every {
        for (index, take) in takes.iter().enumerate() {
            let spec = hound::WavSpec {
                channels: 1,
                sample_rate: take.rate(),
                bits_per_sample: 16,
                sample_format: hound::SampleFormat::Int,
            };
            let path = folder.join(format!("{name}-{}.wav", index + 1));
            let mut writer = hound::WavWriter::create(&path, spec).unwrap();
            for sample in take.data() {
                writer
                    .write_sample((sample * f32::from(i16::MAX)) as i16)
                    .unwrap();
            }
            writer.finalize().unwrap();

            let data = take.data();
            let pitches = Pitches::of(take);
            let third = data.len() / 3;
            println!(
                "{:<16}{:>6.2}{:>6.2}{:>7.3}{:>8.0}{:>8.2}{:>8.2}{:>8.2}{:>8.2}  {:.3} {:.3} {:.3}",
                format!("{name}-{}", index + 1),
                take.seconds(),
                loudest(data),
                level(data),
                pitches.middle(),
                pitches.share(0.0, 300.0),
                pitches.share(0.0, 800.0),
                pitches.share(0.0, 2_000.0),
                pitches.share(0.0, 5_000.0),
                level(&data[..third]),
                level(&data[third..third * 2]),
                level(&data[third * 2..]),
            );
        }
    }
    println!("The water sounds are in {}", folder.display());
}
