//! Animation clips, resampled once when loaded so playing one is cheap and exact.

use lore_assets::{Animation, Interpolation, Keys, Pose, Scene, Track};

use crate::math::{dot4, lerp3, nlerp, unit};
use crate::skeleton::Skeleton;

/// How many keys a second every clip is resampled to when loaded.
///
/// Thirty is what motion capture and most hand animation are keyed at, so
/// little is lost; between keys the player draws a straight line (and a
/// normalised one for turns), which at a thirtieth of a second apart cannot
/// be told from the curve.
pub const KEYS_PER_SECOND: f32 = 30.0;

/// Which of a bone's three parts a [`Channel`] moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Part {
    /// [`Pose::translation`].
    Translation,
    /// [`Pose::rotation`].
    Rotation,
    /// [`Pose::scale`].
    Scale,
}

/// One bone's translation, rotation or scale over a clip, at evenly spaced keys.
#[derive(Debug, Clone, PartialEq)]
pub struct Channel {
    /// The name of the bone it moves. Clips find their bones by name, so a
    /// clip made for one model plays on any model with the same bone names.
    pub bone: String,
    /// What it changes.
    pub part: Part,
    /// Holds each key until the next instead of moving between them (the
    /// file asked for stepped keys).
    pub held: bool,
    /// The value at each key; a 3-vector leaves the fourth number 0. One
    /// value when it never changes.
    pub values: Vec<[f32; 4]>,
}

/// A named moment in a clip, such as a foot touching down or a sword's hit.
#[derive(Debug, Clone, PartialEq)]
pub struct Marker {
    /// What happens, in the clip's own words (`"left_foot_down"`, `"hit"`).
    pub name: String,
    /// When, in seconds from the start of the clip.
    pub time: f32,
}

/// An animation clip ready to play: every part resampled to even keys.
#[derive(Debug, Clone, PartialEq)]
pub struct Clip {
    /// The clip's name (`"walk"`).
    pub name: String,
    /// How long it lasts, in seconds. Never negative.
    pub duration: f32,
    /// Whether it starts again when it ends: when the file says so
    /// (`{"loop":true}` in the clip's custom properties, which Blender
    /// exports), when its name ends in "loop", or when it is one of the
    /// moving-about clips that cycle ([`crate::names::LOOPING`]). A script
    /// may change it.
    pub looping: bool,
    /// The moments in it that scripts and the engine hear about.
    pub markers: Vec<Marker>,
    /// What it moves.
    pub channels: Vec<Channel>,
    /// How many keys each changing channel has: at least 2 when it lasts
    /// at all, 1 when it is a single pose.
    frames: u32,
    /// How high the hips stand above the model's origin at rest in the
    /// model the clip was made for, which scales the hips' travel when it
    /// plays on a taller or shorter body. 0 when unknown.
    pub hip_height: f32,
}

/// Bones whose travel a clip keeps when it plays on another body: the rest
/// of a body's shape stays its own.
const MOVING_BONES: [&str; 6] = ["root", "pelvis", "hips", "Hips", "Root", "mixamorig:Hips"];

/// The names hips are found by, to measure how tall a body stands.
const HIP_BONES: [&str; 4] = ["pelvis", "hips", "Hips", "mixamorig:Hips"];

impl Clip {
    /// Every clip in a loaded file.
    #[must_use]
    pub fn all_from(scene: &Scene) -> Vec<Self> {
        let skeleton = Skeleton::from_scene(scene);
        let hip_height = hip_height(&skeleton);
        scene
            .animations
            .iter()
            .enumerate()
            .map(|(index, animation)| {
                let mut clip = Self::from_animation(animation, &skeleton);
                if clip.name.is_empty() {
                    clip.name = format!("clip {}", index + 1);
                }
                clip.hip_height = hip_height;
                clip
            })
            .collect()
    }

