//! What one character is playing, stepped on the fixed tick.
//!
//! Moving about is chosen from the character's speed and footing, with no
//! script: idle, walk, run and sprint blend by speed, crouching has its own
//! pair, and jumping, falling and landing follow the feet. The clips of one
//! gait are kept in step (both feet down at the same moment of each) and
//! played at the speed the body really moves, so the feet do not slide.
//! Clips a script asks for play over that, on the whole body or on the upper
//! body alone, and every change fades.

use lore_assets::{Pose, Transform};

use crate::clip::{Binding, Clip, Marker};
use crate::math::{IDENTITY, blend_pose, origin, quat_mul, unit};
use crate::skeleton::Skeleton;

/// Seconds a change of what is playing takes to fade in, unless asked otherwise.
pub const DEFAULT_FADE: f32 = 0.2;

/// Standard gravity, for how far a body leans into a change of speed.
const GRAVITY: f32 = 9.81;

/// Which part of the body a clip moves.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Layer {
    /// All of it: the clip takes over from moving about while it plays.
    Body,
    /// From the waist up: the legs keep walking (a sword swing on the run).
    Upper,
}

/// Where a character's feet are.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub enum Footing {
    /// On the ground.
    #[default]
    Ground,
    /// In the air, going up (metres a second, positive) or down (negative).
    Air(f32),
    /// In water deep enough to swim.
    Swimming,
    /// On a ladder or a wall.
    Climbing,
}

/// How a character is moving this tick: what the animator reads to choose
/// its clips. The simulation fills it in; nothing here moves the character.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Moving {
    /// Speed across the ground, metres a second.
    pub speed: f32,
    /// The way it moves relative to the way it faces, across (+X right) and
    /// along (+Z forward); length 1 or 0. Ignored while standing still.
    pub direction: [f32; 2],
    /// Whether it is crouching.
    pub crouched: bool,
    /// Where its feet are.
    pub footing: Footing,
    /// How fast it hit the ground, metres a second, on the tick it lands;
    /// 0 on every other tick.
    pub landed: f32,
    /// How fast its speed is changing, metres a second each second, along
    /// (forward) and across (right) the way it faces. A body leans into it.
    pub accel: [f32; 2],
}

/// How to play a clip, for [`Animator::play`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Play {
    /// Which part of the body it moves.
    pub layer: Layer,
    /// Seconds to fade in, and out at the end.
    pub fade: f32,
    /// How fast it plays: 1 as made, 2 twice as fast.
    pub speed: f32,
    /// Whether it starts again when it ends; `None` keeps the clip's own choice.
    pub looping: Option<bool>,
}

impl Default for Play {
    fn default() -> Self {
        Self {
            layer: Layer::Body,
            fade: DEFAULT_FADE,
            speed: 1.0,
            looping: None,
        }
    }
}

/// A clip a script asked for, as it plays.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Playing {
    /// Which of the body's clips.
    pub clip: u32,
    /// Seconds into it, before wrapping round.
    pub time: f32,
    /// How fast it plays.
    pub speed: f32,
    /// Whether it starts again when it ends.
    pub looping: bool,
    /// How much it shows now, 0 to 1.
    pub weight: f32,
    /// Seconds to fade in or out.
    pub fade: f32,
    /// Whether it is fading out (it ended, or was stopped).
    pub leaving: bool,
    /// A number to tell this play from another of the same clip.
    pub id: u32,
}

/// The clips of one way of moving, by direction.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct Gait {
    /// Forward, backward, left and right: clip numbers, forward always set.
    clips: [Option<u32>; 4],
    /// How fast the forward clip carries the body, metres a second.
    speed: f32,
}

/// A body ready to animate: its skeleton, its clips fitted to it, and what
/// it has worked out about them.
#[derive(Debug, Clone)]
pub struct Body {
    skeleton: Skeleton,
    clips: Vec<Clip>,
    bindings: Vec<Binding>,
    /// For each bone, how much an upper-body clip moves it: 1 from the
    /// lowest spine bone up, 0 below.
    upper: Vec<f32>,
    /// Standing gaits slowest first (walk, run, sprint), and crouching ones.
    standing: Vec<Gait>,
    crouching: Vec<Gait>,
    idle: Option<u32>,
    crouch_idle: Option<u32>,
    jump: Option<u32>,
    fall: Option<u32>,
    land: Option<u32>,
    swim: Option<u32>,
    swim_idle: Option<u32>,
    climb: Option<u32>,
}

