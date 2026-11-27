//! The NPCs as they are drawn: the engine's mannequin, breathing where it
//! stands, its head turning to the player who comes close.
//!
//! None of this is the game's rules. The world says where each NPC stands
//! and which way it faces; this only makes a picture of that, so nothing
//! here is ever read by the simulation, and a world with no window (a test,
//! `lore check`) has none of it.
//!
//! A figure is kept for the NPCs nearest the player only, at most
//! [`MOST_DRAWN`]: one is light (its own clips playing and a pose; the
//! model and the clips themselves are shared by all), and those further
//! off are not drawn until they come nearer. A figure that goes is kept to
//! be used again, so walking about a town sets no memory aside.

use std::sync::Arc;

use lore_anim::{Animator, Body, Footing, Moving};
use lore_assets::{Pose, Scene, Transform};
use lore_npc::{Gaze, HUMAN, SHARES};
use lore_script::CharacterId;

use crate::character::{Character, Drawn};
use crate::game::Frame;
use crate::world::World;

/// The most NPCs drawn at once, the nearest first. Each takes two of the
/// renderer's figures (its picture and its shadow), and the player's body,
/// what they hold and what is in flight need some too.
pub(crate) const MOST_DRAWN: usize = (lore_render::MOST_FIGURES - 16) / 2;

/// How long a body takes to go from standing to falling, in seconds.
const FALL_FADE: f32 = 0.15;

/// What every figure shares: the model, its clips and its bones.
#[derive(Debug)]
struct Kind {
    model: Arc<Scene>,
    body: Arc<Body>,
    /// The chest, the neck and the head, which turn to look.
    looking: [Option<usize>; 3],
    /// Played when an NPC is hurt, and when it dies.
    hit: Option<u32>,
    death: Option<u32>,
}

/// One NPC as it is drawn.
#[derive(Debug, Clone)]
struct Figure {
    who: CharacterId,
    animator: Animator,
    /// The pose of the newest tick and the one before, and a frame's.
    pose: Vec<Pose>,
    pose_before: Vec<Pose>,
    blended: Vec<Pose>,
    placed: Vec<Transform>,
    skinning: Vec<Transform>,
    /// Where its feet are and which way it faces, now and a tick before.
    feet: [f32; 3],
    feet_before: [f32; 3],
    yaw: f32,
    yaw_before: f32,
    /// Where its head is turned, now and a tick before.
    gaze: Gaze,
    gaze_before: Gaze,
    /// How long ago it fell, while it lies where it fell.
    fallen: Option<f32>,
    /// Its health on the last tick: less now is a hurt to flinch at.
    health: f32,
    /// Whether it stood on the ground on the last tick, and how fast it
    /// was falling: a landing.
    grounded: bool,
    falling: f32,
}

impl Figure {
    fn new(kind: &Kind) -> Self {
        let rest = kind.body.skeleton().rest().to_vec();
        let bones = rest.len();
        let joints: usize = (kind.body.skeleton().skins().iter())
            .map(|skin| skin.joints.len())
            .sum();
        Self {
            who: CharacterId::PLAYER,
            animator: Animator::new(&kind.body),
            pose_before: rest.clone(),
            blended: rest.clone(),
            pose: rest,
            placed: vec![lore_anim::math::IDENTITY; bones],
            skinning: vec![lore_anim::math::IDENTITY; joints],
            feet: [0.0; 3],
            feet_before: [0.0; 3],
            yaw: 0.0,
            yaw_before: 0.0,
            gaze: Gaze::AHEAD,
            gaze_before: Gaze::AHEAD,
            fallen: None,
            health: 0.0,
            grounded: true,
            falling: 0.0,
        }
    }

    /// Makes this figure the NPC `who`, standing at `feet` facing `yaw`,
    /// as if it had always been there.
    fn become_npc(&mut self, kind: &Kind, who: CharacterId, feet: [f32; 3], yaw: f32, down: bool) {
        self.who = who;
        self.animator = Animator::new(&kind.body);
        (self.feet, self.feet_before) = (feet, feet);
        (self.yaw, self.yaw_before) = (yaw, yaw);
        (self.gaze, self.gaze_before) = (Gaze::AHEAD, Gaze::AHEAD);
        // One already lying down when it comes into view lies there.
        self.fallen = down.then_some(FALL_FADE * 10.0);
        // Not a number until its first tick: no hurt to flinch at.
        self.health = f32::NAN;
        self.grounded = true;
        self.falling = 0.0;
    }
}