    /// One clip of a file, resampled to [`KEYS_PER_SECOND`].
    ///
    /// Parts that never change are kept as one value, and parts that never
    /// leave the bone's rest pose are left out.
    #[must_use]
    pub fn from_animation(animation: &Animation, skeleton: &Skeleton) -> Self {
        let duration = animation.duration.max(0.0);
        let frames = frame_count(duration);
        let step = if frames > 1 {
            duration / (frames - 1) as f32
        } else {
            0.0
        };
        let rest = skeleton.rest();
        let names: Vec<Option<&str>> = skeleton.names().collect();
        let mut channels = Vec::with_capacity(animation.tracks.len());
        for track in &animation.tracks {
            let node = track.node as usize;
            let Some(Some(bone)) = names.get(node) else {
                continue;
            };
            let Some(rest) = rest.get(node) else {
                continue;
            };
            let part = match track.keys {
                Keys::Translation(_) => Part::Translation,
                Keys::Rotation(_) => Part::Rotation,
                Keys::Scale(_) => Part::Scale,
            };
            let mut values: Vec<[f32; 4]> = (0..frames)
                .map(|frame| {
                    let time = if frame + 1 == frames {
                        duration
                    } else {
                        frame as f32 * step
                    };
                    value_at(track, time)
                })
                .collect();
            if part == Part::Rotation {
                // Neighbouring keys on the same side, so a straight line
                // between them is the short way round.
                for i in 1..values.len() {
                    if dot4(values[i - 1], values[i]) < 0.0 {
                        values[i] = values[i].map(|v| -v);
                    }
                }
            }
            let first = values[0];
            if values.iter().all(|v| close(*v, first, part)) {
                if close(first, rest_value(rest, part), part) {
                    continue;
                }
                values.truncate(1);
            }
            channels.push(Channel {
                bone: (*bone).to_owned(),
                part,
                held: track.interpolation == Interpolation::Step,
                values,
            });
        }
        let name = animation.name.clone().unwrap_or_default();
        let mut clip = Self {
            looping: false,
            name,
            duration,
            markers: Vec::new(),
            channels,
            frames,
            hip_height: hip_height(skeleton),
        };
        let said = animation.extras.as_deref().and_then(said_to_loop);
        clip.looping = said.unwrap_or_else(|| {
            clip.name.to_ascii_lowercase().ends_with("loop")
                || crate::names::LOOPING.contains(&clip.name.as_str())
        });
        clip
    }

    /// How many keys each changing channel holds.
    #[must_use]
    pub fn frames(&self) -> u32 {
        self.frames
    }

    /// Makes the clip play on the spot: `bone` (usually the hips) keeps its
    /// height and sway but no longer travels across the ground (X and Z)
    /// from the first key to the last. The character's own movement does
    /// that instead, and the feet are matched to it by playing speed.
    ///
    /// Returns how far it travelled each second before, which is the speed
    /// the clip was made for.
    pub fn hold_in_place(&mut self, bone: &str) -> [f32; 3] {
        let Some(channel) = self
            .channels
            .iter_mut()
            .find(|c| c.bone == bone && c.part == Part::Translation)
        else {
            return [0.0; 3];
        };
        let (Some(&first), Some(&last)) = (channel.values.first(), channel.values.last()) else {
            return [0.0; 3];
        };
        let travel = [last[0] - first[0], 0.0, last[2] - first[2]];
        let count = channel.values.len();
        if count > 1 {
            for (i, value) in channel.values.iter_mut().enumerate() {
                let share = i as f32 / (count - 1) as f32;
                value[0] -= travel[0] * share;
                value[2] -= travel[2] * share;
            }
        }
        if self.duration > 0.0 {
            travel.map(|v| v / self.duration)
        } else {
            [0.0; 3]
        }
    }

    /// Fits the clip to `skeleton`, matching bones by name.
    #[must_use]
    pub fn bind(&self, skeleton: &Skeleton) -> Binding {
        let target_height = hip_height(skeleton);
        let ratio = if self.hip_height > 0.0 && target_height > 0.0 {
            target_height / self.hip_height
        } else {
            1.0
        };
        // Within a millimetre: the clip was made for this very body, so every
        // bone's travel is its own.
        let same_body = (ratio - 1.0).abs() < 1e-3;
        let targets = self
            .channels
            .iter()
            .map(|channel| {
                let bone = skeleton.find(&channel.bone)?;
                match channel.part {
                    Part::Rotation => Some((bone as u32, 1.0)),
                    Part::Translation if same_body => Some((bone as u32, 1.0)),
                    Part::Translation if MOVING_BONES.contains(&channel.bone.as_str()) => {
                        Some((bone as u32, ratio))
                    }
                    Part::Scale if same_body => Some((bone as u32, 1.0)),
                    // Another body's bone lengths and scale would stretch
                    // this one out of shape; it keeps its own.
                    Part::Translation | Part::Scale => None,
                }
            })
            .collect();
        Binding { targets }
    }

    /// Where `time` falls in the clip: wrapped round if it loops, held at
    /// either end if not.
    #[must_use]
    pub fn wrap(&self, time: f32) -> f32 {
        if self.duration <= 0.0 {
            return 0.0;
        }
        if self.looping {
            let turns = (time / self.duration).floor();
            (time - turns * self.duration).clamp(0.0, self.duration)
        } else {
            time.clamp(0.0, self.duration)
        }
    }

    /// Writes the clip's pose at `time` seconds over `out`, a pose of the
    /// skeleton it was bound to. Bones the clip does not move keep what
    /// `out` held, so start from the rest pose (or another clip's pose to
    /// lay this one over it).
    pub fn sample(&self, binding: &Binding, time: f32, out: &mut [Pose]) {
        self.sample_mixed(binding, time, 1.0, None, out);
    }

