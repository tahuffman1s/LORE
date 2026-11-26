//! Tests for loading, placing, mixing and choosing sounds.

use std::f32::consts::{FRAC_PI_2, TAU};
use std::io::Cursor;

use crate::play::hear;
use crate::*;

const RATE: u32 = 48_000;

/// A mono sound of `frames` samples, all `level`.
fn steady(frames: usize, level: f32) -> Sound {
    Sound::from_samples(RATE, 1, vec![level; frames]).unwrap()
}

fn listener() -> Listener {
    Listener {
        position: [0.0, 1.6, 0.0],
        yaw: 0.0,
    }
}

/// Mixes `frames` moments and gives back the buffer.
fn mix(mixer: &mut Mixer, frames: usize) -> Vec<f32> {
    let mut out = vec![9.0; frames * 2];
    mixer.mix(&mut out);
    out
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() < 1.0e-4
}

/// The biggest jump between one moment and the next, in either ear.
fn biggest_jump(out: &[f32]) -> f32 {
    out.as_chunks::<2>()
        .0
        .iter()
        .zip(out.as_chunks::<2>().0.iter().skip(1))
        .map(|(a, b)| (a[0] - b[0]).abs().max((a[1] - b[1]).abs()))
        .fold(0.0, f32::max)
}

fn wav(
    spec: hound::WavSpec,
    write: impl FnOnce(&mut hound::WavWriter<&mut Cursor<Vec<u8>>>),
) -> Vec<u8> {
    let mut bytes = Cursor::new(Vec::new());
    let mut writer = hound::WavWriter::new(&mut bytes, spec).unwrap();
    write(&mut writer);
    writer.finalize().unwrap();
    bytes.into_inner()
}

// ---- Sounds ----

#[test]
fn a_sound_knows_its_length() {
    let sound = Sound::from_samples(RATE, 2, vec![0.0; 96_000]).unwrap();
    assert!(close(sound.seconds(), 1.0));
    assert_eq!((sound.rate(), sound.channels()), (RATE, 2));
}

#[test]
fn sounds_that_cannot_play_are_refused_with_advice() {
    let surround = Sound::from_samples(RATE, 6, vec![0.0; 60]).unwrap_err();
    assert_eq!(surround, AudioError::TooManyChannels { found: 6 });
    assert!(surround.to_string().contains("mono"));

    let empty = Sound::from_samples(RATE, 1, Vec::new()).unwrap_err();
    assert_eq!(empty, AudioError::Empty);

    let stopped = Sound::from_samples(0, 1, vec![0.0; 8]).unwrap_err();
    assert_eq!(stopped, AudioError::BadRate { found: 0 });
    assert!(stopped.to_string().contains("48000"));
}

#[test]
fn samples_that_are_not_numbers_become_silence() {
    let sound = Sound::from_samples(RATE, 1, vec![f32::NAN, f32::INFINITY, 3.0, -0.5]).unwrap();
    assert_eq!(sound.data(), &[0.0, 0.0, 1.0, -0.5]);
}

