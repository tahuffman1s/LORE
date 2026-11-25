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

/// How much further back it goes when looking straight down, eased in as
/// the look tips down, so the camera is never over the collar and sees the
/// legs and feet past the chest instead of into where the neck was.
const LOOK_DOWN_BACK: f32 = 0.35;

/// Standing still, how far the look may turn from the way the body faces
/// before the body steps round after it, radians (60°). People turn their
/// head and shoulders first and only move their feet past that.
const TURN_START: f32 = 1.05;

/// Standing still, the furthest the body is ever left behind the look,
/// radians (90°): a quick turn drags it round that far behind at once.
const TURN_MOST: f32 = std::f32::consts::FRAC_PI_2;

/// How fast the body steps round on the spot, radians a second, and how
/// fast its turning clip plays to keep the feet with it.
const TURN_IN_PLACE: f32 = 4.0;
const TURN_CLIP_SPEED: f32 = 1.8;

/// The clips that step a standing body round to its left and right.
const TURN_LEFT: &str = "turn_left";
const TURN_RIGHT: &str = "turn_right";

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
    /// The bones of the right hand and the left, for what is held in them.
    hands: [Option<usize>; 2],
    /// Where the body was last drawn, for placing what it holds.
    drawn_at: Transform,
    /// Where each hand closes, from its bone: the wrist is the bone, and
    /// the palm is most of the way to the knuckle of the middle finger.
    palms: [[f32; 3]; 2],
    /// How a held thing is turned in each hand: see [`grip`].
    grips: [Transform; 2],
    /// The arm that ends in each hand, for reaching a weapon held.
    arms: [Option<lore_anim::ik::Limb>; 2],
    /// The weapon held ready this frame, if one is: where the hands go on
    /// it. Set with [`set_hold`](Self::set_hold) before drawing.
    hold: Option<Hold>,
    /// The neck, left out with it: a sprint leans the body over, and looking
    /// down would otherwise show its stump below the eyes.
    neck: Option<usize>,
    /// The swing under way, if any.
    swing: Option<Swing>,
    /// The play that holds the guard up, while blocking.
    guard: Option<u32>,
    /// Whether a standing body is stepping round to face the look, and the
    /// play of the turning clip that moves its feet.
    turning: bool,
    turn_play: Option<u32>,
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
    /// Where the weapon held ready is in the world, if one is: what takes
    /// its model (grip at the origin, muzzle along -Z) there.
    pub(crate) weapon: Option<Transform>,
    /// The skinning matrices for the picture (the head left out in first person).
    pub(crate) seen_joints: &'a [Transform],
    /// The skinning matrices for the shadow: the whole body.
    pub(crate) shadow_joints: &'a [Transform],
    /// Whether the body is drawn in the picture at all.
    pub(crate) seen: bool,
}

/// A weapon held ready, as the body holds it: where the left hand goes on
/// it, in the weapon's own space (grip at the origin, muzzle along -Z, top
/// along +Y), or `None` to hold it in the right hand alone; and whether
/// that hand cups the right one (a pistol) rather than holding a fore-end.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Hold {
    pub(crate) support: Option<[f32; 3]>,
    pub(crate) cupped: bool,
}

/// Where a weapon is held in first person, in metres right, up and ahead
/// of the eyes: low, a little right of the middle, near enough for both
/// arms to reach.
const HELD_AT_EYES: [f32; 3] = [0.08, -0.13, 0.19];

/// Where a weapon is held in third person, from the head: at the right
/// shoulder, pointing where the body faces.
const HELD_AT_SHOULDER: [f32; 3] = [0.08, -0.22, 0.22];

/// How far the chest turns to bring the left shoulder forward when both
/// hands hold a weapon, in radians: a shooter's stance, side-on to it.
const STANCE_TURN: f32 = 0.75;

/// What turns a weapon model (grip at the origin, muzzle along -Z, top
/// along +Y) into the space of a held thing (length along +Y leaving the
/// thumb's side of the fist, square to the fingers along +X): the muzzle
/// points where the fingers do, and the top is on the thumb's side.
pub(crate) const WEAPON_IN_HAND: Transform = [
    [0.0, 0.0, 1.0, 0.0],
    [0.0, 1.0, 0.0, 0.0],
    [-1.0, 0.0, 0.0, 0.0],
    [0.0, 0.0, 0.0, 1.0],
];