    /// Like [`Clip::sample`], but moves each bone only `weight` of the way
    /// from what `out` holds to the clip's pose, times the bone's share in
    /// `mask` if one is given (a mask lets an upper-body swing leave the
    /// legs to the walk).
    pub fn sample_mixed(
        &self,
        binding: &Binding,
        time: f32,
        weight: f32,
        mask: Option<&[f32]>,
        out: &mut [Pose],
    ) {
        if weight <= 0.0 {
            return;
        }
        let time = self.wrap(time);
        let (index, fraction) = self.key_at(time);
        for (channel, target) in self.channels.iter().zip(&binding.targets) {
            let Some((bone, scale)) = *target else {
                continue;
            };
            let bone = bone as usize;
            let Some(pose) = out.get_mut(bone) else {
                continue;
            };
            let share = weight * mask.and_then(|m| m.get(bone)).copied().unwrap_or(1.0);
            if share <= 0.0 {
                continue;
            }
            let value = channel.value(index, fraction);
            match channel.part {
                Part::Translation => {
                    let moved = [value[0] * scale, value[1] * scale, value[2] * scale];
                    pose.translation = if share >= 1.0 {
                        moved
                    } else {
                        lerp3(pose.translation, moved, share)
                    };
                }
                Part::Rotation => {
                    pose.rotation = if share >= 1.0 {
                        unit(value)
                    } else {
                        nlerp(pose.rotation, value, share)
                    };
                }
                Part::Scale => {
                    let stretched = [value[0], value[1], value[2]];
                    pose.scale = if share >= 1.0 {
                        stretched
                    } else {
                        lerp3(pose.scale, stretched, share)
                    };
                }
            }
        }
    }

    /// The key before `time` and how far it is towards the next, 0 to 1.
    fn key_at(&self, time: f32) -> (usize, f32) {
        if self.frames < 2 || self.duration <= 0.0 {
            return (0, 0.0);
        }
        let last = (self.frames - 1) as usize;
        let position = time / self.duration * last as f32;
        let index = position.floor();
        if index < 0.0 {
            return (0, 0.0);
        }
        let index = (index as usize).min(last);
        (index, (position - index as f32).clamp(0.0, 1.0))
    }

    /// The moments passed when play moves from `from` to `to` seconds,
    /// counting a loop's wrap and a whole lap or more, in the order they
    /// happen. Each is handed to `heard`.
    ///
    /// Moments are found by the time play crosses them, not by the frames
    /// drawn, so none is missed or heard twice at any frame rate.
    pub fn markers_passed(&self, from: f32, to: f32, mut heard: impl FnMut(&Marker)) {
        if self.markers.is_empty() || to <= from {
            return;
        }
        if !self.looping || self.duration <= 0.0 {
            for marker in &self.markers {
                if marker.time > from && marker.time <= to {
                    heard(marker);
                }
            }
            return;
        }
        // Lap by lap; a long step on a short loop hears each lap's moments.
        let mut lap = (from / self.duration).floor();
        // At most a few laps a tick in any sane game; the cap keeps a huge
        // jump from spinning.
        for _ in 0..64 {
            let start = lap * self.duration;
            if start >= to {
                break;
            }
            for marker in &self.markers {
                let at = start + marker.time;
                if at > from && at <= to {
                    heard(marker);
                }
            }
            lap += 1.0;
        }
    }
}

/// Which bone of a skeleton each channel of one clip moves, and how much a
/// translation is scaled, made by [`Clip::bind`].
#[derive(Debug, Clone, PartialEq)]
pub struct Binding {
    /// Per channel: the bone, and the scale for its travel; `None` when the
    /// skeleton has no such bone or the part is not carried over.
    targets: Vec<Option<(u32, f32)>>,
}

impl Binding {
    /// How many of the clip's channels found a bone.
    #[must_use]
    pub fn found(&self) -> usize {
        self.targets.iter().filter(|t| t.is_some()).count()
    }
}

impl Channel {
    /// The value `fraction` of the way from key `index` to the next.
    fn value(&self, index: usize, fraction: f32) -> [f32; 4] {
        let last = self.values.len().saturating_sub(1);
        let a = self.values[index.min(last)];
        if self.held || fraction <= 0.0 || index >= last {
            return a;
        }
        let b = self.values[index + 1];
        match self.part {
            Part::Rotation => nlerp(a, b, fraction),
            Part::Translation | Part::Scale => {
                let [x, y, z] = lerp3([a[0], a[1], a[2]], [b[0], b[1], b[2]], fraction);
                [x, y, z, 0.0]
            }
        }
    }
}

