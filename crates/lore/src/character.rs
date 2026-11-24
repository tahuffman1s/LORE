//! The player's body, and the camera that sees the world from its eyes or
//! from over its shoulder.
//!
//! In first person the camera is at the eyes and the body is drawn without
//! its head, so looking down shows legs and the sun throws a whole shadow.
//! In third person the camera hangs on a boom behind and above one shoulder:
//! it turns with the look action, pulls in at once when something comes
//! between it and the player, and eases back out. Switching travels between
//! the two rather than cutting.
//!
//! The body faces the way it walks ("move") or the way the camera looks
//! ("aim"), turning at a speed, never snapping. Everything here that the
//! game's rules could see is worked out on the tick with operations every
//! computer rounds alike (`libm` for angles), so a replay turns the same.
//!
//! Research: Unreal's SpringArm (a probe ball, instant pull-in, lagged
//! return), John Nesky's "50 Game Camera Mistakes" (GDC 2014: never through
//! a wall, never inside the character, keep stick and mouse alike), Red Dead
//! Redemption 2 and Skyrim for a view switch on one button, and the
//! research behind `lore-anim` for turning speeds.

use std::sync::Arc;

use lore_anim::{Animator, Body, Clip, Footing, Moving, Skeleton};
use lore_assets::{AssetError, LoadOptions, Pose, Scene, Transform};
use lore_physics::{Level, Player};

/// LORE's own body, drawn for the player until a game names another.
pub(crate) const MANNEQUIN: &[u8] = include_bytes!("../data/mannequin/mannequin.glb");

/// The same body's female form, with no clips of its own: it plays
/// [`MANNEQUIN`]'s by bone name.
pub(crate) const MANNEQUIN_FEMALE: &[u8] = include_bytes!("../data/mannequin/mannequin_female.glb");

/// Where the camera is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum View {
    /// At the eyes.
    #[default]
    First,
    /// Behind and above a shoulder.
    Third,
}

/// Which way the body turns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub(crate) enum Facing {
    /// The way it walks: an adventure's character runs where the stick
    /// points, relative to the camera.
    #[default]
    Move,
    /// The way the camera looks: it strafes, as over-the-shoulder aiming does.
    Aim,
}

/// How the third-person camera hangs: three numbers a script or the player
/// can set.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Boom {
    /// How far behind the shoulder, in metres.
    pub(crate) distance: f32,
    /// How far above the eyes the boom turns about, in metres.
    pub(crate) height: f32,
    /// How far to the right of the head (negative: the left), in metres.
    pub(crate) shoulder: f32,
}

impl Default for Boom {
    /// Over the right shoulder, close enough to see what the character sees,
    /// far enough to see the whole of them.
    fn default() -> Self {
        Self {
            distance: 3.0,
            height: 0.15,
            shoulder: 0.45,
        }
    }
}

/// How wide the camera is taken to be when keeping it out of walls: the
/// near plane's corners must stay in the open too.
const CAMERA_RADIUS: f32 = 0.22;

/// Seconds a view switch takes to travel between the eyes and the shoulder.
const SWITCH_SECONDS: f32 = 0.35;

/// How quickly a boom pulled in by a wall goes back out once it is clear:
/// the half-life of the gap, in seconds. Pulling in is at once.
const BOOM_RETURN: f32 = 0.25;

/// Closer than this to the head, the third-person camera would see inside
/// the body, so the body is not drawn in the picture (its shadow stays).
const TOO_CLOSE: f32 = 0.55;

/// Turning speeds of the body, radians a second: walking, and running or
/// aiming. People turn faster the faster they go.
const TURN_SLOW: f32 = 6.0;
const TURN_FAST: f32 = 12.0;

/// Closer than this to the head, the camera would see into it, so the head
/// and neck are left out of the picture (their shadow stays).
const HEAD_CLEAR: f32 = 0.3;

/// How far back the body is put behind the eyes in first person, so that
/// looking down does not look into the chest.
const FIRST_PERSON_BACK: f32 = 0.14;

/// The player's body and camera.
#[derive(Debug, Clone)]
pub(crate) struct Character {
    /// The model as loaded, for the renderer to upload.
    model: Arc<Scene>,
    body: Arc<Body>,
    animator: Animator,
    pub(crate) view: View,
    pub(crate) facing: Facing,
    pub(crate) boom: Boom,
    /// Which way the body faces, radians as the camera's yaw counts them
    /// (0 along -Z, growing to the left), this tick and the one before.
    yaw: f32,
    yaw_before: f32,
    /// How far into third person the camera is, 0 to 1, now and before.
    third: f32,
    third_before: f32,
    /// How much of the boom is clear of walls, 0 to 1, now and before.
    reach: f32,
    reach_before: f32,
    /// How fast the body moved along the way it faces, and fell, last tick.
    along_before: f32,
    falling_before: f32,
    grounded_before: bool,
    /// Poses of this tick and the one before, and buffers for a frame.
    pose: Vec<Pose>,
    pose_before: Vec<Pose>,
    blended: Vec<Pose>,
    placed: Vec<Transform>,
    skinning: Vec<Transform>,
    headless: Vec<Transform>,
    /// The bone that is the head, left out of the first-person picture.
    head: Option<usize>,
    /// The neck, left out with it: a sprint leans the body over, and looking
    /// down would otherwise show its stump below the eyes.
    neck: Option<usize>,
    /// The swing under way, if any.
    swing: Option<Swing>,
    /// The play that holds the guard up, while blocking.
    guard: Option<u32>,
}