/// The NPCs as they are drawn.
#[derive(Debug, Default)]
pub(crate) struct NpcFigures {
    kind: Option<Kind>,
    shown: Vec<Figure>,
    spare: Vec<Figure>,
    /// The NPCs by how far off they are, kept between ticks.
    near: Vec<(f32, CharacterId)>,
}

/// The first of `names` the body has.
fn bone(body: &Body, names: &[&str]) -> Option<usize> {
    names.iter().find_map(|name| body.skeleton().find(name))
}

/// The turn that is `gaze`, in the space of a model that faces +Z
/// (`facing` 1) or -Z (-1): about its up for the yaw, then about its side
/// for the pitch.
fn turn_of(gaze: Gaze, facing: f32) -> [f32; 4] {
    let (ys, yc) = (libm::sinf(gaze.yaw * 0.5), libm::cosf(gaze.yaw * 0.5));
    let (ps, pc) = (libm::sinf(gaze.pitch * 0.5), libm::cosf(gaze.pitch * 0.5));
    let side = if facing > 0.0 { -1.0 } else { 1.0 };
    lore_anim::math::quat_mul([0.0, ys, 0.0, yc], [side * ps, 0.0, 0.0, pc])
}

/// The yaw `alpha` of the way from `from` to `to`, the short way round.
fn yaw_between(from: f32, to: f32, alpha: f32) -> f32 {
    let tau = std::f32::consts::TAU;
    let gap = (to - from + std::f32::consts::PI).rem_euclid(tau) - std::f32::consts::PI;
    from + gap * alpha
}

impl NpcFigures {
    /// Figures of the engine's mannequin. With no mannequin (it could not
    /// be read, which the log says), NPCs are not drawn.
    pub(crate) fn mannequin() -> Self {
        let kind = Character::shared_mannequin().map(|character| {
            let body = Arc::clone(character.body());
            Kind {
                model: Arc::clone(character.model()),
                looking: [
                    bone(&body, &["spine_03", "Spine2", "mixamorig:Spine2"]),
                    bone(&body, &["neck_01", "Neck", "neck", "mixamorig:Neck"]),
                    bone(&body, &["Head", "head", "mixamorig:Head"]),
                ],
                hit: body.clip("hit"),
                death: body.clip("death"),
                body,
            }
        });
        Self {
            kind,
            ..Self::default()
        }
    }