/// What a clip's custom properties say about looping: `"loop": true` or
/// `false`, spaces allowed. `None` when they do not say.
fn said_to_loop(extras: &str) -> Option<bool> {
    let squeezed: String = extras.chars().filter(|c| !c.is_whitespace()).collect();
    if squeezed.contains("\"loop\":true") {
        Some(true)
    } else if squeezed.contains("\"loop\":false") {
        Some(false)
    } else {
        None
    }
}

/// How many even keys a clip of `duration` seconds gets.
fn frame_count(duration: f32) -> u32 {
    if duration <= 0.0 || !duration.is_finite() {
        return 1;
    }
    // A clip of an hour is far past any real one; the cap keeps a damaged
    // duration from asking for endless memory (the loader caps it too).
    let frames = (duration * KEYS_PER_SECOND).ceil().min(108_000.0) as u32;
    frames.max(1) + 1
}

/// The value of `part` in a rest pose, laid out as a channel keeps it.
fn rest_value(rest: &Pose, part: Part) -> [f32; 4] {
    match part {
        Part::Translation => {
            let [x, y, z] = rest.translation;
            [x, y, z, 0.0]
        }
        Part::Rotation => rest.rotation,
        Part::Scale => {
            let [x, y, z] = rest.scale;
            [x, y, z, 0.0]
        }
    }
}

/// Whether two values of `part` look the same: a tenth of a millimetre, or a
/// tenth of a degree for turns (either sign of the quaternion). In f32, 1 -
/// 1e-8 is 1 itself, so the margin is 1e-6.
fn close(a: [f32; 4], b: [f32; 4], part: Part) -> bool {
    match part {
        Part::Rotation => dot4(a, b).abs() > 1.0 - 1e-6,
        Part::Translation | Part::Scale => a.iter().zip(&b).all(|(x, y)| (x - y).abs() < 1e-4),
    }
}

/// The value of `track` at `time` seconds, as the file describes it.
fn value_at(track: &Track, time: f32) -> [f32; 4] {
    let times = &track.times;
    let count = times.len();
    // The first key after `time`.
    let next = times.partition_point(|&t| t <= time);
    let (a, b, fraction, span) = if next == 0 {
        (0, 0, 0.0, 0.0)
    } else if next >= count {
        (count - 1, count - 1, 0.0, 0.0)
    } else {
        let span = times[next] - times[next - 1];
        let fraction = if span > 0.0 {
            (time - times[next - 1]) / span
        } else {
            0.0
        };
        (next - 1, next, fraction, span)
    };
    let get = |i: usize, which: usize| -> [f32; 4] {
        // Cubic keys are stored in threes: slope in, value, slope out.
        let at = if track.interpolation == Interpolation::CubicSpline {
            i * 3 + which
        } else {
            i
        };
        match &track.keys {
            Keys::Translation(v) | Keys::Scale(v) => {
                let [x, y, z] = v.get(at).copied().unwrap_or([0.0; 3]);
                [x, y, z, 0.0]
            }
            Keys::Rotation(v) => v.get(at).copied().unwrap_or([0.0, 0.0, 0.0, 1.0]),
        }
    };
    let rotation = matches!(track.keys, Keys::Rotation(_));
    match track.interpolation {
        Interpolation::Step => get(a, 0),
        Interpolation::Linear => {
            let (va, vb) = (get(a, 0), get(b, 0));
            if rotation {
                nlerp(va, vb, fraction)
            } else {
                let [x, y, z] = lerp3([va[0], va[1], va[2]], [vb[0], vb[1], vb[2]], fraction);
                [x, y, z, 0.0]
            }
        }
        Interpolation::CubicSpline => {
            let (va, vb) = (get(a, 1), get(b, 1));
            if a == b {
                return if rotation { unit(va) } else { va };
            }
            let (out_a, in_b) = (get(a, 2), get(b, 0));
            let t = fraction;
            let t2 = t * t;
            let t3 = t2 * t;
            let h00 = 2.0 * t3 - 3.0 * t2 + 1.0;
            let h10 = t3 - 2.0 * t2 + t;
            let h01 = -2.0 * t3 + 3.0 * t2;
            let h11 = t3 - t2;
            let mut out = [0.0; 4];
            for k in 0..4 {
                out[k] = h00 * va[k] + h10 * span * out_a[k] + h01 * vb[k] + h11 * span * in_b[k];
            }
            if rotation { unit(out) } else { out }
        }
    }
}

/// How high the hips stand above the model's origin at rest, or 0 if the
/// skeleton has no bone by a known hip name.
fn hip_height(skeleton: &Skeleton) -> f32 {
    let Some(hips) = HIP_BONES.iter().find_map(|name| skeleton.find(name)) else {
        return 0.0;
    };
    let mut placed = vec![crate::math::IDENTITY; skeleton.len()];
    skeleton.place(skeleton.rest(), &mut placed);
    placed[hips][3][1].abs()
}