/// Clip names a body looks for, as LORE's mannequin names them. A model
/// whose clips are named this way moves with no script.
pub mod names {
    /// Standing still.
    pub const IDLE: &str = "idle";
    /// Walking: forward, backward, left, right.
    pub const WALK: [&str; 4] = ["walk", "walk_back", "walk_left", "walk_right"];
    /// Running.
    pub const RUN: [&str; 4] = ["run", "run_back", "run_left", "run_right"];
    /// Sprinting (forward only).
    pub const SPRINT: &str = "sprint";
    /// Crouching still.
    pub const CROUCH_IDLE: &str = "crouch_idle";
    /// Crouching and moving.
    pub const CROUCH_WALK: [&str; 4] =
        ["crouch_walk", "crouch_back", "crouch_left", "crouch_right"];
    /// Leaving the ground.
    pub const JUMP: &str = "jump";
    /// In the air.
    pub const FALL: &str = "fall";
    /// Touching down.
    pub const LAND: &str = "land";
    /// Swimming, and treading water.
    pub const SWIM: &str = "swim";
    /// Treading water.
    pub const SWIM_IDLE: &str = "swim_idle";
    /// On a ladder or a wall.
    pub const CLIMB: &str = "climb";
    /// The moving-about clips that cycle, which loop even when the file does
    /// not say so.
    pub const LOOPING: [&str; 21] = [
        IDLE, WALK[0], WALK[1], WALK[2], WALK[3], RUN[0], RUN[1], RUN[2], RUN[3], SPRINT,
        CROUCH_IDLE, CROUCH_WALK[0], CROUCH_WALK[1], CROUCH_WALK[2], CROUCH_WALK[3], FALL, SWIM,
        SWIM_IDLE, CLIMB, "idle_look", "talk",
    ];
}

/// Speeds a gait is taken to have when its clip cannot be measured (its feet
/// never touch the ground): a person's usual walk, run and sprint.
const USUAL_SPEEDS: [f32; 3] = [1.4, 3.5, 6.0];

/// The speed a crouching walk is taken to have when it cannot be measured.
const USUAL_CROUCH_SPEED: f32 = 1.0;

impl Body {
    /// A body from a skeleton and the clips that go with it (from the
    /// model's own file, from another, or both). Clips whose names match
    /// [`names`] are used for moving about; the rest are for scripts to play.
    #[must_use]
    pub fn new(skeleton: Skeleton, mut clips: Vec<Clip>) -> Self {
        let hips = ["pelvis", "hips", "Hips", "mixamorig:Hips"]
            .iter()
            .find_map(|n| skeleton.find(n).map(|_| *n));
        let find = |clips: &[Clip], name: &str| {
            clips.iter().position(|c| c.name == name).map(|i| i as u32)
        };
        // Moving clips play on the spot; the body is carried by the game.
        let mut held_speed = vec![0.0f32; clips.len()];
        if let Some(hips) = hips {
            for (i, clip) in clips.iter_mut().enumerate() {
                let moving = names::WALK
                    .iter()
                    .chain(&names::RUN)
                    .chain(&names::CROUCH_WALK)
                    .chain([&names::SPRINT, &names::SWIM])
                    .any(|n| *n == clip.name);
                if moving {
                    let [x, _, z] = clip.hold_in_place(hips);
                    held_speed[i] = (x * x + z * z).sqrt();
                }
            }
        }
        let bindings: Vec<Binding> = clips.iter().map(|c| c.bind(&skeleton)).collect();
        let upper = upper_mask(&skeleton);
        let mut body = Self {
            idle: find(&clips, names::IDLE),
            crouch_idle: find(&clips, names::CROUCH_IDLE),
            jump: find(&clips, names::JUMP),
            fall: find(&clips, names::FALL),
            land: find(&clips, names::LAND),
            swim: find(&clips, names::SWIM),
            swim_idle: find(&clips, names::SWIM_IDLE),
            climb: find(&clips, names::CLIMB),
            standing: Vec::new(),
            crouching: Vec::new(),
            skeleton,
            clips,
            bindings,
            upper,
        };
        let gait = |body: &mut Self, set: [&str; 4], usual: f32| -> Option<Gait> {
            let clips = set.map(|n| find(&body.clips, n));
            let forward = clips[0]?;
            let measured = body.ground_speed(forward as usize);
            let held = held_speed[forward as usize];
            let speed = if measured > 0.2 {
                measured
            } else if held > 0.2 {
                held
            } else {
                usual
            };
            body.mark_feet(clips);
            Some(Gait { clips, speed })
        };
        let walk = gait(&mut body, names::WALK, USUAL_SPEEDS[0]);
        let run = gait(&mut body, names::RUN, USUAL_SPEEDS[1]);
        let sprint = gait(&mut body, [names::SPRINT, "", "", ""], USUAL_SPEEDS[2]);
        let crouch = gait(&mut body, names::CROUCH_WALK, USUAL_CROUCH_SPEED);
        let mut standing: Vec<Gait> = [walk, run, sprint].into_iter().flatten().collect();
        standing.sort_by(|a, b| a.speed.total_cmp(&b.speed));
        body.standing = standing;
        body.crouching = crouch.into_iter().collect();
        body
    }