    /// Moves every figure on by a tick of `dt` seconds, after the world's:
    /// the nearest NPCs to `eye` (the player's eyes) get one, and each
    /// breathes, falls, flinches and turns its head to `eye` as the world
    /// and the game's script say.
    pub(crate) fn tick(&mut self, world: &World, eye: [f32; 3], dt: f32) {
        let Some(kind) = &self.kind else { return };
        let npcs = world.npcs();
        // The nearest, as the crow flies; the same distance goes by id.
        self.near.clear();
        for npc in npcs {
            let p = npc.body.position;
            let far = (0..3).map(|i| (p[i] - eye[i]).powi(2)).sum::<f32>();
            self.near.push((far, npc.who));
        }
        self.near
            .sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        self.near.truncate(MOST_DRAWN);
        // Those no longer near are put by, to be used again.
        let mut index = 0;
        while index < self.shown.len() {
            let who = self.shown[index].who;
            if self.near.iter().any(|(_, near)| *near == who) {
                index += 1;
            } else {
                self.spare.push(self.shown.swap_remove(index));
            }
        }
        for &(_, who) in &self.near {
            if self.shown.iter().any(|figure| figure.who == who) {
                continue;
            }
            let Some(npc) = npcs.iter().find(|npc| npc.who == who) else {
                continue;
            };
            let mut figure = self.spare.pop().unwrap_or_else(|| Figure::new(kind));
            figure.become_npc(kind, who, npc.body.position, npc.facing(), npc.down);
            self.shown.push(figure);
        }
        let host = world.game().host();
        for figure in &mut self.shown {
            let Some(npc) = npcs.iter().find(|npc| npc.who == figure.who) else {
                continue;
            };
            figure.feet_before = figure.feet;
            figure.yaw_before = figure.yaw;
            figure.gaze_before = figure.gaze;
            std::mem::swap(&mut figure.pose_before, &mut figure.pose);
            figure.feet = npc.body.position;
            figure.yaw = npc.facing();
            let health = host.health(npc.who).map_or(0.0, |(now, _)| now);
            let hurt = health < figure.health && !npc.down;
            figure.health = health;
            if npc.down {
                let fallen = figure.fallen.map_or(0.0, |t| t + dt);
                figure.fallen = Some(fallen);
                // What it fell from, eased into the fall.
                figure.blended.copy_from_slice(&figure.pose_before);
                figure.pose.copy_from_slice(kind.body.skeleton().rest());
                if let Some(death) = kind.death {
                    let end = (kind.body.clips().get(death as usize)).map_or(0.0, |c| c.duration);
                    kind.body.sample(death, fallen.min(end), &mut figure.pose);
                }
                if fallen < FALL_FADE {
                    let share = fallen / FALL_FADE;
                    for (pose, was) in figure.pose.iter_mut().zip(&figure.blended) {
                        *pose = lore_anim::math::blend_pose(was, pose, share);
                    }
                }
                figure.gaze = figure.gaze.eased(Gaze::AHEAD, dt, &HUMAN);
                continue;
            }
            if figure.fallen.take().is_some() {
                // Back on its feet: the animator starts it standing.
                figure.animator = Animator::new(&kind.body);
            }
            if let (true, Some(hit)) = (hurt, kind.hit) {
                let how = lore_anim::Play {
                    layer: lore_anim::Layer::Upper,
                    fade: 0.05,
                    speed: 1.0,
                    looping: Some(false),
                };
                figure.animator.play(&kind.body, hit, how);
            }
            let body = &npc.body;
            let [vx, vy, vz] = body.velocity;
            let speed = libm::hypotf(vx, vz);
            let (sin, cos) = (libm::sinf(figure.yaw), libm::cosf(figure.yaw));
            let along = -vx * sin - vz * cos;
            let across = vx * cos - vz * sin;
            let moving = Moving {
                speed,
                direction: if speed > 1e-3 {
                    [across / speed, along / speed]
                } else {
                    [0.0, 1.0]
                },
                crouched: body.crouched,
                footing: if body.swimming {
                    Footing::Swimming
                } else if body.grounded {
                    Footing::Ground
                } else {
                    Footing::Air(vy)
                },
                landed: if body.grounded && !figure.grounded {
                    (-figure.falling).max(0.0)
                } else {
                    0.0
                },
                accel: [0.0; 2],
            };
            figure.grounded = body.grounded;
            figure.falling = vy;
            figure.animator.step(&kind.body, &moving, dt, |_, _| {});
            figure.animator.pose(&kind.body, &mut figure.pose);
            // The head turns to the player who comes close enough.
            let within = host.npc_look_distance(npc.who);
            let eyes = npc.eyes();
            let near = (0..3).map(|i| (eyes[i] - eye[i]).powi(2)).sum::<f32>() <= within * within;
            let goal = (within > 0.0 && near)
                .then(|| Gaze::toward(eyes, figure.yaw, eye, &HUMAN))
                .flatten()
                .unwrap_or(Gaze::AHEAD);
            figure.gaze = figure.gaze.eased(goal, dt, &HUMAN);
        }
    }