/// A swing under way: what it plays, what it does, and what it has hit.
#[derive(Debug, Clone)]
struct Swing {
    play: u32,
    clip: u32,
    damage: f32,
    reach: f32,
    strike: lore_anim::Strike,
    /// The blade where it was on the last tick, in the world.
    was: Option<[[f32; 3]; 2]>,
    /// Who it has hit: each once.
    struck: Vec<lore_script::CharacterId>,
}

/// The most things one swing hits.
const MOST_STRUCK: usize = 8;

/// What a swing hit: who, how hard, and with which clip.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Blow {
    pub(crate) victim: lore_script::CharacterId,
    pub(crate) damage: f32,
    pub(crate) clip: String,
}

/// Something a swing can hit: who, where its feet are, how tall and how wide.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Struck {
    pub(crate) who: lore_script::CharacterId,
    pub(crate) feet: [f32; 3],
    pub(crate) height: f32,
    pub(crate) radius: f32,
}

/// What [`Character::figures`] gives for one frame: the body as seen, and
/// as it throws its shadow.
#[derive(Debug, Clone, Copy)]
pub(crate) struct Drawn<'a> {
    /// Takes the model's space to the world.
    pub(crate) transform: Transform,
    /// The skinning matrices for the picture (the head left out in first person).
    pub(crate) seen_joints: &'a [Transform],
    /// The skinning matrices for the shadow: the whole body.
    pub(crate) shadow_joints: &'a [Transform],
    /// Whether the body is drawn in the picture at all.
    pub(crate) seen: bool,
}

impl Character {
    /// The engine's own mannequin.
    ///
    /// # Errors
    ///
    /// Only if the file built into the engine is damaged, which a test rules out.
    pub(crate) fn mannequin() -> Result<Self, AssetError> {
        let scene = Scene::from_bytes("mannequin.glb", MANNEQUIN, None, LoadOptions::default())?;
        let clips = Clip::all_from(&scene);
        Ok(Self::new(scene, clips))
    }

    /// A copy of the engine's own mannequin, read once and then shared, or
    /// `None` (said in the log) if it could not be read.
    pub(crate) fn shared_mannequin() -> Option<Self> {
        static MANNEQUIN_BODY: std::sync::OnceLock<Option<Character>> = std::sync::OnceLock::new();
        MANNEQUIN_BODY
            .get_or_init(|| match Self::mannequin() {
                Ok(character) => Some(character),
                Err(error) => {
                    log::error!("The engine's own mannequin could not be read: {error}");
                    None
                }
            })
            .clone()
    }

    /// The female mannequin, playing the mannequin's clips.
    ///
    /// # Errors
    ///
    /// Only if a file built into the engine is damaged.
    pub(crate) fn female_mannequin() -> Result<Self, AssetError> {
        let scene = Scene::from_bytes(
            "mannequin_female.glb",
            MANNEQUIN_FEMALE,
            None,
            LoadOptions::default(),
        )?;
        Self::with_mannequin_clips(scene)
    }

    /// A body from a model of the game's own, playing its own clips and the
    /// mannequin's (which find its bones by name). Its own come first, so a
    /// clip of its own called `walk` is the walk.
    ///
    /// # Errors
    ///
    /// Only if a file built into the engine is damaged.
    pub(crate) fn with_mannequin_clips(scene: Scene) -> Result<Self, AssetError> {
        let missing = Skeleton::from_scene(&scene).missing_standard_bones();
        if !missing.is_empty() {
            let list: Vec<String> = missing
                .iter()
                .map(|(bone, near)| match near {
                    Some(own) => format!("{bone} (it has \"{own}\": rename it to {bone})"),
                    None => (*bone).to_owned(),
                })
                .collect();
            log::warn!(
                "This model has no bones called {}, so the mannequin's clips do not move \
                 them. Name its bones as LORE's mannequin does (Unreal's names) for every \
                 clip to play on it.",
                list.join(", ")
            );
        }
        let mannequin =
            Scene::from_bytes("mannequin.glb", MANNEQUIN, None, LoadOptions::default())?;
        let mut clips = Clip::all_from(&scene);
        for clip in Clip::all_from(&mannequin) {
            if !clips.iter().any(|own| own.name == clip.name) {
                clips.push(clip);
            }
        }
        Ok(Self::new(scene, clips))
    }