    /// Its skeleton.
    #[must_use]
    pub fn skeleton(&self) -> &Skeleton {
        &self.skeleton
    }

    /// Its clips.
    #[must_use]
    pub fn clips(&self) -> &[Clip] {
        &self.clips
    }

    /// The number of the clip called `name`.
    #[must_use]
    pub fn clip(&self, name: &str) -> Option<u32> {
        self.clips
            .iter()
            .position(|c| c.name == name)
            .map(|i| i as u32)
    }

    /// The speed, metres a second, at which the body walks, runs and sprints
    /// with its feet planted, slowest first.
    pub fn gait_speeds(&self) -> impl Iterator<Item = f32> + '_ {
        self.standing.iter().map(|g| g.speed)
    }

    /// How fast an on-the-spot clip would carry the body across the ground:
    /// the speed the planted foot slides back under the hips.
    ///
    /// Each key, the lower foot is taken to be the planted one; its travel
    /// backwards over the keys it is planted, over the time, is the speed.
    /// 0 when the body has no feet by a known name or they never touch.
    fn ground_speed(&self, clip: usize) -> f32 {
        let Some((left, right)) = self.feet() else {
            return 0.0;
        };
        let clip_data = &self.clips[clip];
        let frames = clip_data.frames().max(2) as usize;
        let step = clip_data.duration / (frames - 1) as f32;
        if step <= 0.0 {
            return 0.0;
        }
        let mut pose = self.skeleton.rest().to_vec();
        let mut placed = vec![IDENTITY; self.skeleton.len()];
        let mut previous: Option<([f32; 3], [f32; 3])> = None;
        let (mut travel, mut time) = (0.0f32, 0.0f32);
        for frame in 0..frames {
            pose.copy_from_slice(self.skeleton.rest());
            clip_data.sample(&self.bindings[clip], frame as f32 * step, &mut pose);
            self.skeleton.place(&pose, &mut placed);
            let feet = (origin(&placed[left]), origin(&placed[right]));
            if let Some((was_left, was_right)) = previous {
                // The foot lower over both keys is the one on the ground.
                let (now, was) = if feet.0[1] + was_left[1] <= feet.1[1] + was_right[1] {
                    (feet.0, was_left)
                } else {
                    (feet.1, was_right)
                };
                let (dx, dz) = (now[0] - was[0], now[2] - was[2]);
                travel += (dx * dx + dz * dz).sqrt();
                time += step;
            }
            previous = Some(feet);
        }
        // The skeleton's own scale (an armature exported in centimetres)
        // is in `placed`, so this is in the model's metres.
        if time > 0.0 { travel / time } else { 0.0 }
    }

    /// Adds `left_foot_down` and `right_foot_down` markers to the gait's
    /// clips, at the keys each foot becomes the lower one. Footstep sounds
    /// and anything else that should land with a foot listen for them.
    fn mark_feet(&mut self, clips: [Option<u32>; 4]) {
        let Some((left, right)) = self.feet() else {
            return;
        };
        for clip in clips.into_iter().flatten() {
            let clip = clip as usize;
            if self.clips[clip]
                .markers
                .iter()
                .any(|m| m.name.ends_with("_foot_down"))
            {
                continue;
            }
            let frames = self.clips[clip].frames().max(2) as usize;
            let step = self.clips[clip].duration / (frames - 1) as f32;
            let mut pose = self.skeleton.rest().to_vec();
            let mut placed = vec![IDENTITY; self.skeleton.len()];
            let mut lower: Vec<bool> = Vec::with_capacity(frames);
            for frame in 0..frames {
                pose.copy_from_slice(self.skeleton.rest());
                self.clips[clip].sample(&self.bindings[clip], frame as f32 * step, &mut pose);
                self.skeleton.place(&pose, &mut placed);
                lower.push(placed[left][3][1] <= placed[right][3][1]);
            }
            let mut markers = Vec::new();
            for frame in 0..frames.saturating_sub(1) {
                let (was, now) = (lower[frame], lower[frame + 1]);
                if was != now {
                    markers.push(Marker {
                        name: if now {
                            "left_foot_down"
                        } else {
                            "right_foot_down"
                        }
                        .to_owned(),
                        time: (frame + 1) as f32 * step,
                    });
                }
            }
            self.clips[clip].markers.extend(markers);
        }
    }

    /// The left and right foot bones, by the names LORE, Blender and Mixamo use.
    fn feet(&self) -> Option<(usize, usize)> {
        [
            ("foot_l", "foot_r"),
            ("LeftFoot", "RightFoot"),
            ("foot.L", "foot.R"),
            ("mixamorig:LeftFoot", "mixamorig:RightFoot"),
        ]
        .iter()
        .find_map(|(l, r)| Some((self.skeleton.find(l)?, self.skeleton.find(r)?)))
    }
}