#[test]
fn a_16_bit_wav_loads() {
    let spec = hound::WavSpec {
        channels: 2,
        sample_rate: 44_100,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let bytes = wav(spec, |writer| {
        for sample in [0_i16, 16_384, -32_768, 32_767] {
            writer.write_sample(sample).unwrap();
        }
    });
    let sound = Sound::from_wav(&bytes).unwrap();
    assert_eq!((sound.rate(), sound.channels()), (44_100, 2));
    assert!(close(sound.data()[1], 0.5));
    assert!(close(sound.data()[2], -1.0));
    assert!(close(sound.data()[3], 1.0));
}

#[test]
fn a_float_wav_and_a_24_bit_wav_load() {
    let float = hound::WavSpec {
        channels: 1,
        sample_rate: RATE,
        bits_per_sample: 32,
        sample_format: hound::SampleFormat::Float,
    };
    let bytes = wav(float, |writer| writer.write_sample(0.25_f32).unwrap());
    assert_eq!(Sound::from_wav(&bytes).unwrap().data(), &[0.25]);

    let deep = hound::WavSpec {
        bits_per_sample: 24,
        sample_format: hound::SampleFormat::Int,
        ..float
    };
    let bytes = wav(deep, |writer| writer.write_sample(-4_194_304_i32).unwrap());
    assert!(close(Sound::from_wav(&bytes).unwrap().data()[0], -0.5));
}

#[test]
fn a_file_that_is_not_a_wav_says_what_to_do() {
    let error = Sound::from_wav(b"OggS this is something else").unwrap_err();
    assert!(matches!(error, AudioError::NotWav { .. }));
    assert!(
        error
            .to_string()
            .contains("Export the sound as a 16-bit WAV")
    );
}

#[test]
fn a_missing_file_is_named() {
    let error = Sound::load("no/such/gunshot.wav").unwrap_err();
    assert!(matches!(error, AudioError::CannotRead { .. }));
    assert!(error.to_string().contains("gunshot.wav"));
}

// ---- Where a sound is heard ----

fn heard_at(position: [f32; 3]) -> (f32, f32, f32) {
    let heard = hear(&listener(), Place::At(position), Range::default());
    (heard.left, heard.right, heard.muffle)
}

#[test]
fn a_sound_is_louder_in_the_nearer_ear() {
    let (left, right, _) = heard_at([5.0, 1.6, 0.0]);
    assert!(right > 0.19 && left < 0.001, "{left} {right}");
    let (left, right, _) = heard_at([-5.0, 1.6, 0.0]);
    assert!(left > 0.19 && right < 0.001, "{left} {right}");
    let (left, right, muffle) = heard_at([0.0, 1.6, -5.0]);
    assert!(close(left, right));
    assert_eq!(muffle, 0.0);
}

#[test]
fn turning_the_head_moves_the_sound() {
    // Facing +X: what was ahead at -Z is now on the left.
    let facing_right = Listener {
        yaw: -FRAC_PI_2,
        ..listener()
    };
    let heard = hear(&facing_right, Place::At([0.0, 1.6, -5.0]), Range::default());
    assert!(heard.left > heard.right * 100.0);
    assert!(heard.muffle < 1.0e-4);
}

#[test]
fn volume_halves_when_distance_doubles() {
    let loudness = |metres: f32| {
        let (left, right, _) = heard_at([0.0, 1.6, -metres]);
        (left * left + right * right).sqrt()
    };
    assert!(close(loudness(0.5), 1.0));
    assert!(close(loudness(1.0), 1.0));
    assert!(close(loudness(2.0), 0.5));
    assert!(close(loudness(4.0), 0.25));
    assert!(close(loudness(8.0), 0.125));
}

#[test]
fn a_sound_fades_out_before_its_range_ends() {
    let loudness = |metres: f32| heard_at([0.0, 1.6, -metres]).0;
    assert!(loudness(44.0) > loudness(46.0));
    assert!(loudness(49.9) > 0.0 && loudness(49.9) < 0.001);
    assert_eq!(loudness(50.0), 0.0);
    assert_eq!(loudness(500.0), 0.0);
}

#[test]
fn a_sound_behind_is_quieter_and_duller() {
    let (ahead, _, _) = heard_at([0.0, 1.6, -5.0]);
    let (behind, other, muffle) = heard_at([0.0, 1.6, 5.0]);
    assert!(close(behind, other));
    assert!(close(behind, ahead * 0.7));
    assert!(close(muffle, 1.0));
    // To the side it is neither.
    assert!(heard_at([5.0, 1.6, 0.0]).2 < 1.0e-4);
}

#[test]
fn a_sound_on_top_of_the_listener_has_no_side() {
    let (left, right, muffle) = heard_at([0.0, 1.6, 0.0]);
    assert!(close(left, right) && left > 0.7);
    assert_eq!(muffle, 0.0);
    // Just to one side, it leans only a little.
    let (left, right, _) = heard_at([0.1, 1.6, 0.0]);
    assert!(right > left && right < left * 1.3);
}

#[test]
fn nonsense_places_and_ranges_are_silent_not_broken() {
    let nowhere = hear(
        &listener(),
        Place::At([f32::NAN, 0.0, 0.0]),
        Range::default(),
    );
    assert_eq!((nowhere.left, nowhere.right), (0.0, 0.0));
    let broken = Range {
        near: f32::NAN,
        far: f32::NAN,
    };
    let heard = hear(&listener(), Place::At([1.0, 1.6, 0.0]), broken);
    assert!(heard.left.is_finite() && heard.right.is_finite());
    let spun = Listener {
        yaw: f32::NAN,
        ..listener()
    };
    let heard = hear(&spun, Place::At([1.0, 1.6, 0.0]), Range::default());
    assert!(heard.left.is_finite() && heard.right > 0.0);
}

// ---- The mixer ----

#[test]
fn nothing_playing_is_silence() {
    let mut mixer = Mixer::new(RATE);
    assert!(mix(&mut mixer, 64).iter().all(|sample| *sample == 0.0));
    // An odd-length buffer is filled to the end too.
    let mut odd = [9.0; 7];
    mixer.mix(&mut odd);
    assert_eq!(odd, [0.0; 7]);
}

#[test]
fn a_sound_plays_once_and_frees_its_voice() {
    let mut mixer = Mixer::new(RATE);
    let playing = mixer.play(Play::everywhere(steady(100, 0.5)).volume(0.5));
    assert!(mixer.is_playing(playing));
    let out = mix(&mut mixer, 200);
    assert!(out[..200].iter().all(|sample| close(*sample, 0.25)));
    assert!(out[200..].iter().all(|sample| *sample == 0.0));
    assert!(!mixer.is_playing(playing));
    assert_eq!(mixer.playing(), 0);
    // The handle is harmless afterwards.
    mixer.stop(playing);
    mixer.move_to(playing, [1.0, 2.0, 3.0]);
}

#[test]
fn a_placed_sound_reaches_the_right_ear() {
    let mut mixer = Mixer::new(RATE);
    mixer.set_listener(listener());
    mixer.play(Play::at(steady(100, 0.5), [2.0, 1.6, 0.0]));
    let out = mix(&mut mixer, 50);
    assert!(out[61] > 0.2 && out[60] < 0.001);
}

#[test]
fn a_stereo_sound_keeps_its_sides_unless_placed() {
    let stereo = Sound::from_samples(RATE, 2, [0.5, -0.25].repeat(100)).unwrap();
    let mut mixer = Mixer::new(RATE);
    mixer.set_listener(listener());
    mixer.play(Play::everywhere(stereo.clone()));
    let out = mix(&mut mixer, 10);
    assert!(close(out[0], 0.5) && close(out[1], -0.25));

    let mut mixer = Mixer::new(RATE);
    mixer.set_listener(listener());
    mixer.play(Play::at(stereo, [0.0, 1.6, -0.5]));
    let out = mix(&mut mixer, 10);
    // Folded to mono (0.125), then shared equally between the ears.
    assert!(close(out[0], out[1]) && out[0] > 0.05);
}

#[test]
fn a_looping_sound_goes_on_until_stopped() {
    let mut mixer = Mixer::new(RATE);
    let hum = mixer.play(Play::everywhere(steady(64, 0.5)).looping());
    let out = mix(&mut mixer, 1000);
    assert!(out.iter().all(|sample| close(*sample, 0.5)));

    mixer.stop(hum);
    assert!(!mixer.is_playing(hum));
    let out = mix(&mut mixer, 1000);
    // It fades over five thousandths of a second: no click, then silence.
    assert!(biggest_jump(&out) < 0.01);
    assert!(out[0] > 0.4);
    assert!(out[600..].iter().all(|sample| *sample == 0.0));
    assert_eq!(mixer.playing(), 0);
}

#[test]
fn the_mix_does_not_depend_on_buffer_size() {
    let scene = |mixer: &mut Mixer| {
        mixer.set_listener(listener());
        let mut gunfire = starter::gunfire();
        let mut steps = starter::footsteps();
        mixer.play(gunfire.at([6.0, 1.4, 3.0], 1).range(Range::GUNFIRE));
        mixer.play(steps.at([-2.0, 0.0, -1.0], 2).pitch(1.3).after(0.004));
        mixer.play(Play::everywhere(starter::reload()));
    };
    let mut whole = Mixer::new(44_100);
    scene(&mut whole);
    let expected = mix(&mut whole, 4096);

    let mut pieces = Mixer::new(44_100);
    scene(&mut pieces);
    let mut got = Vec::new();
    for frames in [1, 255, 256, 1024, 7, 2553] {
        got.extend(mix(&mut pieces, frames));
    }
    assert_eq!(got, expected);
}

#[test]
fn pitch_and_sample_rate_change_how_long_a_sound_lasts() {
    let lasts = |play: Play| {
        let mut mixer = Mixer::new(RATE);
        mixer.play(play);
        let out = mix(&mut mixer, 1000);
        out.as_chunks::<2>()
            .0
            .iter()
            .take_while(|frame| frame[0] != 0.0)
            .count()
    };
    assert_eq!(lasts(Play::everywhere(steady(200, 0.5))), 200);
    assert_eq!(lasts(Play::everywhere(steady(200, 0.5)).pitch(2.0)), 100);
    assert_eq!(lasts(Play::everywhere(steady(200, 0.5)).pitch(0.5)), 400);
    // Pitch is kept within two octaves either way, and nonsense is ignored.
    assert_eq!(lasts(Play::everywhere(steady(200, 0.5)).pitch(100.0)), 50);
    assert_eq!(
        lasts(Play::everywhere(steady(200, 0.5)).pitch(f32::NAN)),
        200
    );

    let slow = Sound::from_samples(24_000, 1, vec![0.5; 200]).unwrap();
    assert_eq!(lasts(Play::everywhere(slow)), 400);
}

#[test]
fn a_delayed_sound_starts_later() {
    let mut mixer = Mixer::new(RATE);
    mixer.play(Play::everywhere(steady(10, 0.5)).after(0.001));
    let out = mix(&mut mixer, 100);
    assert!(out[..96].iter().all(|sample| *sample == 0.0));
    assert!(close(out[96], 0.5));
    assert_eq!(mixer.playing(), 0);
}

#[test]
fn a_sound_out_of_earshot_is_not_started() {
    let mut mixer = Mixer::new(RATE);
    mixer.set_listener(listener());
    let far = mixer.play(Play::at(steady(100, 0.5), [0.0, 0.0, -400.0]));
    assert!(!mixer.is_playing(far));
    assert_eq!(mixer.playing(), 0);
    // A loop is: the player may walk up to it.
    let engine = mixer.play(Play::at(steady(100, 0.5), [0.0, 0.0, -400.0]).looping());
    assert!(mixer.is_playing(engine));
}

#[test]
fn moving_a_sound_changes_which_ear_hears_it() {
    let mut mixer = Mixer::new(RATE);
    mixer.set_listener(listener());
    let rocket = mixer.play(Play::at(steady(64, 0.5), [3.0, 1.6, 0.0]).looping());
    let out = mix(&mut mixer, 600);
    assert!(out[1] > out[0]);
    mixer.move_to(rocket, [-3.0, 1.6, 0.0]);
    let out = mix(&mut mixer, 600);
    // It glides across instead of jumping.
    assert!(biggest_jump(&out) < 0.01);
    assert!(out[1198] > out[1199] * 100.0);
}

#[test]
fn the_most_important_sounds_win_when_voices_run_out() {
    let mut mixer = Mixer::with_voices(RATE, 4);
    let quiet: Vec<Playing> = (0..4)
        .map(|_| {
            mixer.play(
                Play::everywhere(steady(64, 0.1))
                    .looping()
                    .importance(Importance::Low),
            )
        })
        .collect();
    assert_eq!(mixer.playing(), 4);

    // Something more important takes the oldest of them.
    let shot = mixer.play(
        Play::everywhere(steady(64, 0.1))
            .looping()
            .importance(Importance::High),
    );
    assert!(mixer.is_playing(shot));
    assert!(!mixer.is_playing(quiet[0]));
    assert!(mixer.is_playing(quiet[1]));
    assert_eq!(mixer.playing(), 4);

    // Fill up with important sounds: now a lesser one is turned away.
    let important: Vec<Playing> = (0..3)
        .map(|_| {
            mixer.play(
                Play::everywhere(steady(64, 0.1))
                    .looping()
                    .importance(Importance::High),
            )
        })
        .collect();
    assert!(important.iter().all(|playing| mixer.is_playing(*playing)));
    let lesser = mixer.play(Play::everywhere(steady(64, 0.1)).looping());
    assert!(!mixer.is_playing(lesser));
    assert!(mixer.is_playing(shot));
}

#[test]
fn among_equals_the_quietest_gives_way() {
    let mut mixer = Mixer::with_voices(RATE, 2);
    mixer.set_listener(listener());
    let near = mixer.play(Play::at(steady(64, 0.1), [0.0, 1.6, -2.0]).looping());
    let far = mixer.play(Play::at(steady(64, 0.1), [0.0, 1.6, -20.0]).looping());

    // Farther still: not worth cutting anything for.
    let farther = mixer.play(Play::at(steady(64, 0.1), [0.0, 1.6, -40.0]).looping());
    assert!(!mixer.is_playing(farther));

    // In between: the far one goes.
    let middle = mixer.play(Play::at(steady(64, 0.1), [0.0, 1.6, -5.0]).looping());
    assert!(mixer.is_playing(middle) && mixer.is_playing(near));
    assert!(!mixer.is_playing(far));
}

#[test]
fn cutting_a_sound_off_to_make_room_does_not_click() {
    let mut mixer = Mixer::with_voices(RATE, 1);
    mixer.play(Play::everywhere(steady(64, 0.5)).looping());
    let before = mix(&mut mixer, 300);
    // A silent sound takes the only voice: all that is left is the fade.
    mixer.play(
        Play::everywhere(steady(64, 0.0))
            .looping()
            .importance(Importance::High),
    );
    let after = mix(&mut mixer, 600);
    let mut both = before;
    both.extend(after);
    assert!(biggest_jump(&both) < 0.01, "{}", biggest_jump(&both));
    assert!(both[600] > 0.4);
    assert!(both[1400..].iter().all(|sample| *sample == 0.0));
}

#[test]
fn the_mix_never_leaves_the_range_a_sound_card_takes() {
    let mut mixer = Mixer::new(RATE);
    for _ in 0..Mixer::VOICES {
        mixer.play(Play::everywhere(steady(64, 1.0)).volume(5.0).looping());
    }
    let out = mix(&mut mixer, 500);
    assert!(out.iter().all(|sample| (0.99..=1.0).contains(sample)));
    // A lone quiet sound is left exactly as it is.
    let mut mixer = Mixer::new(RATE);
    mixer.play(Play::everywhere(steady(64, 0.75)));
    assert_eq!(mix(&mut mixer, 4)[0], 0.75);
}

#[test]
fn the_volume_setting_fades_everything() {
    let mut mixer = Mixer::new(RATE);
    mixer.play(Play::everywhere(steady(64, 0.5)).looping());
    mixer.set_volume(0.0);
    let out = mix(&mut mixer, 600);
    assert!(biggest_jump(&out) < 0.01);
    assert!(out[600..].iter().all(|sample| *sample == 0.0));
    mixer.set_volume(0.5);
    let out = mix(&mut mixer, 600);
    assert!(close(out[1198], 0.25));
}

#[test]
fn stop_all_ends_everything() {
    let mut mixer = Mixer::new(RATE);
    for _ in 0..5 {
        mixer.play(Play::everywhere(steady(64, 0.1)).looping());
    }
    mixer.stop_all();
    mix(&mut mixer, 600);
    assert_eq!(mixer.playing(), 0);
}

#[test]
fn a_sound_behind_loses_its_high_notes() {
    // The fastest wobble a sound can hold: +, -, +, -.
    let hiss = Sound::from_samples(RATE, 1, [0.5, -0.5].repeat(500)).unwrap();
    let loudest = |position: [f32; 3]| {
        let mut mixer = Mixer::new(RATE);
        mixer.set_listener(listener());
        mixer.play(Play::at(hiss.clone(), position));
        mix(&mut mixer, 500)[200..]
            .iter()
            .fold(0.0_f32, |a, b| a.max(b.abs()))
    };
    let ahead = loudest([0.0, 1.6, -1.0]);
    let behind = loudest([0.0, 1.6, 1.0]);
    assert!(behind < ahead * 0.3, "{ahead} {behind}");
}

// ---- Under water ----

/// One turn of a note of `pitch` hertz at half volume, to play looping.
/// `pitch` must go into 48,000 a whole number of times.
fn note(pitch: u32) -> Sound {
    let turn = RATE / pitch;
    let samples = (0..turn).map(|at| 0.5 * (TAU * at as f32 / turn as f32).sin());
    Sound::from_samples(RATE, 1, samples.collect()).unwrap()
}

/// How loud a mix is on the whole: the root of its mean square.
fn level(out: &[f32]) -> f32 {
    (out.iter().map(|sample| sample * sample).sum::<f32>() / out.len() as f32).sqrt()
}

/// How many decibels quieter `play` is this far under water than in air,
/// once the change has been made.
fn lost_under_water(play: &Play, amount: f32) -> f32 {
    let heard = |amount: f32| {
        let mut mixer = Mixer::new(RATE);
        mixer.play(play.clone().looping());
        mixer.set_underwater(amount);
        mix(&mut mixer, RATE as usize / 2);
        level(&mix(&mut mixer, RATE as usize / 2))
    };
    20.0 * (heard(0.0) / heard(amount)).log10()
}

#[test]
fn under_water_the_high_notes_go_and_the_low_notes_stay() {
    let low = lost_under_water(&Play::everywhere(note(100)), 1.0);
    let high = lost_under_water(&Play::everywhere(note(4_000)), 1.0);
    // Everything is a little quieter, and no more than that down low.
    assert!((2.0..3.0).contains(&low), "a 100 Hz note lost {low} dB");
    assert!(high > 30.0, "a 4 kHz note lost {high} dB");
    // Steeper than the dulling of a sound behind the listener: an octave
    // further up loses twice as many decibels again.
    let higher = lost_under_water(&Play::everywhere(note(8_000)), 1.0);
    assert!(higher > high + 10.0, "{high} then {higher}");

    // Half way under is between the two.
    let half = lost_under_water(&Play::everywhere(note(4_000)), 0.5);
    assert!((3.0..10.0).contains(&half), "half under lost {half} dB");
}

#[test]
fn under_water_music_and_placed_sounds_are_muffled_too() {
    let music = lost_under_water(&Play::everywhere(note(4_000)).music(), 1.0);
    assert!(music > 30.0, "music lost {music} dB");
    let mut placed = Play::at(note(4_000), [1.0, 0.0, 0.0]);
    placed.range = Range::GUNFIRE;
    let placed = lost_under_water(&placed, 1.0);
    assert!(placed > 30.0, "a sound at a place lost {placed} dB");
}

#[test]
fn going_under_takes_a_tenth_of_a_second_and_is_a_little_quieter() {
    let mut mixer = Mixer::new(RATE);
    mixer.play(Play::everywhere(steady(64, 0.5)).looping());
    mix(&mut mixer, 1_000);
    mixer.set_underwater(1.0);
    let out = mix(&mut mixer, 9_600);
    // A steady push is as low as a sound gets, so only the loudness goes:
    // half way there after a twentieth of a second, and there after a
    // tenth.
    let (air, under) = (0.5, 0.5 * 0.72);
    assert!((out[0] - air).abs() < 0.001, "{}", out[0]);
    assert!((out[2_400 * 2] - (air + under) / 2.0).abs() < 0.005);
    assert!(close(out[4_900 * 2], under), "{}", out[4_900 * 2]);
    assert!(close(out[9_599 * 2], under));

    // And back, and to anywhere between. Nonsense is taken as air.
    mixer.set_underwater(0.5);
    let out = mix(&mut mixer, 4_800);
    assert!(close(out[4_799 * 2], (air + under) / 2.0));
    for nonsense in [f32::NAN, -3.0, f32::NEG_INFINITY] {
        mixer.set_underwater(nonsense);
        let out = mix(&mut mixer, 4_800);
        assert_eq!(out[4_799 * 2], air);
    }
    mixer.set_underwater(7.0);
    assert!(close(mix(&mut mixer, 9_600)[9_599 * 2], under));
}

#[test]
fn under_water_it_is_harder_to_tell_which_side_a_sound_is_on() {
    // All of it in the left ear.
    let one_sided = Sound::from_samples(RATE, 2, [0.5, 0.0].repeat(64)).unwrap();
    let mut mixer = Mixer::new(RATE);
    mixer.play(Play::everywhere(one_sided).looping());
    let air = mix(&mut mixer, 1_000);
    assert_eq!((air[1_998], air[1_999]), (0.5, 0.0));
    mixer.set_underwater(1.0);
    let under = mix(&mut mixer, 9_600);
    let (left, right) = (under[19_198], under[19_199]);
    // Half of the difference between the ears is left, and still the
    // right way round.
    assert!(close(left, 0.375 * 0.72) && close(right, 0.125 * 0.72));
}

#[test]
fn going_under_and_coming_up_never_click() {
    let mut mixer = Mixer::new(RATE);
    mixer.set_listener(listener());
    mixer.play(Play::everywhere(note(200)).looping());
    mixer.play(Play::at(note(1_000), [2.0, 1.6, 0.0]).looping());
    let mut heard = mix(&mut mixer, 4_800);
    // The biggest step from one moment to the next that the sounds make
    // by themselves in air: no change may make a bigger one.
    let usual = biggest_jump(&heard[4_800..]);
    // Under, up again before it is all the way under, part of the way,
    // and up.
    for (amount, frames) in [
        (1.0, 9_600),
        (0.0, 1_700),
        (1.0, 300),
        (0.4, 9_600),
        (0.0, 9_600),
    ] {
        mixer.set_underwater(amount);
        heard.extend(mix(&mut mixer, frames));
    }
    let changing = biggest_jump(&heard[4_800..]);
    assert!(changing <= usual * 1.01, "{usual} in air, {changing}");
    // It did go under, all the same.
    assert!(level(&heard[2 * 12_000..2 * 14_000]) < level(&heard[..9_600]) * 0.8);
}

#[test]
fn in_air_the_mix_is_exactly_what_it_was() {
    let scene = || {
        let mut mixer = Mixer::new(RATE);
        mixer.set_listener(listener());
        let stereo = Sound::from_samples(RATE, 2, [0.3, -0.2, 0.1, 0.25].repeat(100)).unwrap();
        mixer.play(Play::everywhere(stereo).music().looping());
        mixer.play(Play::at(note(480), [-3.0, 1.6, 2.0]).looping());
        mixer.play(starter::gunfire().at([4.0, 1.0, -6.0], 3));
        mixer
    };
    let bits = |out: Vec<f32>| -> Vec<u32> { out.into_iter().map(f32::to_bits).collect() };

    // Never told about water, and told there is none: the same numbers,
    // to the last bit.
    let mut plain = scene();
    let mut told = scene();
    told.set_underwater(0.0);
    let plain = bits(mix(&mut plain, 30_000));
    assert_eq!(bits(mix(&mut told, 30_000)), plain);

    // Under and up again: once it is up, the same numbers again.
    let mut dived = scene();
    dived.set_underwater(1.0);
    let under = bits(mix(&mut dived, 10_000));
    assert_ne!(under[..], plain[..20_000]);
    dived.set_underwater(0.0);
    mix(&mut dived, 10_000);
    assert_eq!(bits(mix(&mut dived, 10_000)), plain[40_000..]);
}

#[test]
fn going_under_does_not_depend_on_buffer_size() {
    let run = |frames: usize| {
        let mut mixer = Mixer::new(RATE);
        mixer.set_listener(listener());
        mixer.play(Play::at(note(480), [-3.0, 1.6, 2.0]).looping());
        let mut heard = Vec::new();
        // Under for long enough to be all the way, up for not quite long
        // enough, and under again.
        for (amount, moments) in [(1.0, 6_000), (0.0, 3_000), (0.6, 3_000), (0.0, 6_000)] {
            mixer.set_underwater(amount);
            for _ in 0..moments / frames {
                heard.extend(mix(&mut mixer, frames));
            }
        }
        heard
    };
    assert_eq!(run(3_000), run(50));
    assert_eq!(run(1_000), run(8));
}

#[test]
fn silence_under_water_is_silence() {
    let mut mixer = Mixer::new(RATE);
    mixer.set_underwater(1.0);
    assert!(mix(&mut mixer, 6_000).iter().all(|sample| *sample == 0.0));
    // And a sound that has ended leaves nothing behind for long.
    mixer.play(Play::everywhere(steady(64, 0.5)));
    mix(&mut mixer, RATE as usize);
    assert!(mix(&mut mixer, 600).iter().all(|sample| *sample == 0.0));
}

// ---- Sound sets ----

#[test]
fn a_sound_set_needs_a_sound() {
    assert_eq!(SoundSet::new(Vec::new()).unwrap_err(), AudioError::NoSounds);
}

#[test]
fn a_sound_set_never_plays_the_same_take_twice_running() {
    let takes: Vec<Sound> = (1..=4).map(|take| steady(8, take as f32 * 0.1)).collect();
    let mut set = SoundSet::new(takes).unwrap();
    let mut last = -1.0;
    let mut used = [false; 4];
    for seed in 0..1000 {
        let play = set.everywhere(seed);
        let level = play.sound.data()[0];
        assert_ne!(level, last, "seed {seed}");
        last = level;
        used[(level * 10.0).round() as usize - 1] = true;
        assert!((0.94..=1.06).contains(&play.pitch));
        assert!((0.9..=1.0).contains(&play.volume));
    }
    assert_eq!(used, [true; 4]);
}

#[test]
fn the_same_seed_gives_the_same_take() {
    let takes: Vec<Sound> = (1..=4).map(|take| steady(8, take as f32 * 0.1)).collect();
    let set = SoundSet::new(takes).unwrap();
    let (mut here, mut there) = (set.clone(), set);
    for seed in [7, 7, 99, 12_345, 0] {
        let (a, b) = (
            here.at([1.0, 0.0, 0.0], seed),
            there.at([1.0, 0.0, 0.0], seed),
        );
        assert_eq!(a.sound.data(), b.sound.data());
        assert_eq!((a.pitch, a.volume, a.place), (b.pitch, b.volume, b.place));
    }
}

#[test]
fn a_set_of_one_and_a_wild_spread_still_work() {
    let mut set = SoundSet::new(vec![steady(8, 0.5)])
        .unwrap()
        .spread(f32::NAN, 40.0);
    for seed in 0..50 {
        let play = set.everywhere(seed);
        assert_eq!(play.pitch, 1.0);
        assert!((0.5..=1.0).contains(&play.volume));
    }
}

// ---- Footsteps ----

fn walking(speed: f32) -> Feet {
    Feet {
        speed,
        grounded: true,
        ..Feet::default()
    }
}

/// Steps taken over `seconds` at `ticks` a second.
fn count_steps(feet: &Feet, seconds: f32, ticks: u32) -> Vec<Footfall> {
    let settings = FootstepSettings::default();
    let mut footsteps = Footsteps::default();
    (0..(seconds * ticks as f32) as u32)
        .filter_map(|_| footsteps.step(&settings, feet, 1.0 / ticks as f32))
        .collect()
}

#[test]
fn steps_come_by_distance_covered() {
    // 44.5 metres at a step every 2, the first a metre in.
    assert_eq!(count_steps(&walking(4.5), 9.9, 60).len(), 22);
    // Sprinting: 64 metres at a step every 2.2, the first 1.1 in.
    let sprint = Feet {
        sprinting: true,
        ..walking(6.5)
    };
    assert_eq!(count_steps(&sprint, 9.9, 60).len(), 29);
    assert!(count_steps(&walking(0.0), 10.0, 60).is_empty());
    assert!(count_steps(&walking(0.3), 10.0, 60).is_empty());
}

#[test]
fn the_tick_rate_does_not_change_the_steps() {
    for ticks in [30, 60, 144, 1000] {
        let steps = count_steps(&walking(4.5), 9.9, ticks).len();
        assert_eq!(steps, 22, "at {ticks} ticks a second");
    }
}

#[test]
fn feet_take_turns_and_crouching_is_quiet() {
    let steps = count_steps(&walking(4.5), 5.0, 60);
    let sides: Vec<bool> = steps
        .iter()
        .map(|step| matches!(step, Footfall::Step { left: true, .. }))
        .collect();
    assert!(sides.windows(2).all(|pair| pair[0] != pair[1]));

    let volume = |feet: &Feet| match count_steps(feet, 5.0, 60)[0] {
        Footfall::Step { volume, .. } => volume,
        Footfall::Landing { .. } => panic!("no one jumped"),
    };
    let crouch = Feet {
        crouched: true,
        ..walking(2.0)
    };
    let sprint = Feet {
        sprinting: true,
        ..walking(6.5)
    };
    assert!(volume(&crouch) < volume(&walking(4.5)));
    assert!(volume(&walking(4.5)) < volume(&sprint));
}

#[test]
fn the_first_step_comes_early_each_time_the_player_sets_off() {
    let settings = FootstepSettings::default();
    let mut footsteps = Footsteps::default();
    let dt = 1.0 / 60.0;
    let first = |footsteps: &mut Footsteps| {
        (1..).find(|_| footsteps.step(&settings, &walking(4.5), dt).is_some())
    };
    // A metre at 4.5 metres a second: 14 ticks.
    assert_eq!(first(&mut footsteps), Some(14));
    for _ in 0..30 {
        assert_eq!(footsteps.step(&settings, &walking(0.0), dt), None);
    }
    assert_eq!(first(&mut footsteps), Some(14));
}

#[test]
fn landing_thumps_harder_after_a_longer_fall() {
    let settings = FootstepSettings::default();
    let dt = 1.0 / 60.0;
    let land_after = |ticks: u32, falling: f32| {
        let mut footsteps = Footsteps::default();
        footsteps.step(&settings, &walking(4.5), dt);
        let air = Feet {
            speed: 4.5,
            falling,
            ..Feet::default()
        };
        for _ in 0..ticks {
            assert_eq!(footsteps.step(&settings, &air, dt), None);
        }
        footsteps.step(&settings, &walking(4.5), dt)
    };
    // Stepping off a kerb: three ticks in the air.
    assert_eq!(land_after(3, 1.0), None);
    let Some(Footfall::Landing { volume: hop }) = land_after(30, 4.0) else {
        panic!("a jump should land");
    };
    let Some(Footfall::Landing { volume: drop }) = land_after(60, 14.0) else {
        panic!("a fall should land");
    };
    assert!(hop < drop);
    assert_eq!(drop, 1.0);
}

#[test]
fn footsteps_survive_nonsense() {
    let settings = FootstepSettings {
        walk_stride: f32::NAN,
        slowest: f32::NAN,
        ..FootstepSettings::default()
    };
    let mut footsteps = Footsteps::default();
    for speed in [f32::NAN, f32::INFINITY, -3.0, 4.5] {
        for dt in [f32::NAN, -1.0, 0.0, 1.0e9] {
            footsteps.step(&settings, &walking(speed), dt);
        }
    }
    // It still works afterwards, and a huge tick owes one step, not a drum roll.
    let settings = FootstepSettings::default();
    footsteps.step(&settings, &walking(4.5), 100.0);
    let steps = (0..60)
        .filter_map(|_| footsteps.step(&settings, &walking(4.5), 1.0 / 60.0))
        .count();
    assert!(steps <= 3);
}

// ---- Starter sounds ----

#[test]
fn starter_sounds_are_audible_tidy_and_always_the_same() {
    let mut steps = starter::footsteps();
    let mut gunfire = starter::gunfire();
    let sounds = [
        steps.everywhere(0).sound,
        gunfire.everywhere(0).sound,
        starter::landing(),
        starter::dry_fire(),
        starter::reload(),
    ];
    for sound in &sounds {
        let data = sound.data();
        let loudest = data.iter().fold(0.0_f32, |a, b| a.max(b.abs()));
        assert!(
            (0.2..=1.0).contains(&loudest),
            "{sound:?} peaks at {loudest}"
        );
        // They start and end near silence, so they never click.
        assert!(data[0].abs() < 0.05 && data[data.len() - 1].abs() < 0.01);
        assert_eq!(sound.channels(), 1);
    }
    assert_eq!(starter::reload().data(), sounds[4].data());
    assert_eq!(
        starter::gunfire().everywhere(0).sound.data(),
        sounds[1].data()
    );
}

#[test]
fn starter_takes_differ_from_each_other() {
    let mut gunfire = starter::gunfire();
    let first = gunfire.everywhere(0).sound;
    let second = gunfire.everywhere(0).sound;
    assert_ne!(first.data(), second.data());
}

// ---- Ogg Vorbis and streamed sounds ----

// The two files were made with ffmpeg from sine waves, so they are nobody's
// music:
//   tone.ogg: a quarter of a second, mono, 48 kHz, 440 Hz at half volume.
//     ffmpeg -f lavfi -i "sine=frequency=440:sample_rate=48000:duration=0.25"
//       -af volume=4 -ac 1 -c:a libvorbis -q:a 0 -map_metadata -1 -bitexact tone.ogg
//   song.ogg: eleven seconds, stereo, 8 kHz, 220 Hz at half volume on the
//   left and 330 Hz at a quarter on the right.
//     ffmpeg -f lavfi -i "sine=frequency=220:sample_rate=8000:duration=11"
//       -f lavfi -i "sine=frequency=330:sample_rate=8000:duration=11"
//       -filter_complex "[0:a]volume=4[l];[1:a]volume=2[r];[l][r]amerge=inputs=2"
//       -c:a libvorbis -q:a 0 -map_metadata -1 -bitexact song.ogg
const TONE: &[u8] = include_bytes!("../tests/data/tone.ogg");
const SONG: &[u8] = include_bytes!("../tests/data/song.ogg");

/// The loudest sample in one ear (0 left, 1 right) of a mixed buffer.
fn loudest(out: &[f32], ear: usize) -> f32 {
    out.iter()
        .skip(ear)
        .step_by(2)
        .fold(0.0, |loudest, sample| loudest.max(sample.abs()))
}

/// Mixes `frames` moments a hundredth of a second at a time, decoding
/// before each as the decoding thread would have.
fn mix_fed(mixer: &mut Mixer, feeder: &mut stream::Feeder, frames: usize) -> Vec<f32> {
    let mut out = vec![9.0; frames * 2];
    for part in out.chunks_mut(960) {
        feeder.fill();
        mixer.mix(part);
    }
    out
}

#[test]
fn an_ogg_vorbis_file_loads_as_a_sound() {
    let tone = Sound::from_ogg(TONE).unwrap();
    assert_eq!((tone.rate(), tone.channels()), (RATE, 1));
    // To the sample: the padding an encoder adds at the end is cut off.
    assert_eq!(tone.frames(), 12_000);
    assert!(!tone.is_streamed());
    let peak = tone
        .data()
        .iter()
        .fold(0.0_f32, |peak, s| peak.max(s.abs()));
    assert!((peak - 0.5).abs() < 0.05, "{peak}");

    // The same file whatever it is asked as, since it is short.
    let found = Sound::from_bytes(TONE).unwrap();
    assert!(!found.is_streamed());
    assert_eq!(found.data(), tone.data());
}

#[test]
fn a_long_ogg_vorbis_file_is_streamed_and_a_short_one_or_a_wav_is_not() {
    let song = Sound::from_bytes(SONG).unwrap();
    assert!(song.is_streamed());
    assert_eq!((song.rate(), song.channels()), (8_000, 2));
    assert!(close(song.seconds(), 11.0));
    // It holds the file, not the 700 KB of sound in it.
    assert!(song.data().is_empty());
    assert_eq!(song.packed().len(), SONG.len());

    // Decoded whole, it is as long as it said it would be.
    assert_eq!(Sound::from_ogg(SONG).unwrap().frames(), song.frames());

    let spec = hound::WavSpec {
        channels: 1,
        sample_rate: RATE,
        bits_per_sample: 16,
        sample_format: hound::SampleFormat::Int,
    };
    let bytes = wav(spec, |writer| {
        for _ in 0..RATE * 11 {
            writer.write_sample(8_000_i16).unwrap();
        }
    });
    assert!(!Sound::from_bytes(bytes.clone()).unwrap().is_streamed());
    assert!(!Sound::streamed(bytes).unwrap().is_streamed());
}

#[test]
fn files_that_are_not_sounds_are_refused_with_advice() {
    assert_eq!(
        Sound::from_bytes(&b"ID3 an MP3, perhaps"[..]).unwrap_err(),
        AudioError::UnknownFormat
    );
    assert!(AudioError::UnknownFormat.to_string().contains("Ogg Vorbis"));

    // An Ogg page that starts an Opus stream.
    let mut opus = b"OggS".to_vec();
    opus.extend_from_slice(&[0; 22]);
    opus.extend_from_slice(&[1, 19]);
    opus.extend_from_slice(b"OpusHead\x01\x02\0\0\x80\xbb\0\0\0\0\0");
    for read in [
        Sound::from_bytes(opus.clone()),
        Sound::streamed(opus.clone()),
    ] {
        let error = read.unwrap_err().to_string();
        assert!(error.contains("it holds Opus sound, not Vorbis"), "{error}");
        assert!(error.contains("Export the sound as Ogg Vorbis"), "{error}");
    }
    assert!(matches!(
        Sound::from_ogg(&opus).unwrap_err(),
        AudioError::NotOgg { .. }
    ));

    // Cut off inside its headers.
    for read in [
        Sound::from_bytes(&SONG[..600]),
        Sound::from_ogg(&SONG[..600]),
    ] {
        assert!(matches!(read.unwrap_err(), AudioError::NotOgg { .. }));
    }
    assert!(matches!(
        Sound::from_ogg(b"RIFF....WAVE").unwrap_err(),
        AudioError::NotOgg { .. }
    ));
}

#[test]
fn a_streamed_sound_sounds_the_same_as_the_sound_in_memory() {
    let mut whole = Mixer::new(RATE);
    whole.play(Play::everywhere(Sound::from_ogg(SONG).unwrap()));
    let mut streamed = Mixer::new(RATE);
    let song = Sound::streamed(SONG).unwrap();
    let (_, mut feeder) = streamed.play_fed_by_hand(Play::everywhere(song));

    // Twelve seconds: past the second that is decoded ahead, through the
    // change of rate from 8 kHz, and past the end.
    let frames = RATE as usize * 12;
    let expected = mix(&mut whole, frames);
    let heard = mix_fed(&mut streamed, &mut feeder, frames);
    let apart = expected
        .iter()
        .zip(&heard)
        .fold(0.0_f32, |apart, (a, b)| apart.max((a - b).abs()));
    let at = expected
        .iter()
        .zip(&heard)
        .position(|(a, b)| (a - b).abs() > 1.0e-4);
    assert!(apart < 1.0e-4, "{apart} {at:?}");
    // Half volume on the left, a quarter on the right.
    assert!((loudest(&heard, 0) - 0.5).abs() < 0.05);
    assert!((loudest(&heard, 1) - 0.25).abs() < 0.05);
    assert_eq!((whole.playing(), streamed.playing()), (0, 0));
    assert_eq!(heard[heard.len() - 2..], [0.0, 0.0]);
    assert_eq!(feeder.fill(), stream::Fed::Done);
}

#[test]
fn a_streamed_sound_loops_until_it_is_stopped() {
    let tone = Sound::streamed(TONE).unwrap();
    assert!(tone.is_streamed());
    let mut mixer = Mixer::new(RATE);
    let (once, mut feeder) = mixer.play_fed_by_hand(Play::everywhere(tone.clone()));
    let out = mix_fed(&mut mixer, &mut feeder, RATE as usize);
    assert!(loudest(&out[..24_000], 0) > 0.4);
    assert_eq!(
        loudest(&out[24_002..], 0),
        0.0,
        "it is a quarter second long"
    );
    assert!(!mixer.is_playing(once));

    let (looped, mut feeder) = mixer.play_fed_by_hand(Play::everywhere(tone).looping());
    let out = mix_fed(&mut mixer, &mut feeder, RATE as usize * 3);
    for part in out.chunks(RATE as usize / 2) {
        assert!(loudest(part, 0) > 0.4, "still going");
    }
    // Where it goes round, nothing is left out or put in: it is the same
    // as the sound in memory going round.
    let mut whole = Mixer::new(RATE);
    whole.play(Play::everywhere(Sound::from_ogg(TONE).unwrap()).looping());
    let expected = mix(&mut whole, RATE as usize * 3);
    let apart = expected
        .iter()
        .zip(&out)
        .fold(0.0_f32, |apart, (a, b)| apart.max((a - b).abs()));
    assert!(apart < 1.0e-4, "{apart}");

    mixer.stop(looped);
    let out = mix_fed(&mut mixer, &mut feeder, 4_800);
    assert_eq!(out[out.len() - 2..], [0.0, 0.0]);
    assert_eq!(mixer.playing(), 0);
    // Its voice has let go of the stream, so the decoding stops.
    assert_eq!(feeder.fill(), stream::Fed::Done);
}

#[test]
fn a_streamed_sound_waits_for_its_decoding_and_never_holds_up_the_mix() {
    let mut mixer = Mixer::new(RATE);
    let song = Sound::streamed(SONG).unwrap();
    let (playing, mut feeder) = mixer.play_fed_by_hand(Play::everywhere(song.clone()));
    // Nothing decoded yet: silence, and the sound is still to come.
    assert_eq!(loudest(&mix(&mut mixer, 4_800), 0), 0.0);
    assert!(mixer.is_playing(playing));

    // One second is decoded ahead, and then no more: the voice plays that
    // and falls silent until there is more.
    assert_eq!(feeder.fill(), stream::Fed::Some);
    assert_eq!(feeder.fill(), stream::Fed::Full);
    let out = mix(&mut mixer, RATE as usize * 2);
    assert!(loudest(&out[..RATE as usize * 2], 0) > 0.4);
    assert_eq!(loudest(&out[RATE as usize * 2..], 0), 0.0);
    assert!(mixer.is_playing(playing));
    feeder.fill();
    assert!(loudest(&mix(&mut mixer, 4_800), 0) > 0.4);

    // A sound stopped while it waits ends at once.
    let (waiting, _unfed) = mixer.play_fed_by_hand(Play::everywhere(song));
    mix(&mut mixer, 480);
    mixer.stop(waiting);
    mixer.stop(playing);
    mix(&mut mixer, 480);
    assert_eq!(mixer.playing(), 0);
}

#[test]
fn a_streamed_file_that_is_cut_short_ends_early_and_quietly() {
    let cut = Sound::streamed(&SONG[..SONG.len() / 2]).unwrap();
    let mut mixer = Mixer::new(RATE);
    let (_, mut feeder) = mixer.play_fed_by_hand(Play::everywhere(cut).looping());
    let out = mix_fed(&mut mixer, &mut feeder, RATE as usize * 12);
    assert!(loudest(&out[..RATE as usize], 0) > 0.4);
    assert_eq!(mixer.playing(), 0);
    assert_eq!(out[out.len() - 2..], [0.0, 0.0]);
}

#[test]
fn a_streamed_stereo_sound_at_a_place_is_folded_to_mono_and_placed() {
    let mut mixer = Mixer::new(RATE);
    mixer.set_listener(listener());
    let song = Sound::streamed(SONG).unwrap();
    let (_, mut feeder) = mixer.play_fed_by_hand(Play::at(song, [3.0, 1.6, 0.0]));
    let out = mix_fed(&mut mixer, &mut feeder, RATE as usize);
    let (left, right) = (loudest(&out, 0), loudest(&out, 1));
    assert!(right > left * 3.0, "{left} {right}");
    assert!(left > 0.0);
}

#[test]
fn speech_has_a_volume_of_its_own() {
    let mut mixer = Mixer::new(RATE);
    // Speech in the left ear only and an effect in the right only.
    let left = Sound::from_samples(RATE, 2, [0.5, 0.0].repeat(RATE as usize)).unwrap();
    let right = Sound::from_samples(RATE, 2, [0.0, 0.5].repeat(RATE as usize)).unwrap();
    mixer.play(Play::everywhere(left).speech().looping());
    mixer.play(Play::everywhere(right).looping());
    let out = mix(&mut mixer, 4_800);
    assert!(close(loudest(&out, 0), 0.5) && close(loudest(&out, 1), 0.5));

    // Speech turned down leaves the effect alone.
    mixer.set_group_volume(Group::Speech, 0.5);
    let out = mix(&mut mixer, 4_800);
    assert!(close(out[out.len() - 2], 0.25) && close(out[out.len() - 1], 0.5));

    // Effects turned down leaves speech at its own volume.
    mixer.set_group_volume(Group::Speech, 1.0);
    mixer.set_group_volume(Group::Effects, 0.0);
    let out = mix(&mut mixer, 4_800);
    assert!(close(out[out.len() - 2], 0.5) && close(out[out.len() - 1], 0.0));
}

#[test]
fn music_has_a_volume_of_its_own() {
    let mut mixer = Mixer::new(RATE);
    // Music in the left ear only and an effect in the right only, so each
    // can be told from the other.
    let left = Sound::from_samples(RATE, 2, [0.5, 0.0].repeat(RATE as usize)).unwrap();
    let right = Sound::from_samples(RATE, 2, [0.0, 0.5].repeat(RATE as usize)).unwrap();
    mixer.play(Play::everywhere(left).music().looping());
    mixer.play(Play::everywhere(right).looping());
    let out = mix(&mut mixer, 4_800);
    assert!(close(loudest(&out, 0), 0.5) && close(loudest(&out, 1), 0.5));

    mixer.set_group_volume(Group::Music, 0.5);
    let out = mix(&mut mixer, 4_800);
    // No click: the music slides to its new volume.
    assert!(biggest_jump(&out) < 0.01, "{}", biggest_jump(&out));
    assert!(close(out[out.len() - 2], 0.25) && close(out[out.len() - 1], 0.5));

    mixer.set_group_volume(Group::Music, 0.0);
    mixer.set_group_volume(Group::Effects, 0.5);
    let out = mix(&mut mixer, 4_800);
    assert!(close(out[out.len() - 2], 0.0) && close(out[out.len() - 1], 0.25));

    // The volume of everything is on top of both.
    mixer.set_group_volume(Group::Music, 1.0);
    mixer.set_volume(0.5);
    let out = mix(&mut mixer, 4_800);
    assert!(close(out[out.len() - 2], 0.25) && close(out[out.len() - 1], 0.125));
    // A sound started now is at its group's volume from its first sample.
    mixer.stop_all();
    mix(&mut mixer, 4_800);
    mixer.set_volume(1.0);
    mix(&mut mixer, 4_800);
    let quiet = Sound::from_samples(RATE, 1, vec![0.5; 480]).unwrap();
    mixer.play(Play::everywhere(quiet));
    assert!(close(mix(&mut mixer, 8)[0], 0.25));
}

#[test]
fn a_streamed_sound_is_decoded_by_a_thread_of_its_own() {
    use std::time::{Duration, Instant};

    let mut mixer = Mixer::new(RATE);
    let song = Sound::from_bytes(SONG).unwrap();
    let playing = mixer.play(Play::everywhere(song).music().looping());
    // Nothing here decodes: the sound arrives when the thread has made it.
    let give_up = Instant::now() + Duration::from_secs(30);
    let mut heard = 0.0;
    while heard == 0.0 {
        assert!(Instant::now() < give_up, "the stream never started");
        std::thread::sleep(Duration::from_millis(2));
        heard = loudest(&mix(&mut mixer, 480), 0);
    }
    assert!(mixer.is_playing(playing));
    // It keeps coming: three seconds of it, which is three times what is
    // decoded ahead, however long a busy computer takes to make them.
    let mut heard = 0;
    while heard < 150 {
        assert!(Instant::now() < give_up, "the stream stopped coming");
        std::thread::sleep(Duration::from_millis(1));
        if loudest(&mix(&mut mixer, 960), 0) > 0.3 {
            heard += 1;
        }
    }
    mixer.stop(playing);
    mix(&mut mixer, 960);
    assert_eq!(mixer.playing(), 0);
}

#[test]
fn a_sound_of_the_world_waits_while_the_world_is_stopped_and_slows_with_it() {
    /// How many of the first `frames` of a mix are not silent.
    fn heard(mixer: &mut Mixer, frames: usize) -> usize {
        let mut out = vec![0.0; frames * 2];
        mixer.mix(&mut out);
        out.chunks(2).filter(|frame| frame[0].abs() > 1e-4).count()
    }
    // A tenth of a second of tone.
    let wave: Vec<f32> = (0..4_800u16)
        .map(|n| (f32::from(n) * 0.05).sin() * 0.5)
        .collect();
    let tone = Sound::from_samples(48_000, 1, wave).unwrap();

    // Stopped, a sound of the world is not heard and does not run on...
    let mut mixer = Mixer::new(48_000);
    mixer.set_world_speed(0.0);
    let steps = mixer.play(Play::everywhere(tone.clone()).in_world());
    assert_eq!(heard(&mut mixer, 9_600), 0, "the world's sound waits");
    // ...while a menu's click is heard, and ends, all the same.
    let click = mixer.play(Play::everywhere(tone.clone()));
    assert!(heard(&mut mixer, 9_600) > 4_000);
    assert!(mixer.is_playing(steps) && !mixer.is_playing(click));
    // Started again, it plays from where it was: all of it.
    mixer.set_world_speed(1.0);
    assert!(heard(&mut mixer, 9_600) > 4_000);
    assert!(!mixer.is_playing(steps));

    // At half speed it lasts twice as long.
    let mut mixer = Mixer::new(48_000);
    mixer.set_world_speed(0.5);
    let slow = mixer.play(Play::at(tone, [0.0; 3]));
    heard(&mut mixer, 7_200);
    assert!(mixer.is_playing(slow), "still going after 0.15 s");
    heard(&mut mixer, 4_800);
    assert!(!mixer.is_playing(slow), "done after 0.25 s");
    // Nonsense is the usual speed.
    mixer.set_world_speed(f32::NAN);
    let short = Sound::from_samples(48_000, 1, vec![0.5; 480]).unwrap();
    let usual = mixer.play(Play::everywhere(short).in_world());
    heard(&mut mixer, 2_400);
    assert!(!mixer.is_playing(usual));
}