    /// The names of the clips the body has.
    pub(crate) fn clip_names(&self) -> impl Iterator<Item = String> + '_ {
        self.body.clips().iter().map(|clip| clip.name.clone())
    }

    /// The names of the clips a script asked for that are still showing.
    pub(crate) fn playing_names(&self) -> impl Iterator<Item = &str> + '_ {
        self.animator
            .playing()
            .iter()
            .filter(|p| !p.leaving)
            .filter_map(|p| self.body.clips().get(p.clip as usize))
            .map(|clip| clip.name.as_str())
    }

    /// Does what a script asked: plays or stops a clip, or swings.
    pub(crate) fn ask(&mut self, ask: &lore_script::AnimationAsk) {
        match ask {
            lore_script::AnimationAsk::Swing {
                clip,
                damage,
                reach,
                upper_body,
            } => {
                let Some(number) = self.body.clip(clip) else {
                    log::warn!("game.player:swing(\"{clip}\"): the body has no clip by that name.");
                    return;
                };
                let Some(strike) = self.body.strike(number) else {
                    log::warn!(
                        "game.player:swing(\"{clip}\"): that clip never strikes (no hand or foot \
                         moves fast in it, and it marks no hit_start and hit_end), so it can hit \
                         nothing. Swing with a blow, such as \"sword_attack\" or \"punch\"."
                    );
                    return;
                };
                // A swing already under way finishes first.
                if self.swing.is_some() {
                    return;
                }
                let how = lore_anim::Play {
                    layer: if *upper_body {
                        lore_anim::Layer::Upper
                    } else {
                        lore_anim::Layer::Body
                    },
                    fade: 0.08,
                    speed: 1.0,
                    looping: Some(false),
                };
                if let Some(play) = self.animator.play(&self.body, number, how) {
                    self.swing = Some(Swing {
                        play,
                        clip: number,
                        damage: *damage,
                        reach: *reach,
                        strike,
                        was: None,
                        struck: Vec::with_capacity(MOST_STRUCK),
                    });
                }
            }
            lore_script::AnimationAsk::Play {
                clip,
                fade,
                speed,
                looping,
                upper_body,
            } => {
                let Some(number) = self.body.clip(clip) else {
                    log::warn!("game.player:play(\"{clip}\"): the body has no clip by that name.");
                    return;
                };
                let how = lore_anim::Play {
                    layer: if *upper_body {
                        lore_anim::Layer::Upper
                    } else {
                        lore_anim::Layer::Body
                    },
                    fade: *fade,
                    speed: *speed,
                    looping: *looping,
                };
                self.animator.play(&self.body, number, how);
            }
            lore_script::AnimationAsk::Stop { clip, fade } => match clip {
                None => self.animator.stop(None, *fade),
                Some(name) => {
                    let Some(number) = self.body.clip(name) else {
                        return;
                    };
                    let ids: Vec<u32> = self
                        .animator
                        .playing()
                        .iter()
                        .filter(|p| p.clip == number)
                        .map(lore_anim::Playing::id_number)
                        .collect();
                    for id in ids {
                        self.animator.stop(Some(id), *fade);
                    }
                }
            },
        }
    }

    /// A body from a loaded model and the clips it plays (its own, or the
    /// mannequin's, which play on any body with the same bone names).
    pub(crate) fn new(scene: Scene, clips: Vec<Clip>) -> Self {
        let skeleton = Skeleton::from_scene(&scene);
        let body = Body::new(skeleton, clips);
        let animator = Animator::new(&body);
        let bones = body.skeleton().len();
        let joints: usize = body.skeleton().skins().iter().map(|s| s.joints.len()).sum();
        let rest = body.skeleton().rest().to_vec();
        let head = ["Head", "head", "mixamorig:Head"]
            .iter()
            .find_map(|n| body.skeleton().find(n));
        let neck = ["neck_01", "Neck", "neck", "mixamorig:Neck"]
            .iter()
            .find_map(|n| body.skeleton().find(n));
        Self {
            model: Arc::new(scene),
            animator,
            view: View::First,
            facing: Facing::Move,
            boom: Boom::default(),
            yaw: 0.0,
            yaw_before: 0.0,
            third: 0.0,
            third_before: 0.0,
            reach: 1.0,
            reach_before: 1.0,
            along_before: 0.0,
            falling_before: 0.0,
            grounded_before: true,
            pose_before: rest.clone(),
            blended: rest.clone(),
            pose: rest,
            placed: vec![lore_anim::math::IDENTITY; bones],
            skinning: vec![lore_anim::math::IDENTITY; joints],
            headless: vec![lore_anim::math::IDENTITY; joints],
            head,
            neck,
            swing: None,
            guard: None,
            body: Arc::new(body),
        }
    }

    /// Raises or lowers the guard: the body's block clip, held on the upper
    /// body, while `up`.
    pub(crate) fn guard(&mut self, up: bool) {
        match (up, self.guard) {
            (true, None) => {
                let clip = self
                    .body
                    .clip("sword_block")
                    .or_else(|| self.body.clip("shield_idle"));
                if let Some(clip) = clip {
                    let how = lore_anim::Play {
                        layer: lore_anim::Layer::Upper,
                        fade: 0.1,
                        speed: 1.0,
                        looping: Some(false),
                    };
                    self.guard = self.animator.play(&self.body, clip, how);
                }
            }
            (false, Some(play)) => {
                self.animator.stop(Some(play), 0.15);
                self.guard = None;
            }
            _ => {}
        }
    }

    /// Where every bone is in the world this tick, for a body with its feet
    /// at `feet`, into the bone buffer. Uses the tick's own pose, not a
    /// frame's, so what it hits is the same at any frame rate.
    fn place_in_world(&mut self, feet: [f32; 3]) -> Transform {
        let model_yaw = if self.body.facing() > 0.0 {
            self.yaw + std::f32::consts::PI
        } else {
            self.yaw
        };
        let (s, c) = (libm::sinf(model_yaw), libm::cosf(model_yaw));
        self.body.skeleton().place(&self.pose, &mut self.placed);
        [
            [c, 0.0, -s, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [s, 0.0, c, 0.0],
            [feet[0], feet[1], feet[2], 1.0],
        ]
    }

    /// Steps the swing under way: while its clip strikes, sweeps the blade
    /// (the striking hand or foot and `reach` past it) from where it was on
    /// the last tick to where it is, and gives each of `around` it passes
    /// through, once, to `hit`.
    pub(crate) fn swing_through(
        &mut self,
        feet: [f32; 3],
        around: &[Struck],
        mut hit: impl FnMut(Blow),
    ) {
        let Some(swing) = &self.swing else { return };
        let Some(playing) = self
            .animator
            .playing()
            .iter()
            .find(|p| p.id_number() == swing.play)
            .copied()
        else {
            self.swing = None;
            return;
        };
        let body = Arc::clone(&self.body);
        let clip = &body.clips()[swing.clip as usize];
        let time = clip.wrap(playing.time);
        let striking = time >= swing.strike.start && time <= swing.strike.end + 1.0 / 30.0;
        let (bone, parent, reach) = (swing.strike.bone, swing.strike.parent, swing.reach);
        let world = self.place_in_world(feet);
        let at = |t: &Transform| lore_anim::math::apply(&world, lore_anim::math::origin(t));
        let (hand, elbow) = (at(&self.placed[bone]), at(&self.placed[parent]));
        let along: [f32; 3] = std::array::from_fn(|i| hand[i] - elbow[i]);
        let length = along.iter().map(|v| v * v).sum::<f32>().sqrt().max(1e-4);
        let tip: [f32; 3] = std::array::from_fn(|i| hand[i] + along[i] / length * reach);
        let name = clip.name.clone();
        let Some(swing) = &mut self.swing else { return };
        let blade = [hand, tip];
        if striking {
            let was = swing.was.unwrap_or(blade);
            // Points along the blade, at the last tick, between, and now.
            for step in 0..=2 {
                let t = step as f32 / 2.0;
                for k in 0..=4 {
                    let u = k as f32 / 4.0;
                    let point: [f32; 3] = std::array::from_fn(|i| {
                        let a = was[0][i] + (was[1][i] - was[0][i]) * u;
                        let b = blade[0][i] + (blade[1][i] - blade[0][i]) * u;
                        a + (b - a) * t
                    });
                    for target in around {
                        if swing.struck.contains(&target.who) || swing.struck.len() >= MOST_STRUCK {
                            continue;
                        }
                        if inside(point, target) {
                            swing.struck.push(target.who);
                            hit(Blow {
                                victim: target.who,
                                damage: swing.damage,
                                clip: name.clone(),
                            });
                        }
                    }
                }
            }
        }
        swing.was = Some(blade);
    }

    /// The model, for the renderer to upload once.
    pub(crate) fn model(&self) -> &Arc<Scene> {
        &self.model
    }

    /// Which way the body faces now.
    pub(crate) fn yaw(&self) -> f32 {
        self.yaw
    }

    /// Puts the body facing `yaw` at once, as a teleport does.
    pub(crate) fn face(&mut self, yaw: f32) {
        self.yaw = yaw;
        self.yaw_before = yaw;
    }

    /// One tick: turns the body, moves its clips on, swings the camera
    /// between views and keeps the boom out of walls.
    ///
    /// `look` is the camera's yaw and pitch after this tick's turning.
    /// Each moment a clip passes (a foot coming down) is handed to `heard`.
    pub(crate) fn tick(
        &mut self,
        player: &Player,
        look: (f32, f32),
        level: &Level,
        step: f32,
        mut heard: impl FnMut(&str, &lore_anim::Marker),
    ) {
        let dt = if step.is_finite() { step.max(0.0) } else { 0.0 };
        self.yaw_before = self.yaw;
        self.third_before = self.third;
        self.reach_before = self.reach;
        std::mem::swap(&mut self.pose_before, &mut self.pose);

        // Which way to face: in first person and when aiming, the camera's
        // way; otherwise the way the body moves, and where it was while it
        // stands.
        let [vx, vy, vz] = player.velocity;
        let speed = (vx * vx + vz * vz).sqrt();
        let aiming = self.view == View::First || self.facing == Facing::Aim;
        let goal = if aiming {
            Some(look.0)
        } else if speed > 0.3 {
            Some(libm::atan2f(-vx, -vz))
        } else {
            None
        };
        if let Some(goal) = goal {
            let run = (speed / 4.0).min(1.0);
            let rate = if aiming {
                TURN_FAST * 1.5
            } else {
                TURN_SLOW + (TURN_FAST - TURN_SLOW) * run
            };
            self.yaw = turn_toward(self.yaw, goal, rate * dt);
        }

        // What the animator reads: speed and direction against the way the
        // body faces, footing, landing and leaning.
        let (sin, cos) = (libm::sinf(self.yaw), libm::cosf(self.yaw));
        let along = -vx * sin - vz * cos;
        let across = vx * cos - vz * sin;
        let direction = if speed > 1e-3 {
            [across / speed, along / speed]
        } else {
            [0.0, 1.0]
        };
        let turning = angle_between(self.yaw_before, self.yaw) / dt.max(1e-4);
        let footing = if player.swimming {
            Footing::Swimming
        } else if player.grounded {
            Footing::Ground
        } else {
            Footing::Air(vy)
        };
        let landed = if player.grounded && !self.grounded_before {
            (-self.falling_before).max(0.0)
        } else {
            0.0
        };
        let moving = Moving {
            speed,
            direction,
            crouched: player.crouched,
            footing,
            landed,
            accel: [(along - self.along_before) / dt.max(1e-4), -speed * turning],
        };
        self.along_before = along;
        self.falling_before = vy;
        self.grounded_before = player.grounded;
        let body = &self.body;
        self.animator.step(body, &moving, dt, |clip, marker| {
            if let Some(clip) = body.clips().get(clip as usize) {
                heard(&clip.name, marker);
            }
        });
        self.animator.pose(&self.body, &mut self.pose);

        // The view travels, eased, between the eyes and the shoulder.
        let toward = if self.view == View::Third { 1.0 } else { 0.0 };
        let move_by = dt / SWITCH_SECONDS;
        self.third = if self.third < toward {
            (self.third + move_by).min(toward)
        } else {
            (self.third - move_by).max(toward)
        };

        // How much of the boom is clear: in at once, out slowly.
        let clear = self.clear_reach(player.eye_position(), look, level);
        self.reach = if clear < self.reach {
            clear
        } else {
            self.reach + (clear - self.reach) * (dt / (dt + BOOM_RETURN))
        };
    }

    /// How much of the boom is clear of walls from the eyes at `eye`
    /// looking along `look`, 0 to 1.
    fn clear_reach(&self, eye: [f32; 3], look: (f32, f32), level: &Level) -> f32 {
        let (shoulder, end) = self.free_boom(eye, look, level);
        level.ball_cast(shoulder, end, CAMERA_RADIUS).unwrap_or(1.0)
    }

    /// The boom as far out to the shoulder as is clear, and its far end
    /// from there: against a wall on the right, the camera hangs nearer the
    /// head rather than giving up the view.
    fn free_boom(&self, eye: [f32; 3], look: (f32, f32), level: &Level) -> ([f32; 3], [f32; 3]) {
        let (pivot, shoulder, end) = self.boom_points(eye, look);
        // Stopped a little short of what it touched, so the boom cast from
        // there does not start against it.
        let side = level
            .ball_cast(pivot, shoulder, CAMERA_RADIUS)
            .map_or(1.0, |side| {
                let length = self.boom.shoulder.abs().max(1e-3);
                (side - 0.05 / length).max(0.0)
            });
        let moved: [f32; 3] = std::array::from_fn(|i| (shoulder[i] - pivot[i]) * (1.0 - side));
        (
            std::array::from_fn(|i| shoulder[i] - moved[i]),
            std::array::from_fn(|i| end[i] - moved[i]),
        )
    }

    /// The boom's pivot above the head, the point beside it over the
    /// shoulder, and where the camera hangs with nothing in the way.
    fn boom_points(
        &self,
        eye: [f32; 3],
        (yaw, pitch): (f32, f32),
    ) -> ([f32; 3], [f32; 3], [f32; 3]) {
        let (sy, cy) = (libm::sinf(yaw), libm::cosf(yaw));
        let (sp, cp) = (libm::sinf(pitch), libm::cosf(pitch));
        let forward = [-sy * cp, sp, -cy * cp];
        let right = [cy, 0.0, -sy];
        let pivot = [eye[0], eye[1] + self.boom.height, eye[2]];
        let shoulder = [
            pivot[0] + right[0] * self.boom.shoulder,
            pivot[1],
            pivot[2] + right[2] * self.boom.shoulder,
        ];
        let end = [
            shoulder[0] - forward[0] * self.boom.distance,
            shoulder[1] - forward[1] * self.boom.distance,
            shoulder[2] - forward[2] * self.boom.distance,
        ];
        (pivot, shoulder, end)
    }

    /// Where the camera is for a frame `alpha` of the way from the last
    /// tick to this one, with the eyes at `eye` and the look at `look`
    /// (both already blended and turned for this frame).
    ///
    /// The boom is checked against walls again for this frame's look, so a
    /// quick turn of the mouse between ticks never swings it into one.
    pub(crate) fn camera(
        &self,
        alpha: f32,
        eye: [f32; 3],
        look: (f32, f32),
        level: &Level,
    ) -> [f32; 3] {
        let third = smooth(self.third_before + (self.third - self.third_before) * alpha);
        if third <= 0.0 {
            return eye;
        }
        let reach = self.reach_before + (self.reach - self.reach_before) * alpha;
        let reach = reach.min(self.clear_reach(eye, look, level));
        let (shoulder, end) = self.free_boom(eye, look, level);
        let hung: [f32; 3] = std::array::from_fn(|i| shoulder[i] + (end[i] - shoulder[i]) * reach);
        [
            eye[0] + (hung[0] - eye[0]) * third,
            eye[1] + (hung[1] - eye[1]) * third,
            eye[2] + (hung[2] - eye[2]) * third,
        ]
    }

    /// The body for a frame `alpha` of the way from the last tick to this
    /// one, with its feet at `feet` and the camera at `camera`.
    pub(crate) fn figures(&mut self, alpha: f32, feet: [f32; 3], camera: [f32; 3]) -> Drawn<'_> {
        for ((out, a), b) in self
            .blended
            .iter_mut()
            .zip(&self.pose_before)
            .zip(&self.pose)
        {
            *out = lore_anim::math::blend_pose(a, b, alpha);
        }
        let yaw = self.yaw_before + angle_between(self.yaw_before, self.yaw) * alpha;
        let third = smooth(self.third_before + (self.third - self.third_before) * alpha);
        // The model faces +Z or -Z; the camera's yaw 0 looks along -Z.
        let model_yaw = if self.body.facing() > 0.0 {
            yaw + std::f32::consts::PI
        } else {
            yaw
        };
        let (s, c) = (libm::sinf(model_yaw), libm::cosf(model_yaw));
        let skeleton = self.body.skeleton();
        skeleton.place(&self.blended, &mut self.placed);
        // In first person the body stands with its head a little behind the
        // eyes. It is the head that is kept there, not the feet: a sprint or
        // a swing leans the body forward, and with the feet held still the
        // shoulders would come up in front of the camera.
        let first = 1.0 - third;
        let back = FIRST_PERSON_BACK * first;
        let (fs, fc) = (libm::sinf(yaw), libm::cosf(yaw));
        let [hx, _, hz, _] = self
            .head
            .and_then(|head| self.placed.get(head))
            .map_or([0.0; 4], |placed| placed[3]);
        let head_x = c * hx + s * hz;
        let head_z = -s * hx + c * hz;
        let at = [
            feet[0] + fs * back - head_x * first,
            feet[1],
            feet[2] + fc * back - head_z * first,
        ];
        let transform = [
            [c, 0.0, -s, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [s, 0.0, c, 0.0],
            [at[0], at[1], at[2], 1.0],
        ];
        let mut written = 0;
        for skin in 0..skeleton.skins().len() {
            written += skeleton.skinning(skin, &self.placed, &mut self.skinning[written..]);
        }
        // Where the head really is this frame, leaning or not.
        let head_place = match self.head.and_then(|head| self.placed.get(head)) {
            Some(placed) => {
                let [x, y, z, _] = placed[3];
                [at[0] + c * x + s * z, at[1] + y, at[2] - s * x + c * z]
            }
            None => [feet[0], feet[1] + 1.6, feet[2]],
        };
        let near: f32 = (0..3)
            .map(|i| (camera[i] - head_place[i]).powi(2))
            .sum::<f32>()
            .sqrt();
        // The picture's copy leaves the head and neck out in first person and
        // while a view switch has the camera close to them: squeezed to
        // nothing, so nothing of the inside shows. It goes by where the
        // camera is, not how far the switch has got, so the head is never
        // seen to vanish and come back however quickly the view is changed.
        let hide = self.neck.or(self.head);
        if let (Some(hide), true) = (hide, third <= 0.0 || near < HEAD_CLEAR) {
            self.blended[hide].scale = [1e-3; 3];
            skeleton.place(&self.blended, &mut self.placed);
            let mut written = 0;
            for skin in 0..skeleton.skins().len() {
                written += skeleton.skinning(skin, &self.placed, &mut self.headless[written..]);
            }
        } else {
            self.headless.copy_from_slice(&self.skinning);
        }
        Drawn {
            transform,
            seen_joints: &self.headless,
            shadow_joints: &self.skinning,
            // In third person, hidden when the camera is pressed against
            // the head by a wall; in first person always drawn, headless.
            seen: third < 0.5 || near > TOO_CLOSE,
        }
    }
}