/// Which bones an upper-body clip moves: the lowest spine bone and
/// everything that hangs from it.
fn upper_mask(skeleton: &Skeleton) -> Vec<f32> {
    let mut mask = vec![0.0; skeleton.len()];
    let Some(spine) = ["spine_01", "Spine", "spine", "mixamorig:Spine"]
        .iter()
        .find_map(|n| skeleton.find(n))
    else {
        return mask;
    };
    for bone in 0..skeleton.len() {
        // Parents come first, so a parent's share is already known.
        mask[bone] = if bone == spine {
            1.0
        } else {
            skeleton.parent(bone).map_or(0.0, |p| mask[p])
        };
    }
    mask
}

/// What moving about is doing.
#[derive(Debug, Clone, Copy, PartialEq)]
enum State {
    Ground,
    Jumping,
    Falling,
    Landing,
    Swimming,
    Climbing,
}

/// What one character is playing.
///
/// Make one per character with [`Animator::new`], call [`Animator::step`]
/// once a tick and [`Animator::pose`] when its bones are wanted. Its buffers
/// are made once, here, and reused.
#[derive(Debug, Clone)]
pub struct Animator {
    state: State,
    /// Seconds since the state began.
    state_time: f32,
    /// How far through a stride the gaits are, 0 to 1. Kept in common so a
    /// walk blends into a run with the feet together.
    phase: f32,
    /// The idle's own clock.
    idle_time: f32,
    /// How hard the last landing was, 0 to 1.
    landing: f32,
    /// What it moved like last tick, smoothed: speed and direction.
    speed: f32,
    direction: [f32; 2],
    crouch: f32,
    lean: [f32; 2],
    /// The clips scripts asked for: at most a few at once.
    layers: Vec<Playing>,
    next_id: u32,
    /// The pose last worked out, and the one a fade is leaving.
    last: Vec<Pose>,
    from: Vec<Pose>,
    fade_left: f32,
    fade_total: f32,
    /// A pose to work in.
    scratch: Vec<Pose>,
}

/// The most clips a script may have playing on one character at once.
pub const MOST_PLAYING: usize = 8;

impl Animator {
    /// An animator for `body`, standing still at rest.
    #[must_use]
    pub fn new(body: &Body) -> Self {
        let rest = body.skeleton.rest().to_vec();
        Self {
            state: State::Ground,
            state_time: 0.0,
            phase: 0.0,
            idle_time: 0.0,
            landing: 0.0,
            speed: 0.0,
            direction: [0.0, 1.0],
            crouch: 0.0,
            lean: [0.0; 2],
            layers: Vec::with_capacity(MOST_PLAYING),
            next_id: 1,
            last: rest.clone(),
            from: rest.clone(),
            fade_left: 0.0,
            fade_total: 0.0,
            scratch: rest,
        }
    }