    /// Hands `frame` every figure, drawn `alpha` of the way from the tick
    /// before the newest to the newest.
    pub(crate) fn show(&mut self, frame: &mut Frame) {
        let Some(kind) = &self.kind else { return };
        let alpha = frame.alpha();
        let skeleton = kind.body.skeleton();
        let facing = kind.body.facing();
        for figure in &mut self.shown {
            for ((out, a), b) in (figure.blended.iter_mut())
                .zip(&figure.pose_before)
                .zip(&figure.pose)
            {
                *out = lore_anim::math::blend_pose(a, b, alpha);
            }
            skeleton.place(&figure.blended, &mut figure.placed);
            let gaze = figure.gaze_before.between(figure.gaze, alpha);
            if gaze != Gaze::AHEAD {
                for (bone, share) in kind.looking.iter().zip(SHARES) {
                    if let Some(bone) = *bone {
                        let turn = turn_of(gaze.part(share), facing);
                        lore_anim::ik::turn(
                            skeleton,
                            &mut figure.blended,
                            &mut figure.placed,
                            bone,
                            turn,
                        );
                    }
                }
            }
            let mut written = 0;
            for skin in 0..skeleton.skins().len() {
                written += skeleton.skinning(skin, &figure.placed, &mut figure.skinning[written..]);
            }
            let yaw = yaw_between(figure.yaw_before, figure.yaw, alpha);
            let model_yaw = if facing > 0.0 {
                yaw + std::f32::consts::PI
            } else {
                yaw
            };
            let (s, c) = (libm::sinf(model_yaw), libm::cosf(model_yaw));
            let feet = crate::blend::position(figure.feet_before, figure.feet, alpha);
            let drawn = Drawn {
                transform: [
                    [c, 0.0, -s, 0.0],
                    [0.0, 1.0, 0.0, 0.0],
                    [s, 0.0, c, 0.0],
                    [feet[0], feet[1], feet[2], 1.0],
                ],
                weapon: None,
                seen_joints: &figure.skinning,
                shadow_joints: &figure.skinning,
                seen: true,
            };
            frame.show_figure(&kind.model, &drawn);
        }
    }
}

#[cfg(test)]
mod tests {
    use lore_input::{ActionMap, Input};

    use super::*;
    use crate::game::Game;
    use crate::script_game::ScriptGame;
    use crate::world::Playing;

    const STEP: f32 = 1.0 / 60.0;

    /// A game of `script` on a flat floor, played for `ticks`.
    fn played(script: &str, ticks: u32) -> (Playing, Input) {
        let actions = ActionMap::standard();
        let game = ScriptGame::from_source(script, &actions);
        assert!(
            game.host().last_error().is_none(),
            "{:?}",
            game.host().last_error()
        );
        let mut playing = Playing::start(game, None, &actions, 60);
        let mut input = Input::new(actions);
        for _ in 0..ticks {
            playing.tick(input.tick(), STEP);
        }
        (playing, input)
    }

    #[test]
    fn an_npc_turns_its_head_to_the_player_who_comes_close_and_not_beyond() {
        // Ahead of the player and to the right, facing back toward them:
        // the player is 45 degrees to its right.
        let (playing, _) = played(
            r#"game.npc { name = "Guard", at = vector.create(2, 0, -2), facing = 180 }"#,
            90,
        );
        let figure = &playing.npc_figures.shown[0];
        assert!(
            (figure.gaze.yaw + std::f32::consts::FRAC_PI_4).abs() < 0.05,
            "{:?}",
            figure.gaze
        );
        assert!(figure.gaze.pitch.abs() < 0.1, "{:?}", figure.gaze);

        // Told never to look, it looks ahead.
        let (playing, _) = played(
            r#"local guard = game.npc { name = "Guard", at = vector.create(2, 0, -2), facing = 180 }
            guard.lookDistance = 0"#,
            90,
        );
        assert_eq!(playing.npc_figures.shown[0].gaze, Gaze::AHEAD);
    }

    #[test]
    fn only_the_nearest_npcs_are_drawn_and_each_as_a_picture_and_a_shadow() {
        let script = format!(
            "for i = 1, {} do game.npc {{ name = \"Villager \" .. i, at = vector.create(i * 2, 0, -3) }} end",
            MOST_DRAWN + 6
        );
        let (mut playing, input) = played(&script, 3);
        assert_eq!(playing.npc_figures.shown.len(), MOST_DRAWN);
        // The furthest are the ones left out.
        let furthest = playing.world().npcs().last().map(|npc| npc.who);
        assert!(
            playing
                .npc_figures
                .shown
                .iter()
                .all(|figure| Some(figure.who) != furthest)
        );
        let mut frame = Frame::between_ticks(&input, 0.5, STEP);
        playing.frame(&mut frame);
        let (figures, _) = frame.figures();
        let mannequins = figures
            .iter()
            .filter(|figure| Arc::ptr_eq(&figure.model, &figures[0].model))
            .count();
        // The player's body, and every NPC drawn: two each.
        assert_eq!(mannequins, 2 + 2 * MOST_DRAWN);
    }