/// Whether `point` is inside `target`'s upright capsule.
fn inside(point: [f32; 3], target: &Struck) -> bool {
    let low = target.feet[1] + target.radius;
    let high = target.feet[1] + target.height - target.radius;
    let y = point[1].clamp(low, high.max(low));
    let d: f32 = [
        point[0] - target.feet[0],
        point[1] - y,
        point[2] - target.feet[2],
    ]
    .iter()
    .map(|v| v * v)
    .sum();
    d <= target.radius * target.radius
}

/// The signed turn from `from` to `to`, the short way, -π to π.
fn angle_between(from: f32, to: f32) -> f32 {
    use std::f32::consts::{PI, TAU};
    let d = (to - from).rem_euclid(TAU);
    if d > PI { d - TAU } else { d }
}

/// `from` turned toward `to` by at most `most` radians, kept within 0 to 2π.
pub(crate) fn turn_toward(from: f32, to: f32, most: f32) -> f32 {
    let d = angle_between(from, to);
    let turned = from + d.clamp(-most, most);
    turned.rem_euclid(std::f32::consts::TAU)
}

/// An ease in and out.
fn smooth(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

#[cfg(test)]
mod tests {
    use super::*;
    use lore_physics::MoveSettings;

    fn standing() -> Player {
        Player::at([0.0, 0.0, 0.0], &MoveSettings::default())
    }

    /// A wall across behind the player (+Z), two metres back.
    fn wall_behind() -> Level {
        Level::from_triangles(&[
            [[-5.0, 0.0, 2.0], [5.0, 0.0, 2.0], [5.0, 10.0, 2.0]],
            [[-5.0, 0.0, 2.0], [5.0, 10.0, 2.0], [-5.0, 10.0, 2.0]],
        ])
    }

    #[test]
    fn the_built_in_mannequin_loads_with_clips_and_a_skeleton() {
        let character = Character::mannequin().unwrap();
        assert!(character.body.clips().len() >= 80);
        assert!(character.body.skeleton().skins().len() == 1);
        assert!(character.head.is_some());
    }

    #[test]
    fn the_body_turns_the_way_it_walks_at_a_speed() {
        let mut character = Character::mannequin().unwrap();
        character.view = View::Third;
        let mut player = standing();
        // Running to the right (+X): yaw -90°.
        player.velocity = [4.0, 0.0, 0.0];
        let level = Level::from_triangles(&[]);
        character.tick(&player, (0.0, 0.0), &level, 1.0 / 60.0, |_, _| {});
        let first = character.yaw();
        let turned = angle_between(0.0, first);
        assert!(
            turned < 0.0 && turned > -0.25,
            "turned {turned} in one tick"
        );
        for _ in 0..60 {
            character.tick(&player, (0.0, 0.0), &level, 1.0 / 60.0, |_, _| {});
        }
        assert!((angle_between(character.yaw(), -std::f32::consts::FRAC_PI_2)).abs() < 1e-3);
    }

    #[test]
    fn aiming_faces_the_camera_and_first_person_always_does() {
        let mut character = Character::mannequin().unwrap();
        let mut player = standing();
        player.velocity = [4.0, 0.0, 0.0];
        let level = Level::from_triangles(&[]);
        for _ in 0..60 {
            character.tick(&player, (1.0, 0.0), &level, 1.0 / 60.0, |_, _| {});
        }
        assert!(angle_between(character.yaw(), 1.0).abs() < 1e-3);
    }

    #[test]
    fn the_camera_pulls_in_at_once_against_a_wall_and_eases_back_out() {
        let mut character = Character::mannequin().unwrap();
        character.view = View::Third;
        let player = standing();
        let wall = wall_behind();
        let open = Level::from_triangles(&[]);
        let eye = player.eye_position();
        for _ in 0..30 {
            character.tick(&player, (0.0, 0.0), &wall, 1.0 / 60.0, |_, _| {});
        }
        let camera = character.camera(1.0, eye, (0.0, 0.0), &wall);
        // Never at or past the wall, which is 2 m behind.
        assert!(camera[2] < 2.0 - CAMERA_RADIUS + 0.01, "{camera:?}");
        assert!(
            camera[2] > 1.0,
            "pulled in only as far as the wall: {camera:?}"
        );
        // Out in the open it goes back to its full length over a while.
        character.tick(&player, (0.0, 0.0), &open, 1.0 / 60.0, |_, _| {});
        let next = character.camera(1.0, eye, (0.0, 0.0), &open);
        assert!(next[2] < 2.5, "not all at once: {next:?}");
        for _ in 0..120 {
            character.tick(&player, (0.0, 0.0), &open, 1.0 / 60.0, |_, _| {});
        }
        let out = character.camera(1.0, eye, (0.0, 0.0), &open);
        assert!((out[2] - 3.0).abs() < 0.05, "{out:?}");
    }

    #[test]
    fn a_wall_at_the_shoulder_moves_the_camera_in_not_up_onto_the_head() {
        let mut character = Character::mannequin().unwrap();
        character.view = View::Third;
        let player = standing();
        // A wall to the right (+X), 0.4 m from the eyes.
        let wall = Level::from_triangles(&[
            [[0.4, 0.0, -5.0], [0.4, 0.0, 5.0], [0.4, 10.0, 5.0]],
            [[0.4, 0.0, -5.0], [0.4, 10.0, 5.0], [0.4, 10.0, -5.0]],
        ]);
        for _ in 0..60 {
            character.tick(&player, (0.0, 0.0), &wall, 1.0 / 60.0, |_, _| {});
        }
        let camera = character.camera(1.0, player.eye_position(), (0.0, 0.0), &wall);
        assert!(
            (camera[2] - 3.0).abs() < 0.05,
            "still the whole way back: {camera:?}"
        );
        assert!(
            camera[0] < 0.4 - CAMERA_RADIUS + 0.01,
            "out of the wall: {camera:?}"
        );
    }

    #[test]
    fn switching_view_travels_rather_than_cuts() {
        let mut character = Character::mannequin().unwrap();
        let player = standing();
        let level = Level::from_triangles(&[]);
        let eye = player.eye_position();
        character.view = View::Third;
        character.tick(&player, (0.0, 0.0), &level, 1.0 / 60.0, |_, _| {});
        let a = character.camera(1.0, eye, (0.0, 0.0), &level);
        assert!(
            a[2] > 0.0 && a[2] < 0.5,
            "a little way back after one tick: {a:?}"
        );
        for _ in 0..30 {
            character.tick(&player, (0.0, 0.0), &level, 1.0 / 60.0, |_, _| {});
        }
        let b = character.camera(1.0, eye, (0.0, 0.0), &level);
        assert!((b[2] - 3.0).abs() < 0.05, "{b:?}");
        // Behind and over the right shoulder.
        assert!(b[0] > 0.3 && b[1] > eye[1]);
    }

    #[test]
    fn first_person_draws_the_body_without_its_head_and_the_shadow_whole() {
        let mut character = Character::mannequin().unwrap();
        let player = standing();
        let level = Level::from_triangles(&[]);
        character.tick(&player, (0.0, 0.0), &level, 1.0 / 60.0, |_, _| {});
        let eye = player.eye_position();
        let drawn = character.figures(1.0, player.position, eye);
        assert!(drawn.seen);
        assert_ne!(
            drawn.seen_joints, drawn.shadow_joints,
            "the head is left out"
        );
    }

    #[test]
    fn a_sprint_in_first_person_keeps_the_head_behind_the_eyes() {
        let mut character = Character::mannequin().unwrap();
        let mut player = standing();
        player.grounded = true;
        // Sprinting the way the camera looks (-Z), which leans the body over.
        player.velocity = [0.0, 0.0, -7.5];
        let level = Level::from_triangles(&[]);
        let head = character.head.unwrap();
        for tick in 0..120 {
            character.tick(&player, (0.0, 0.0), &level, 1.0 / 60.0, |_, _| {});
            if tick < 30 {
                continue;
            }
            let eye = player.eye_position();
            let transform = character.figures(1.0, player.position, eye).transform;
            let [mx, _, mz, at] = transform;
            // Where the head is in the pose itself, before the picture's
            // copy squeezes it away.
            let mut placed = character.placed.clone();
            character
                .body
                .skeleton()
                .place(&character.pose, &mut placed);
            let [hx, _, hz, _] = placed[head][3];
            let world = [
                at[0] + mx[0] * hx + mz[0] * hz,
                at[2] + mx[2] * hx + mz[2] * hz,
            ];
            assert!(
                (world[1] - FIRST_PERSON_BACK).abs() < 1e-3 && world[0].abs() < 1e-3,
                "tick {tick}: the head is at {world:?}, not just behind the eyes"
            );
        }
    }

    #[test]
    fn the_head_and_neck_are_left_out_only_while_the_camera_is_near_them() {
        let mut character = Character::mannequin().unwrap();
        let player = standing();
        let level = Level::from_triangles(&[]);
        let eye = player.eye_position();
        character.tick(&player, (0.0, -1.2), &level, 1.0 / 60.0, |_, _| {});
        let first = character.figures(1.0, player.position, eye);
        assert_ne!(first.seen_joints, first.shadow_joints, "first person");
        // Halfway through a switch the camera is well clear: the head is drawn.
        character.view = View::Third;
        for _ in 0..10 {
            character.tick(&player, (0.0, 0.0), &level, 1.0 / 60.0, |_, _| {});
        }
        let camera = character.camera(1.0, eye, (0.0, 0.0), &level);
        assert!(character.third > 0.0 && character.third < 1.0);
        let midway = character.figures(1.0, player.position, camera);
        assert_eq!(midway.seen_joints, midway.shadow_joints, "midway");
        // The same frame with the camera at the eyes leaves it out again.
        let close = character.figures(1.0, player.position, eye);
        assert_ne!(close.seen_joints, close.shadow_joints, "camera at the eyes");
    }

    #[test]
    fn the_same_ticks_turn_and_pose_the_body_the_same() {
        let run = || {
            let mut character = Character::mannequin().unwrap();
            character.view = View::Third;
            let mut player = standing();
            let level = wall_behind();
            for tick in 0..200u32 {
                let t = tick as f32 / 60.0;
                player.velocity = [libm::sinf(t) * 3.0, 0.0, libm::cosf(t) * 3.0];
                player.grounded = tick % 70 > 10;
                character.tick(&player, (t, -0.2), &level, 1.0 / 60.0, |_, _| {});
            }
            (character.yaw(), character.pose.clone(), character.reach)
        };
        assert_eq!(run(), run());
    }
}