    /// Plays `clip` as `how` says, and returns a number for this play.
    /// A clip already playing on the same layer fades out as this fades in.
    /// `None` when the clip is not one of the body's.
    pub fn play(&mut self, body: &Body, clip: u32, how: Play) -> Option<u32> {
        let data = body.clips.get(clip as usize)?;
        for playing in &mut self.layers {
            if layer_of(body, playing) == how.layer || playing.clip == clip {
                playing.leaving = true;
            }
        }
        if self.layers.len() >= MOST_PLAYING {
            // Make room: the faintest goes first.
            if let Some(faintest) = (0..self.layers.len())
                .min_by(|&a, &b| self.layers[a].weight.total_cmp(&self.layers[b].weight))
            {
                self.layers.swap_remove(faintest);
            }
        }
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        let upper = how.layer == Layer::Upper;
        self.layers.push(Playing {
            clip,
            time: 0.0,
            speed: if how.speed.is_finite() {
                how.speed.clamp(0.0, 10.0)
            } else {
                1.0
            },
            looping: how.looping.unwrap_or(data.looping),
            weight: if how.fade > 0.0 { 0.0 } else { 1.0 },
            fade: how.fade.max(0.0),
            leaving: false,
            // The top bit says the upper layer, so `layer_of` needs no lookup.
            id: if upper {
                id | UPPER_BIT
            } else {
                id & !UPPER_BIT
            },
        });
        Some(id & !UPPER_BIT)
    }

    /// Fades out the play numbered `id` (or every play of every clip if `None`)
    /// over `fade` seconds.
    pub fn stop(&mut self, id: Option<u32>, fade: f32) {
        for playing in &mut self.layers {
            if id.is_none_or(|id| playing.id & !UPPER_BIT == id) {
                playing.leaving = true;
                playing.fade = fade.max(0.0);
            }
        }
    }

    /// Whether the play numbered `id` is still showing.
    #[must_use]
    pub fn is_playing(&self, id: u32) -> bool {
        self.layers.iter().any(|p| p.id & !UPPER_BIT == id)
    }

    /// The clips a script asked for that are still showing.
    #[must_use]
    pub fn playing(&self) -> &[Playing] {
        &self.layers
    }

    /// Moves time on by `seconds` (one tick), reading how the body moves.
    /// Each marker play passes is handed to `heard` with the clip it is in.
    pub fn step(
        &mut self,
        body: &Body,
        moving: &Moving,
        seconds: f32,
        mut heard: impl FnMut(u32, &Marker),
    ) {
        let dt = if seconds.is_finite() {
            seconds.max(0.0)
        } else {
            0.0
        };
        self.follow(moving, dt);
        let state = self.next_state(body, moving);
        if state != self.state {
            self.state = state;
            self.state_time = 0.0;
            self.begin_fade(if state == State::Landing {
                0.08
            } else {
                DEFAULT_FADE
            });
        } else {
            self.state_time += dt;
        }
        if moving.landed > 0.0 {
            // A hard landing (3 m/s, a jump from about half a metre) shows in
            // full; a step down hardly at all.
            self.landing = ((moving.landed - 1.0) / 5.0).clamp(0.0, 1.0);
        }
        self.idle_time += dt;
        // The gait's stride: how far one cycle of the blended clips carries
        // the body. The phase moves on by the ground covered over it, so the
        // planted foot moves exactly with the ground.
        let gaits = if moving.crouched {
            &body.crouching
        } else {
            &body.standing
        };
        if let Some((a, b, share)) = gait_pair(gaits, self.speed) {
            let cycle = |g: &Gait| {
                let clip = &body.clips[g.clips[0].unwrap_or(0) as usize];
                (clip.duration.max(0.1), g.speed * clip.duration.max(0.1))
            };
            let (duration_a, stride_a) = cycle(a);
            let (duration_b, stride_b) = cycle(b);
            let stride = stride_a + (stride_b - stride_a) * share;
            let duration = duration_a + (duration_b - duration_a) * share;
            // Below the slowest gait the feet keep the gait's own cadence and
            // shorten the step, as people do, rather than stepping in slow motion.
            let rate = if self.speed < a.speed && share <= 0.0 {
                1.0 / duration
            } else if stride > 0.0 {
                self.speed / stride
            } else {
                0.0
            };
            let before = self.phase;
            self.phase += rate * dt;
            if self.state == State::Ground && self.speed > 0.1 {
                // Footsteps from the gait that shows most.
                let main = if share < 0.5 { a } else { b };
                if let Some(clip) = main.clips[0] {
                    let c = &body.clips[clip as usize];
                    c.markers_passed(before * c.duration, self.phase * c.duration, |m| {
                        heard(clip, m);
                    });
                }
            }
            self.phase -= self.phase.floor();
        }
        // Clips scripts asked for.
        let mut i = 0;
        while i < self.layers.len() {
            let playing = &mut self.layers[i];
            let clip = &body.clips[playing.clip as usize];
            let before = playing.time;
            playing.time += dt * playing.speed;
            if !playing.leaving {
                clip.markers_passed(before, playing.time, |m| heard(playing.clip, m));
            }
            let fade_rate = if playing.fade > 0.0 {
                dt / playing.fade
            } else {
                1.0
            };
            if !playing.looping && playing.time >= clip.duration - playing.fade * playing.speed {
                playing.leaving = true;
            }
            playing.weight = if playing.leaving {
                playing.weight - fade_rate
            } else {
                (playing.weight + fade_rate).min(1.0)
            };
            if playing.leaving && playing.weight <= 0.0 {
                self.layers.remove(i);
            } else {
                i += 1;
            }
        }
        if self.fade_left > 0.0 {
            self.fade_left = (self.fade_left - dt).max(0.0);
        }
    }