    #[test]
    fn an_npc_whose_health_is_gone_lies_where_it_fell_and_stops_looking() {
        let (mut playing, mut input) = played(
            r#"local guard = game.npc { name = "Guard", at = vector.create(0, 0, -2), facing = 180 }
            game.onTick(function() if game.time() > 0.5 then guard:setHealth(0) end end)"#,
            20,
        );
        assert!(playing.npc_figures.shown[0].fallen.is_none());
        for _ in 0..60 {
            playing.tick(input.tick(), STEP);
        }
        let figure = &playing.npc_figures.shown[0];
        assert!(figure.fallen.is_some_and(|t| t > 0.1));
        assert!(figure.gaze.yaw.abs() < 0.05);
    }

    /// Where the bone `bone` of `figure` is in the world, as of its newest
    /// tick.
    fn bone_in_world(kind: &Kind, figure: &Figure, bone: usize) -> [f32; 3] {
        let mut placed = vec![lore_anim::math::IDENTITY; figure.pose.len()];
        kind.body.skeleton().place(&figure.pose, &mut placed);
        let [x, y, z, _] = placed[bone][3];
        let yaw = if kind.body.facing() > 0.0 {
            figure.yaw + std::f32::consts::PI
        } else {
            figure.yaw
        };
        let (s, c) = (yaw.sin(), yaw.cos());
        [
            figure.feet[0] + c * x + s * z,
            figure.feet[1] + y,
            figure.feet[2] - s * x + c * z,
        ]
    }

    #[test]
    fn a_walking_npc_plants_its_feet_turns_smoothly_and_still_looks_at_the_player() {
        // Behind the player and facing away: it turns right round to walk
        // past them, ahead (-Z).
        let (mut playing, mut input) = played(
            r#"local guard = game.npc { name = "Guard", at = vector.create(1, 0, 3), facing = 180 }
            guard:goTo(vector.create(1, 0, -20))"#,
            1,
        );
        let kind = playing.npc_figures.kind.as_ref().map(|kind| {
            let skeleton = kind.body.skeleton();
            let foot = |names: [&str; 2]| names.iter().find_map(|name| skeleton.find(name));
            (foot(["foot_l", "LeftFoot"]), foot(["foot_r", "RightFoot"]))
        });
        let Some((Some(left), Some(right))) = kind else {
            panic!("the mannequin has no feet");
        };
        let mut turned_most: f32 = 0.0;
        let (mut body_went, mut planted_went) = (0.0, 0.0);
        let mut was: Option<([f32; 3], [f32; 3], [f32; 3])> = None;
        let mut looked = false;
        for tick in 0..400 {
            playing.tick(input.tick(), STEP);
            let kind = playing.npc_figures.kind.as_ref().unwrap();
            let figure = &playing.npc_figures.shown[0];
            let gap = (figure.yaw - figure.yaw_before + std::f32::consts::PI)
                .rem_euclid(std::f32::consts::TAU)
                - std::f32::consts::PI;
            turned_most = turned_most.max(gap.abs());
            looked |= figure.gaze != Gaze::AHEAD
                && (figure.feet[0] - figure.feet_before[0]).abs()
                    + (figure.feet[2] - figure.feet_before[2]).abs()
                    > 0.01;
            let now = (
                figure.feet,
                bone_in_world(kind, figure, left),
                bone_in_world(kind, figure, right),
            );
            // Once it is under way, not while it starts.
            if let Some((feet, l, r)) = was
                && tick > 60
            {
                let flat = |a: [f32; 3], b: [f32; 3]| (a[0] - b[0]).hypot(a[2] - b[2]);
                // The foot on the ground: the lower, while it is down.
                let (foot, before) = if now.1[1] < now.2[1] {
                    (now.1, l)
                } else {
                    (now.2, r)
                };
                if foot[1] < 0.13 {
                    body_went += flat(feet, now.0);
                    planted_went += flat(before, foot);
                }
            }
            was = Some(now);
        }
        // No snap round to a corner: never more than a body turns in a tick.
        assert!(turned_most <= 5.0 * STEP + 1e-4, "{turned_most}");
        assert!(body_went > 0.5, "{body_went}");
        // The foot on the ground keeps to it while the body goes on.
        assert!(
            planted_went < body_went * 0.35,
            "{planted_went} against {body_went}"
        );
        // Passing close by the player, its head turned to them as it walked.
        assert!(looked);
    }
}