/// A weapon held at `offset` (metres right, up, ahead) from `from`,
/// looking along `yaw` and `pitch`, turned with the look.
pub(crate) fn held_from(from: [f32; 3], (yaw, pitch): (f32, f32), offset: [f32; 3]) -> Transform {
    let (sin, cos) = (libm::sinf(yaw), libm::cosf(yaw));
    let (sin_pitch, cos_pitch) = (libm::sinf(pitch), libm::cosf(pitch));
    let ahead = [-sin * cos_pitch, sin_pitch, -cos * cos_pitch];
    let right = [cos, 0.0, -sin];
    let up = cross(right, ahead);
    let [r, u, a] = offset;
    let at: [f32; 3] = std::array::from_fn(|i| from[i] + right[i] * r + up[i] * u + ahead[i] * a);
    [
        [right[0], right[1], right[2], 0.0],
        [up[0], up[1], up[2], 0.0],
        [-ahead[0], -ahead[1], -ahead[2], 0.0],
        [at[0], at[1], at[2], 1.0],
    ]
}

fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn unit3(v: [f32; 3]) -> [f32; 3] {
    let long = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
    if long > 1e-6 {
        v.map(|part| part / long)
    } else {
        [0.0, 1.0, 0.0]
    }
}

/// The turn of `transform` undone, its place left out: for a pure turn,
/// its transpose.
fn turn_undone(transform: &Transform) -> Transform {
    let mut out = lore_anim::math::IDENTITY;
    for (column, out_column) in out.iter_mut().enumerate().take(3) {
        for (row, cell) in out_column.iter_mut().enumerate().take(3) {
            *cell = transform[row][column];
        }
    }
    out
}