    /// Smooths what the simulation says about moving, so a sudden stop or
    /// turn does not snap the legs.
    fn follow(&mut self, moving: &Moving, dt: f32) {
        // A critically damped approach, without `exp`: a rational stand-in
        // that every computer works out alike. About a tenth of a second.
        let toward = |now: f32, goal: f32, half_life: f32| {
            let k = dt / (dt + half_life);
            now + (goal - now) * k
        };
        let speed = if moving.speed.is_finite() {
            moving.speed.max(0.0)
        } else {
            0.0
        };
        self.speed = toward(self.speed, speed, 0.06);
        let [dx, dz] = moving.direction;
        if speed > 0.05 && (dx * dx + dz * dz) > 0.25 {
            let x = toward(self.direction[0], dx, 0.08);
            let z = toward(self.direction[1], dz, 0.08);
            let length = (x * x + z * z).sqrt();
            if length > 1e-3 {
                self.direction = [x / length, z / length];
            }
        }
        self.crouch = toward(self.crouch, if moving.crouched { 1.0 } else { 0.0 }, 0.08);
        // A body leans into a change of speed until its weight is over its
        // feet again: the angle whose tangent is the push over gravity. For
        // the small angles people lean, the angle and its tangent are the
        // same, and it is held under about 14 degrees.
        let lean = moving.accel.map(|a| (a / GRAVITY).clamp(-0.25, 0.25));
        self.lean = [
            toward(self.lean[0], lean[0], 0.15),
            toward(self.lean[1], lean[1], 0.15),
        ];
    }

    /// What moving about should be doing now.
    fn next_state(&self, body: &Body, moving: &Moving) -> State {
        match moving.footing {
            Footing::Swimming => State::Swimming,
            Footing::Climbing => State::Climbing,
            Footing::Air(rising) => {
                if self.state == State::Jumping
                    && rising <= 0.0
                    && body
                        .jump
                        .is_none_or(|j| self.state_time >= body.clips[j as usize].duration * 0.5)
                {
                    State::Falling
                } else if self.state == State::Jumping || self.state == State::Falling {
                    self.state
                } else if rising > 0.5 && body.jump.is_some() {
                    State::Jumping
                } else {
                    State::Falling
                }
            }
            Footing::Ground => {
                let landing_lasts = body
                    .land
                    .map_or(0.0, |l| body.clips[l as usize].duration)
                    .min(0.6);
                // Landing from the air, or still landing. Moving fast, the
                // legs run straight on instead.
                let touching_down = matches!(self.state, State::Jumping | State::Falling)
                    && body.land.is_some();
                let still_landing = self.state == State::Landing
                    && self.state_time < landing_lasts * (0.4 + 0.6 * self.landing);
                if (touching_down || still_landing) && moving.speed < 2.5 {
                    State::Landing
                } else {
                    State::Ground
                }
            }
        }
    }

    /// Starts fading from the pose last shown to whatever comes next.
    fn begin_fade(&mut self, seconds: f32) {
        self.from.copy_from_slice(&self.last);
        self.fade_left = seconds;
        self.fade_total = seconds;
    }

    /// Works out every bone's pose, relative to its parent, into `out`
    /// (as long as the skeleton).
    pub fn pose(&mut self, body: &Body, out: &mut [Pose]) {
        let rest = body.skeleton.rest();
        let count = rest.len().min(out.len());
        let out = &mut out[..count];
        out.copy_from_slice(&rest[..count]);
        self.moving_pose(body, out);
        // A whole-body clip over the moving pose, then upper-body ones.
        for playing in &self.layers {
            let clip = &body.clips[playing.clip as usize];
            let mask = (playing.id & UPPER_BIT != 0).then_some(body.upper.as_slice());
            let weight = smooth(playing.weight);
            clip.sample_mixed(
                &body.bindings[playing.clip as usize],
                playing.time,
                weight,
                mask,
                out,
            );
        }
        self.lean_into(body, out);
        if self.fade_left > 0.0 && self.fade_total > 0.0 {
            let shown = smooth(1.0 - self.fade_left / self.fade_total);
            for (pose, from) in out.iter_mut().zip(&self.from) {
                *pose = blend_pose(from, pose, shown);
            }
        }
        let len = out.len().min(self.last.len());
        self.last[..len].copy_from_slice(&out[..len]);
    }

    /// The pose of moving about alone, over the rest pose in `out`.
    fn moving_pose(&mut self, body: &Body, out: &mut [Pose]) {
        let sample = |clip: Option<u32>, time: f32, weight: f32, out: &mut [Pose]| {
            if let Some(clip) = clip {
                let c = &body.clips[clip as usize];
                c.sample_mixed(&body.bindings[clip as usize], time, weight, None, out);
            }
        };
        match self.state {
            State::Jumping => sample(body.jump.or(body.fall), self.state_time, 1.0, out),
            State::Falling => sample(body.fall.or(body.jump), self.state_time, 1.0, out),
            State::Landing => {
                // A soft landing plays quickly and lightly over the stance.
                self.ground_pose(body, out);
                let speed = 1.0 + (1.0 - self.landing);
                sample(
                    body.land,
                    self.state_time * speed,
                    0.5 + 0.5 * self.landing,
                    out,
                );
            }
            State::Swimming => {
                let moving = smooth((self.speed / 1.0).min(1.0));
                sample(body.swim_idle.or(body.swim), self.idle_time, 1.0, out);
                sample(body.swim, self.idle_time, moving, out);
            }
            State::Climbing => sample(body.climb.or(body.idle), self.idle_time, 1.0, out),
            State::Ground => self.ground_pose(body, out),
        }
    }

    /// Standing, walking, running or crouching, blended by speed and direction.
    fn ground_pose(&mut self, body: &Body, out: &mut [Pose]) {
        let len = out.len();
        self.layer_pose(body, &body.standing, body.idle, len);
        out.copy_from_slice(&self.scratch[..len]);
        if self.crouch > 0.001 && (body.crouch_idle.is_some() || !body.crouching.is_empty()) {
            let crouch_idle = body.crouch_idle.or(body.idle);
            self.layer_pose(body, &body.crouching, crouch_idle, len);
            let share = smooth(self.crouch);
            for (pose, crouched) in out.iter_mut().zip(&self.scratch) {
                *pose = blend_pose(pose, crouched, share);
            }
        }
    }

    /// One set of gaits and its idle, blended by speed and direction, into
    /// `self.scratch` (from the rest pose).
    fn layer_pose(&mut self, body: &Body, gaits: &[Gait], idle: Option<u32>, len: usize) {
        let scratch = &mut self.scratch[..len];
        scratch.copy_from_slice(&body.skeleton.rest()[..len]);
        if let Some(idle) = idle {
            let c = &body.clips[idle as usize];
            c.sample(&body.bindings[idle as usize], self.idle_time, scratch);
        }
        let Some((a, b, share)) = gait_pair(gaits, self.speed) else {
            return;
        };
        // From idle to the slowest gait over its first half: a slow shuffle
        // is mostly a short-stepped walk.
        let moving = smooth((self.speed / (a.speed * 0.5)).min(1.0));
        if moving <= 0.0 {
            return;
        }
        let weights = direction_weights(self.direction);
        // Each gait's directions are laid in turn: what is laid first is
        // taken over in part by what comes after, so each share is its
        // weight over the weight still to come.
        let mut laid = 0.0;
        for (gait, gait_share) in [(a, 1.0 - share), (b, share)] {
            if gait_share <= 0.0 {
                continue;
            }
            for (way, &w) in weights.iter().enumerate() {
                let w = w * gait_share;
                if w <= 0.0 {
                    continue;
                }
                let clip = gait.clips[way].or(gait.clips[0]);
                let Some(clip) = clip else { continue };
                laid += w;
                // Idle holds 1 - moving; each clip laid so far its own
                // share of moving. Laying this one at its share of the total
                // so far keeps every share right.
                let total = (1.0 - moving) + moving * laid;
                let c = &body.clips[clip as usize];
                c.sample_mixed(
                    &body.bindings[clip as usize],
                    self.phase * c.duration,
                    if total > 0.0 { moving * w / total } else { 1.0 },
                    None,
                    scratch,
                );
            }
        }
    }