/// A frame whose +X is `x` and +Y as near `y` as is square to it, at `at`.
fn frame_of(x: [f32; 3], y: [f32; 3], at: [f32; 3]) -> Transform {
    let x = unit3(x);
    let dot = x[0] * y[0] + x[1] * y[1] + x[2] * y[2];
    let y = unit3(std::array::from_fn(|i| y[i] - x[i] * dot));
    let z = cross(x, y);
    [
        [x[0], x[1], x[2], 0.0],
        [y[0], y[1], y[2], 0.0],
        [z[0], z[1], z[2], 0.0],
        [at[0], at[1], at[2], 1.0],
    ]
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
            .filter(|p| !p.leaving && Some(p.id_number()) != self.turn_play)
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
        let hands = [
            ["hand_r", "RightHand", "mixamorig:RightHand", "Hand.R"],
            ["hand_l", "LeftHand", "mixamorig:LeftHand", "Hand.L"],
        ]
        .map(|names| names.iter().find_map(|n| body.skeleton().find(n)));
        // Where a finger's first bone is from the wrist, by any of its names.
        let finger = |names: [&str; 3]| {
            names
                .iter()
                .find_map(|n| body.skeleton().find(n))
                .and_then(|bone| body.skeleton().rest().get(bone))
                .map(|bone| bone.translation)
        };
        let fingers = [
            [
                [
                    "middle_01_r",
                    "RightHandMiddle1",
                    "mixamorig:RightHandMiddle1",
                ],
                ["thumb_01_r", "RightHandThumb1", "mixamorig:RightHandThumb1"],
                ["pinky_01_r", "RightHandPinky1", "mixamorig:RightHandPinky1"],
            ],
            [
                [
                    "middle_01_l",
                    "LeftHandMiddle1",
                    "mixamorig:LeftHandMiddle1",
                ],
                ["thumb_01_l", "LeftHandThumb1", "mixamorig:LeftHandThumb1"],
                ["pinky_01_l", "LeftHandPinky1", "mixamorig:LeftHandPinky1"],
            ],
        ]
        .map(|hand| hand.map(finger));
        let palms = fingers
            .map(|[middle, ..]| middle.map_or([0.0; 3], |middle| middle.map(|part| part * PALM)));
        // A hand with no fingers to go by holds a thing as its bone lies.
        let grips = fingers.map(|[middle, thumb, little]| {
            middle
                .zip(thumb)
                .zip(little)
                .and_then(|((middle, thumb), little)| grip(middle, thumb, little))
                .unwrap_or(lore_anim::math::IDENTITY)
        });
        let arms = hands.map(|hand| {
            hand.and_then(|hand| lore_anim::ik::Limb::ending_in(body.skeleton(), hand))
        });
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
            hands,
            palms,
            grips,
            arms,
            hold: None,
            drawn_at: lore_anim::math::IDENTITY,
            neck,
            swing: None,
            guard: None,
            turning: false,
            turn_play: None,
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
        // stands. Standing on the ground, the body lets the look turn a
        // little way on its own and then steps round after it.
        let [vx, vy, vz] = player.velocity;
        let speed = (vx * vx + vz * vz).sqrt();
        let aiming = self.view == View::First || self.facing == Facing::Aim;
        // A swing goes where the camera looks, so it turns the body at once.
        let standing = speed <= 0.3 && player.grounded && !player.swimming && self.swing.is_none();
        if aiming && standing {
            self.turn_in_place(look.0, dt);
        } else {
            self.end_turn();
        }
        let goal = if aiming && standing {
            None
        } else if aiming {
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

    /// Turns a standing body toward `goal` as people do: not at all while
    /// the look is within [`TURN_START`] of the way it faces, then round on
    /// the spot with its feet stepping, until it faces the look again.
    fn turn_in_place(&mut self, goal: f32, dt: f32) {
        let gap = angle_between(self.yaw, goal);
        if gap.abs() > TURN_MOST {
            self.yaw = turn_toward(self.yaw, goal, gap.abs() - TURN_MOST);
        }
        let gap = angle_between(self.yaw, goal);
        if !self.turning && gap.abs() > TURN_START {
            self.turning = true;
            // The feet step only when nothing a script or a swing asked
            // for is playing; the body turns either way.
            let free = self.swing.is_none()
                && self
                    .animator
                    .playing()
                    .iter()
                    .all(|p| p.leaving || Some(p.id_number()) == self.turn_play);
            // Yaw grows to the left.
            let name = if gap > 0.0 { TURN_LEFT } else { TURN_RIGHT };
            if let (true, Some(clip)) = (free, self.body.clip(name)) {
                let how = lore_anim::Play {
                    layer: lore_anim::Layer::Body,
                    fade: 0.15,
                    speed: TURN_CLIP_SPEED,
                    looping: Some(false),
                };
                self.turn_play = self.animator.play(&self.body, clip, how);
            }
        }
        if self.turning {
            self.yaw = turn_toward(self.yaw, goal, TURN_IN_PLACE * dt);
            if angle_between(self.yaw, goal).abs() < 0.02 {
                self.turning = false;
            }
        }
    }

    /// Stops stepping round on the spot: the body has started to move, or
    /// left the ground, or stopped following the look.
    fn end_turn(&mut self) {
        self.turning = false;
        if let Some(play) = self.turn_play.take() {
            self.animator.stop(Some(play), 0.15);
        }
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
        let (mut pivot, mut shoulder, mut end) = self.boom_points(eye, look);
        // Under a low ceiling the pivot rises only as far as there is room
        // over the eyes, and the whole boom hangs lower with it.
        let rise = pivot[1] - eye[1];
        if rise.abs() > 1e-4
            && let Some(clear) = level.ball_cast(eye, pivot, CAMERA_RADIUS)
        {
            let lowered = rise * (1.0 - (clear - 0.01 / rise.abs()).max(0.0));
            pivot[1] -= lowered;
            shoulder[1] -= lowered;
            end[1] -= lowered;
        }
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
    /// one, with its feet at `feet` and the camera at `camera`, looking
    /// along `look`: yaw, and pitch up (negative: down), in radians.
    pub(crate) fn figures(
        &mut self,
        alpha: f32,
        feet: [f32; 3],
        camera: [f32; 3],
        look: (f32, f32),
    ) -> Drawn<'_> {
        let (look_yaw, pitch) = look;
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
        let down = libm::sinf((-pitch).clamp(0.0, std::f32::consts::FRAC_PI_2));
        let back = (FIRST_PERSON_BACK + LOOK_DOWN_BACK * down * down) * first;
        // Back from the look, not the body: standing, the body may face a
        // little away from it, and its shoulders must still be behind.
        let (fs, fc) = (libm::sinf(look_yaw), libm::cosf(look_yaw));
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
        // Where the head really is this frame, leaning or not.
        let head_place = match self.head.and_then(|head| self.placed.get(head)) {
            Some(placed) => {
                let [x, y, z, _] = placed[3];
                [at[0] + c * x + s * z, at[1] + y, at[2] - s * x + c * z]
            }
            None => [feet[0], feet[1] + 1.6, feet[2]],
        };
        // A weapon held ready: before the eyes in first person, at the
        // shoulder in third, and the arms reach it.
        let weapon = self.hold.map(|hold| {
            let weapon = if third < 0.5 {
                held_from(camera, look, HELD_AT_EYES)
            } else {
                held_from(head_place, (yaw, pitch), HELD_AT_SHOULDER)
            };
            self.reach_for(&weapon, hold, &transform);
            weapon
        });
        let skeleton = self.body.skeleton();
        let mut written = 0;
        for skin in 0..skeleton.skins().len() {
            written += skeleton.skinning(skin, &self.placed, &mut self.skinning[written..]);
        }
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
        self.drawn_at = transform;
        Drawn {
            transform,
            weapon,
            seen_joints: &self.headless,
            shadow_joints: &self.skinning,
            // In third person, hidden when the camera is pressed against
            // the head by a wall; in first person always drawn, headless.
            seen: third < 0.5 || near > TOO_CLOSE,
        }
    }
}

impl Character {
    /// Holds a weapon ready from the next frame drawn, or with `None`
    /// lets the arms hang as the clips have them.
    pub(crate) fn set_hold(&mut self, hold: Option<Hold>) {
        self.hold = hold;
    }

    /// Turns the arms so the hands hold `weapon`, which is in the world;
    /// `drawn` takes the model's space to the world (a turn about +Y and a
    /// move, nothing stretched).
    fn reach_for(&mut self, weapon: &Transform, hold: Hold, drawn: &Transform) {
        // Into the model's space, where the bones are.
        let undone = turn_undone(drawn);
        let back = lore_anim::math::apply(&undone, lore_anim::math::origin(drawn));
        let mut to_model = undone;
        to_model[3] = [-back[0], -back[1], -back[2], 1.0];
        let gun = lore_anim::math::combine(&to_model, weapon);
        let column = |i: usize| [gun[i][0], gun[i][1], gun[i][2]];
        let (right, up, behind) = (column(0), column(1), column(2));
        let down = [0.0, -1.0, 0.0];
        // Both hands on it: the chest turns, the left shoulder forward.
        if hold.support.is_some() {
            self.stand_side_on(behind);
        }
        // The right hand on the grip; its elbow down and out to the right.
        let grip = lore_anim::math::combine(&gun, &turn_undone(&WEAPON_IN_HAND));
        let elbow_out = |side: f32| {
            let o = lore_anim::math::origin(&gun);
            std::array::from_fn(|i| o[i] + down[i] * 0.6 + right[i] * 0.5 * side + behind[i] * 0.3)
        };
        let mut reaches = [None, None];
        reaches[0] = self.hand_reach(0, &grip, elbow_out(1.0));
        if let Some(support) = hold.support {
            let mut at = lore_anim::math::apply(&gun, support);
            let ahead = behind.map(|v| -v);
            // Out of the arm's reach (in first person the body stands
            // behind the eyes), the hand slides back along the weapon
            // towards the grip until it can hold it.
            if let Some((root, long)) = self.arm_reach(1) {
                let far =
                    |p: [f32; 3]| (0..3).map(|i| (p[i] - root[i]).powi(2)).sum::<f32>().sqrt();
                let mut step = 0;
                while far(at) > long && step < 40 {
                    at = std::array::from_fn(|i| at[i] + behind[i] * 0.01);
                    step += 1;
                }
            }
            // On a fore-end the barrel runs through the fist, thumb towards
            // the muzzle, knuckles down and to the left; cupping the right
            // hand, the thumb is up and the knuckles forward and left.
            let thing = if hold.cupped {
                frame_of(std::array::from_fn(|i| ahead[i] - right[i]), up, at)
            } else {
                frame_of(std::array::from_fn(|i| -right[i] - up[i]), ahead, at)
            };
            reaches[1] = self.hand_reach(1, &thing, elbow_out(-1.0));
        }
        let skeleton = self.body.skeleton();
        for (limb, reach) in reaches.into_iter().flatten() {
            lore_anim::ik::reach(skeleton, &mut self.blended, &mut self.placed, limb, &reach);
        }
    }

    /// Turns the chest (what both arms hang from) so the left shoulder comes
    /// forward, towards the way a weapon points (`behind` is its back).
    fn stand_side_on(&mut self, behind: [f32; 3]) {
        let (Some(left), Some(right)) = (self.arms[1], self.arms[0]) else {
            return;
        };
        let skeleton = self.body.skeleton();
        // The chest is the first bone above both arms.
        let above = |bone: usize| {
            let mut chain = [None; 8];
            let mut at = Some(bone);
            for slot in &mut chain {
                *slot = at;
                at = at.and_then(|b| skeleton.parent(b));
            }
            chain
        };
        let (lefts, rights) = (above(left.root), above(right.root));
        let Some(chest) = lefts
            .iter()
            .flatten()
            .find(|b| rights.contains(&Some(**b)))
            .copied()
        else {
            return;
        };
        let at = |bone: usize| self.placed.get(bone).map(lore_anim::math::origin);
        let (Some(middle), Some(shoulder)) = (at(chest), at(left.root)) else {
            return;
        };
        // Which way about the up axis brings the left shoulder ahead.
        let out = [shoulder[0] - middle[0], 0.0, shoulder[2] - middle[2]];
        let ahead = [-behind[0], 0.0, -behind[2]];
        let sign = (out[2] * ahead[0] - out[0] * ahead[2]).signum();
        let half = STANCE_TURN * 0.5 * sign;
        let turn = [0.0, libm::sinf(half), 0.0, libm::cosf(half)];
        lore_anim::ik::turn(skeleton, &mut self.blended, &mut self.placed, chest, turn);
    }

    /// Where the shoulder of arm `side` is in the model's space and how far
    /// its hand can reach from there, a little short of straight.
    fn arm_reach(&self, side: usize) -> Option<([f32; 3], f32)> {
        let limb = self.arms.get(side).copied().flatten()?;
        let at = |bone: usize| self.placed.get(bone).map(lore_anim::math::origin);
        let (root, middle, end) = (at(limb.root)?, at(limb.middle)?, at(limb.end)?);
        let far =
            |a: [f32; 3], b: [f32; 3]| (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f32>().sqrt();
        Some((root, (far(root, middle) + far(middle, end)) * 0.97))
    }

    /// What the arm of hand `side` (0 right, 1 left) must reach for that
    /// hand to hold a thing placed at `thing` (in the model's space, made
    /// with its length along +Y leaving the thumb's side): where its wrist
    /// goes and how the hand faces, with the elbow towards `pole`.
    fn hand_reach(
        &self,
        side: usize,
        thing: &Transform,
        pole: [f32; 3],
    ) -> Option<(lore_anim::ik::Limb, lore_anim::ik::Reach)> {
        let limb = self.arms.get(side).copied().flatten()?;
        let hand = self.hands.get(side).copied().flatten()?;
        let placed = self.placed.get(hand)?;
        // The hand bone as it must be: the grip undone from the thing.
        let bone = lore_anim::math::combine(thing, &turn_undone(&self.grips[side]));
        let scale: [f32; 3] = std::array::from_fn(|i| {
            (placed[i][0] * placed[i][0]
                + placed[i][1] * placed[i][1]
                + placed[i][2] * placed[i][2])
                .sqrt()
        });
        let palm = self.palms[side];
        let offset: [f32; 3] = std::array::from_fn(|i| palm[i] * scale[i]);
        let mut turned = bone;
        turned[3] = [0.0, 0.0, 0.0, 1.0];
        let reach = lore_anim::math::apply(&turned, offset);
        let at = lore_anim::math::origin(&bone);
        Some((
            limb,
            lore_anim::ik::Reach {
                target: std::array::from_fn(|i| at[i] - reach[i]),
                pole,
                facing: Some(lore_anim::ik::turn_of(&bone)),
            },
        ))
    }

    /// Where a thing held in the right hand (or, with `left`, the left)
    /// is, as of the body last drawn with [`figures`](Self::figures): what
    /// takes a model made with its grip at the origin and its length along
    /// +Y into the world. `None` for a body with no such hand.
    pub(crate) fn held_place(&self, left: bool) -> Option<Transform> {
        let hand = self.hands[usize::from(left)]?;
        let placed = self.placed.get(hand)?;
        // The bone's own size is left out: a thing is as big as it is made.
        let mut bone = *placed;
        for axis in bone.iter_mut().take(3) {
            let long = (axis[0] * axis[0] + axis[1] * axis[1] + axis[2] * axis[2]).sqrt();
            if long > 1e-6 {
                for part in axis.iter_mut().take(3) {
                    *part /= long;
                }
            }
        }
        // In the palm, not at the wrist. The offset is the bone's own, so
        // it is scaled as the bone is.
        let palm = lore_anim::math::apply(placed, self.palms[usize::from(left)]);
        bone[3] = [palm[0], palm[1], palm[2], 1.0];
        let grip = &self.grips[usize::from(left)];
        Some(lore_anim::math::combine(
            &self.drawn_at,
            &lore_anim::math::combine(&bone, grip),
        ))
    }
}

/// How far from the wrist to the knuckle of the middle finger a hand closes.
const PALM: f32 = 0.7;

/// How a thing sits in a hand whose knuckle of the middle finger is at
/// `middle` from the wrist, with the thumb's first bone at `thumb` and the
/// little finger's at `little`: what takes the thing's space (grip at the
/// origin, length along +Y, a blade's edge along +X) to the hand bone's.
///
/// The thing leaves the fist on the thumb's side, square to the fingers,
/// with its edge the way the knuckles face: how a sword, a torch or a
/// hammer is held. Worked out from the rig's own bones, so it is right for
/// either hand and for a model whose bones are turned another way.
fn grip(middle: [f32; 3], thumb: [f32; 3], little: [f32; 3]) -> Option<Transform> {
    let unit = |v: [f32; 3]| {
        let long = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        (long > 1e-6).then(|| v.map(|part| part / long))
    };
    let along = unit(middle)?;
    let across: [f32; 3] = std::array::from_fn(|i| thumb[i] - little[i]);
    let lean: f32 = (0..3).map(|i| across[i] * along[i]).sum();
    let up = unit(std::array::from_fn(|i| across[i] - along[i] * lean))?;
    let side = [
        along[1] * up[2] - along[2] * up[1],
        along[2] * up[0] - along[0] * up[2],
        along[0] * up[1] - along[1] * up[0],
    ];
    Some([
        [along[0], along[1], along[2], 0.0],
        [up[0], up[1], up[2], 0.0],
        [side[0], side[1], side[2], 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ])
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

    /// The inverse of a transform that turns and moves, nothing stretched.
    fn inverse(t: &Transform) -> Transform {
        let mut out = turn_undone(t);
        let back = lore_anim::math::apply(&out, lore_anim::math::origin(t));
        out[3] = [-back[0], -back[1], -back[2], 1.0];
        out
    }

    #[test]
    fn the_hands_hold_a_weapon_by_its_grip_and_fore_end_in_either_view() {
        let level = Level::from_triangles(&[]);
        let player = standing();
        let support = [0.0, -0.035, -0.25];
        for view in [View::First, View::Third] {
            let mut character = Character::mannequin().unwrap();
            character.view = view;
            for _ in 0..90 {
                character.tick(&player, (0.0, -0.1), &level, 1.0 / 60.0, |_, _| {});
            }
            character.set_hold(Some(Hold {
                support: Some(support),
                cupped: false,
            }));
            let eye = player.eye_position();
            let camera = character.camera(1.0, eye, (0.0, -0.1), &level);
            let drawn = character.figures(1.0, player.position, camera, (0.0, -0.1));
            let weapon = drawn.weapon.expect("a weapon is held");
            let far = |a: [f32; 3], b: [f32; 3]| {
                (0..3).map(|i| (a[i] - b[i]).powi(2)).sum::<f32>().sqrt()
            };
            let grip = lore_anim::math::origin(&weapon);
            let right = lore_anim::math::origin(&character.held_place(false).unwrap());
            let left = lore_anim::math::origin(&character.held_place(true).unwrap());
            assert!(
                far(right, grip) < 0.03,
                "{view:?}: the right hand is {} m off",
                far(right, grip)
            );
            // The left hand is under the barrel, between the grip and the
            // fore-end: on it in third person, nearer the grip in first,
            // where the body stands behind the eyes.
            let local = lore_anim::math::apply(&inverse(&weapon), left);
            let off_line = (local[0].powi(2) + (local[1] - support[1]).powi(2)).sqrt();
            assert!(
                off_line < 0.06,
                "{view:?}: the left hand is {off_line} m off the fore-end"
            );
            assert!(
                (support[2] - 0.02..-0.05).contains(&local[2]),
                "{view:?}: {local:?}"
            );
            if view == View::Third {
                assert!((local[2] - support[2]).abs() < 0.06, "{local:?}");
            }
            // In front of the body, below the eyes.
            assert!(grip[1] < eye[1] && grip[2] < eye[2], "{view:?}: {grip:?}");
        }
        // Held in one hand, the left hangs as the clips have it.
        let mut character = Character::mannequin().unwrap();
        character.set_hold(Some(Hold {
            support: None,
            cupped: false,
        }));
        let drawn = character.figures(1.0, player.position, player.eye_position(), (0.0, 0.0));
        assert!(drawn.weapon.is_some());
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

    /// A flat sheet with corners `a`, `b`, `c` and `d` in order, as two
    /// triangles.
    fn sheet(a: [f32; 3], b: [f32; 3], c: [f32; 3], d: [f32; 3]) -> [[[f32; 3]; 3]; 2] {
        [[a, b, c], [a, c, d]]
    }

    /// A room round the origin: walls at x = ±`half_width` and z =
    /// ±`half_length`, a floor at 0 and a ceiling at `height`.
    fn room(half_width: f32, half_length: f32, height: f32) -> Level {
        let (w, l, h) = (half_width, half_length, height);
        let sheets = [
            sheet([-w, 0.0, -l], [w, 0.0, -l], [w, 0.0, l], [-w, 0.0, l]),
            sheet([-w, h, -l], [w, h, -l], [w, h, l], [-w, h, l]),
            sheet([-w, 0.0, -l], [-w, 0.0, l], [-w, h, l], [-w, h, -l]),
            sheet([w, 0.0, -l], [w, 0.0, l], [w, h, l], [w, h, -l]),
            sheet([-w, 0.0, -l], [w, 0.0, -l], [w, h, -l], [-w, h, -l]),
            sheet([-w, 0.0, l], [w, 0.0, l], [w, h, l], [-w, h, l]),
        ];
        Level::from_triangles(&sheets.concat())
    }

    /// Looks every way round in `level` from a standing start, with the
    /// camera asked for both where a tick left the look and after a quick
    /// turn of the mouse no tick has seen, and hands each camera to `check`.
    fn look_all_round(level: &Level, mut check: impl FnMut([f32; 3], (f32, f32))) {
        let mut character = Character::mannequin().unwrap();
        character.view = View::Third;
        let player = standing();
        let eye = player.eye_position();
        for turn in 0..24u8 {
            let yaw = f32::from(turn) * std::f32::consts::TAU / 24.0;
            for tip in -4..=4i8 {
                let pitch = f32::from(tip) * 0.3;
                for _ in 0..6 {
                    character.tick(&player, (yaw, pitch), level, 1.0 / 60.0, |_, _| {});
                }
                for alpha in [0.0, 0.5, 1.0] {
                    check(
                        character.camera(alpha, eye, (yaw, pitch), level),
                        (yaw, pitch),
                    );
                }
                // A flick between two ticks: the boom is checked again.
                let flicked = (yaw + 1.3, (pitch - 0.5).clamp(-1.5, 1.5));
                check(character.camera(0.5, eye, flicked, level), flicked);
            }
        }
    }

    /// How far inside a wall the camera may be said to be: the sums of a
    /// sweep end a hair short of exact.
    const SLACK: f32 = 0.02;

    #[test]
    fn in_a_corridor_the_camera_stays_between_the_walls_however_the_player_looks() {
        // 1.8 m wide and 2.4 m high: narrower than the boom is long.
        let corridor = room(0.9, 8.0, 2.4);
        let free = 0.9 - CAMERA_RADIUS + SLACK;
        look_all_round(&corridor, |camera, look| {
            assert!(
                camera[0].abs() <= free,
                "through a side wall at {look:?}: {camera:?}"
            );
            assert!(
                camera[1] >= CAMERA_RADIUS - SLACK && camera[1] <= 2.4 - CAMERA_RADIUS + SLACK,
                "through the floor or ceiling at {look:?}: {camera:?}"
            );
        });
    }

    #[test]
    fn against_a_wall_the_camera_never_shows_its_inside() {
        // The player's back is half a metre from the end wall of a wide room.
        let wide = room(6.0, 0.9, 4.0);
        look_all_round(&wide, |camera, look| {
            assert!(
                camera[2].abs() <= 0.9 - CAMERA_RADIUS + SLACK,
                "through an end wall at {look:?}: {camera:?}"
            );
        });
    }

    #[test]
    fn under_a_low_ceiling_the_camera_stays_under_it() {
        // 30 cm over the eyes: the boom would hang higher than that.
        let low = room(8.0, 8.0, 1.9);
        look_all_round(&low, |camera, look| {
            assert!(
                camera[1] <= 1.9 - CAMERA_RADIUS + SLACK,
                "through the ceiling at {look:?}: {camera:?}"
            );
            assert!(
                camera[1] >= CAMERA_RADIUS - SLACK,
                "through the floor at {look:?}: {camera:?}"
            );
        });
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
        let drawn = character.figures(1.0, player.position, eye, (0.0, 0.0));
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
            let transform = character
                .figures(1.0, player.position, eye, (0.0, 0.0))
                .transform;
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
        let first = character.figures(1.0, player.position, eye, (0.0, 0.0));
        assert_ne!(first.seen_joints, first.shadow_joints, "first person");
        // Halfway through a switch the camera is well clear: the head is drawn.
        character.view = View::Third;
        for _ in 0..10 {
            character.tick(&player, (0.0, 0.0), &level, 1.0 / 60.0, |_, _| {});
        }
        let camera = character.camera(1.0, eye, (0.0, 0.0), &level);
        assert!(character.third > 0.0 && character.third < 1.0);
        let midway = character.figures(1.0, player.position, camera, (0.0, 0.0));
        assert_eq!(midway.seen_joints, midway.shadow_joints, "midway");
        // The same frame with the camera at the eyes leaves it out again.
        let close = character.figures(1.0, player.position, eye, (0.0, 0.0));
        assert_ne!(close.seen_joints, close.shadow_joints, "camera at the eyes");
    }

    #[test]
    fn looking_straight_down_in_a_sprint_keeps_the_collar_behind_the_camera() {
        let mut character = Character::mannequin().unwrap();
        let mut player = standing();
        player.grounded = true;
        player.velocity = [0.0, 0.0, -7.5];
        let level = Level::from_triangles(&[]);
        let neck = character.neck.unwrap();
        let down = -std::f32::consts::FRAC_PI_2;
        for tick in 0..120 {
            character.tick(&player, (0.0, down), &level, 1.0 / 60.0, |_, _| {});
            if tick < 30 {
                continue;
            }
            let eye = player.eye_position();
            let transform = character
                .figures(1.0, player.position, eye, (0.0, down))
                .transform;
            let [mx, _, mz, at] = transform;
            let mut placed = character.placed.clone();
            character
                .body
                .skeleton()
                .place(&character.pose, &mut placed);
            let [nx, _, nz, _] = placed[neck][3];
            // Behind is +Z: the camera looks along -Z.
            let behind = at[2] + mx[2] * nx + mz[2] * nz - eye[2];
            assert!(
                behind > 0.3,
                "tick {tick}: the neck is only {behind} m behind the camera"
            );
        }
    }

    #[test]
    fn standing_the_body_lets_the_look_turn_a_little_before_stepping_round() {
        let mut character = Character::mannequin().unwrap();
        let mut player = standing();
        player.grounded = true;
        let level = Level::from_triangles(&[]);
        let step = 1.0 / 60.0;
        // A small look to the side leaves the feet where they are.
        for _ in 0..60 {
            character.tick(&player, (0.6, 0.0), &level, step, |_, _| {});
        }
        assert!(character.yaw().abs() < 1e-4, "{}", character.yaw());
        assert!(character.turn_play.is_none());
        // A big one: the body is dragged to 90° behind at once, then steps
        // round with its turning clip until it faces the look.
        character.tick(&player, (2.2, 0.0), &level, step, |_, _| {});
        let gap = angle_between(character.yaw(), 2.2);
        assert!(gap <= TURN_MOST + 1e-3 && gap > 0.0, "{gap}");
        let play = character.turn_play.expect("the feet step");
        assert!(character.animator.is_playing(play));
        assert_eq!(character.playing_names().count(), 0, "scripts never see it");
        for _ in 0..60 {
            character.tick(&player, (2.2, 0.0), &level, step, |_, _| {});
        }
        assert!(angle_between(character.yaw(), 2.2).abs() < 0.02);
        assert!(!character.turning);
        // Walking off stops the stepping.
        player.velocity = [0.0, 0.0, -1.4];
        character.tick(&player, (2.2, 0.0), &level, step, |_, _| {});
        assert!(character.turn_play.is_none());
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