    /// Leans the whole body into a change of speed, pivoting at the feet
    /// as a person does (an inverted pendulum). A model faces +Z, as glTF
    /// asks, with +Y up, so its right is -X.
    fn lean_into(&self, body: &Body, out: &mut [Pose]) {
        let [forward, right] = self.lean;
        if forward.abs() < 1e-4 && right.abs() < 1e-4 {
            return;
        }
        // For the small angles people lean, half the angle in the vector
        // part, normalised, is the turn: no sine or cosine needed.
        let turn = unit([forward * 0.5, 0.0, right * 0.5, 1.0]);
        for (bone, pose) in out.iter_mut().enumerate() {
            if body.skeleton.parent(bone).is_none() {
                pose.rotation = unit(quat_mul(turn, pose.rotation));
                pose.translation = rotate(turn, pose.translation);
            }
        }
    }

    /// Where every bone stands relative to the model, after [`Animator::pose`]
    /// has written `local`. Convenience for [`Skeleton::place`].
    pub fn place(body: &Body, local: &[Pose], out: &mut [Transform]) {
        body.skeleton.place(local, out);
    }
}

/// The bit of [`Playing::id`] that marks an upper-body play.
const UPPER_BIT: u32 = 1 << 31;

/// Which layer a play is on.
fn layer_of(_body: &Body, playing: &Playing) -> Layer {
    if playing.id & UPPER_BIT != 0 {
        Layer::Upper
    } else {
        Layer::Body
    }
}

/// The two gaits either side of `speed`, and how far from the first to the
/// second it is. Below the slowest, both are the slowest; above the fastest,
/// both are the fastest.
fn gait_pair(gaits: &[Gait], speed: f32) -> Option<(&Gait, &Gait, f32)> {
    let first = gaits.first()?;
    if speed <= first.speed {
        return Some((first, first, 0.0));
    }
    for pair in gaits.windows(2) {
        let (a, b) = (&pair[0], &pair[1]);
        if speed <= b.speed {
            let span = b.speed - a.speed;
            let share = if span > 0.0 {
                (speed - a.speed) / span
            } else {
                1.0
            };
            return Some((a, b, share));
        }
    }
    let last = gaits.last()?;
    Some((last, last, 0.0))
}

/// How much of each of forward, backward, left and right a direction is
/// (across, along): shares that add up to 1.
fn direction_weights([x, z]: [f32; 2]) -> [f32; 4] {
    let parts = [z.max(0.0), (-z).max(0.0), (-x).max(0.0), x.max(0.0)];
    let total: f32 = parts.iter().sum();
    if total > 1e-6 {
        parts.map(|p| p / total)
    } else {
        [1.0, 0.0, 0.0, 0.0]
    }
}

/// An ease in and out: 0 at 0, 1 at 1, flat at both ends.
fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// `v` turned by the unit quaternion `q`.
fn rotate(q: [f32; 4], v: [f32; 3]) -> [f32; 3] {
    let [x, y, z, w] = q;
    // v + 2w(q×v) + 2q×(q×v)
    let c = [y * v[2] - z * v[1], z * v[0] - x * v[2], x * v[1] - y * v[0]];
    let cc = [y * c[2] - z * c[1], z * c[0] - x * c[2], x * c[1] - y * c[0]];
    [
        v[0] + 2.0 * (w * c[0] + cc[0]),
        v[1] + 2.0 * (w * c[1] + cc[1]),
        v[2] + 2.0 * (w * c[2] + cc[2]),
    ]
}
