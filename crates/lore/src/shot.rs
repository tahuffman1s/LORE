//! The view a script asked for with `game.camera`: where the camera is for
//! each frame, and the travel from one shot to the next.
//!
//! The player's own view is worked out first, as ever. A shot then takes
//! its place: a camera fixed somewhere, or one trailing a character. When
//! the script says a new shot (or gives the view back) with some seconds,
//! the view travels there from wherever it is on an ease-in, ease-out
//! curve, timed by the game's clock, so a paused game holds the camera
//! still and slow motion slows it.
//!
//! None of this is read by the simulation: it only says what is drawn.

use std::f32::consts::{PI, TAU};

use lore_script::{CameraSaid, CameraTarget, CharacterId, ShotSaid};

/// How far above a character's feet a shot looks: about the chest.
const CHEST: f32 = 1.2;

/// A camera: where it is, which way it faces and how wide it sees.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Pose {
    /// Where the camera is, in metres.
    pub(crate) position: [f32; 3],
    /// Which way it faces, as [`Frame::look_from`](crate::game::Frame::look_from) takes it.
    pub(crate) yaw: f32,
    /// How far it looks up, in radians.
    pub(crate) pitch: f32,
    /// A field of view in degrees, and how far from the player's own
    /// setting toward it the picture is, from 0 (the player's) to 1.
    pub(crate) lens: (f32, f32),
}

impl Pose {
    /// A camera at `position` that looks at `target` with the player's
    /// field of view. One on top of its target looks along `yaw`.
    fn looking(position: [f32; 3], target: [f32; 3], yaw: f32) -> Self {
        let to: [f32; 3] = std::array::from_fn(|axis| target[axis] - position[axis]);
        let flat = to[0].hypot(to[2]);
        let (yaw, pitch) = if flat > 1e-4 || to[1].abs() > 1e-4 {
            let yaw = if flat > 1e-4 {
                (-to[0]).atan2(-to[2]).rem_euclid(TAU)
            } else {
                yaw
            };
            (yaw, to[1].atan2(flat))
        } else {
            (yaw, 0.0)
        };
        Self {
            position,
            yaw,
            pitch,
            lens: (0.0, 0.0),
        }
    }

    /// This camera through a lens of `fov` degrees, if the shot has one.
    fn through(mut self, fov: Option<f32>) -> Self {
        if let Some(fov) = fov {
            self.lens = (fov, 1.0);
        }
        self
    }

    /// The camera `t` of the way from `self` to `to`, turning the short way.
    fn toward(self, to: Self, t: f32) -> Self {
        let turn = (to.yaw - self.yaw + PI).rem_euclid(TAU) - PI;
        // The two fields of view are mixed as the angles they would be, so
        // one that is the player's own (weight 0) needs no number.
        let (near, far) = (self.lens.1 * (1.0 - t), to.lens.1 * t);
        let weight = near + far;
        let degrees = if weight > 0.0 {
            (self.lens.0 * near + to.lens.0 * far) / weight
        } else {
            0.0
        };
        Self {
            position: crate::blend::position(self.position, to.position, t),
            yaw: (self.yaw + turn * t).rem_euclid(TAU),
            pitch: self.pitch + (to.pitch - self.pitch) * t,
            lens: (degrees, weight),
        }
    }
}

/// A camera `height` metres straight above `target`, looking down with
/// north (-Z) at the top of the picture.
fn top_down(target: [f32; 3], height: f32) -> Pose {
    Pose {
        position: [target[0], target[1] + height, target[2]],
        yaw: 0.0,
        // A hair short of straight down, where which way is up is not said.
        pitch: -(std::f32::consts::FRAC_PI_2 - 0.001),
        lens: (0.0, 0.0),
    }
}

/// A camera `distance` metres from `target` and `height` above it, looking
/// along `angle` (radians to the left of -Z) at it.
fn side_on(target: [f32; 3], distance: f32, height: f32, angle: f32) -> Pose {
    let (sin, cos) = angle.sin_cos();
    // Back from the target along the way the camera looks.
    let position = [
        target[0] + sin * distance,
        target[1] + height,
        target[2] + cos * distance,
    ];
    Pose::looking(position, target, angle)
}

/// The place `share` of the way (0 to 1) along a smooth curve through
/// `points`: a Catmull-Rom spline, which passes through every one.
fn along(points: &[[f32; 3]], share: f32) -> [f32; 3] {
    let spans = points.len().saturating_sub(1).max(1);
    // A handful of spans: far below where an f32 stops being exact.
    #[allow(
        clippy::cast_precision_loss,
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss
    )]
    let (span, t) = {
        let scaled = share.clamp(0.0, 1.0) * spans as f32;
        let span = (scaled.floor() as usize).min(spans - 1);
        (span, scaled - span as f32)
    };
    let at = |index: isize| {
        let last = points.len() as isize - 1;
        points[index.clamp(0, last.max(0)) as usize]
    };
    let span = span as isize;
    let (a, b, c, d) = (at(span - 1), at(span), at(span + 1), at(span + 2));
    std::array::from_fn(|i| {
        0.5 * ((2.0 * b[i])
            + (c[i] - a[i]) * t
            + (2.0 * a[i] - 5.0 * b[i] + 4.0 * c[i] - d[i]) * t * t
            + (3.0 * b[i] - a[i] - 3.0 * c[i] + d[i]) * t * t * t)
    })
}

/// Which way "forward" is for a player who walks by the shot (`steer`):
/// the way the camera looks, on the ground. Worked out from the game's own
/// state, never from a picture, so it is the same on every computer. `None`
/// for a shot that does not steer or whose heading is not the game's to
/// know (one that trails, circles or travels by the clock of the picture).
pub(crate) fn steer_yaw(
    said: &CameraSaid,
    feet_of: impl Fn(CharacterId) -> Option<[f32; 3]>,
) -> Option<f32> {
    if !said.steer {
        return None;
    }
    match said.shot {
        ShotSaid::TopDown { .. } => Some(0.0),
        ShotSaid::SideOn { angle, .. } => Some(angle.rem_euclid(TAU)),
        ShotSaid::Fixed { from, at } => {
            let target = match at {
                CameraTarget::Point(point) => point,
                CameraTarget::Character(who) => feet_of(who)?,
            };
            let (x, z) = (target[0] - from[0], target[2] - from[2]);
            (x.hypot(z) > 1e-3).then(|| (-x).atan2(-z).rem_euclid(TAU))
        }
        _ => None,
    }
}

/// The camera a script directs.
#[derive(Debug)]
pub(crate) struct ShotCamera {
    /// Which saying of a shot this is on ([`CameraSaid::said`]).
    said: u32,
    /// Where the view was when the shot was said, and when that was, in
    /// seconds of the game's clock.
    from: Option<Pose>,
    started: f64,
    /// What was drawn last frame.
    drawn: Option<Pose>,
    /// Which way a trailing camera is from who it follows, on the ground:
    /// a direction of length 1.
    trail: [f32; 2],
    /// Where the shot's target was last known to be.
    target: [f32; 3],
}

impl Default for ShotCamera {
    fn default() -> Self {
        Self {
            said: 0,
            from: None,
            started: 0.0,
            drawn: None,
            trail: [0.0, 1.0],
            target: [0.0; 3],
        }
    }
}

impl ShotCamera {
    /// The camera for this frame.
    ///
    /// `player` is the player's own view, `now` the game's clock in
    /// seconds, and `feet_of` says where a character stands, or `None` for
    /// one who has left the world (the shot then stays on where they were).
    pub(crate) fn pose(
        &mut self,
        said: &CameraSaid,
        player: Pose,
        now: f64,
        feet_of: impl Fn(CharacterId) -> Option<[f32; 3]>,
    ) -> Pose {
        let chest = |who: CharacterId| feet_of(who).map(|feet| [feet[0], feet[1] + CHEST, feet[2]]);
        let fresh = said.said != self.said;
        if fresh {
            self.said = said.said;
            let from = self.drawn.unwrap_or(player);
            self.from = (said.seconds > 0.0).then_some(from);
            self.started = now;
            let circled = match said.shot {
                ShotSaid::Follow { who, .. } => Some(chest(who).unwrap_or(self.target)),
                ShotSaid::Orbit { about, .. } => Some(match about {
                    CameraTarget::Point(point) => point,
                    CameraTarget::Character(who) => chest(who).unwrap_or(self.target),
                }),
                _ => None,
            };
            if let Some(about) = circled {
                // It trails, or starts its circle, from the side it is
                // already on.
                let away = [from.position[0] - about[0], from.position[2] - about[2]];
                let far = away[0].hypot(away[1]);
                self.trail = if far > 1e-3 {
                    [away[0] / far, away[1] / far]
                } else {
                    // Behind the way the view faces.
                    [from.yaw.sin(), from.yaw.cos()]
                };
            }
        }
        let wanted = match said.shot {
            ShotSaid::Player => player,
            ShotSaid::Fixed { from, at } => {
                self.target = match at {
                    CameraTarget::Point(point) => point,
                    CameraTarget::Character(who) => chest(who).unwrap_or(self.target),
                };
                Pose::looking(from, self.target, player.yaw).through(said.fov)
            }
            ShotSaid::Follow {
                who,
                distance,
                height,
            } => {
                self.target = chest(who).unwrap_or(self.target);
                let feet = self.target[1] - CHEST;
                if let (false, Some(before)) = (fresh, self.drawn) {
                    // Dragged along behind: it keeps the side it was on.
                    let away = [
                        before.position[0] - self.target[0],
                        before.position[2] - self.target[2],
                    ];
                    let far = away[0].hypot(away[1]);
                    if far > 1e-3 && self.from.is_none() {
                        self.trail = [away[0] / far, away[1] / far];
                    }
                }
                let position = [
                    self.target[0] + self.trail[0] * distance,
                    feet + height,
                    self.target[2] + self.trail[1] * distance,
                ];
                Pose::looking(position, self.target, player.yaw).through(said.fov)
            }
            ShotSaid::TopDown { who, height } => {
                self.target = chest(who).unwrap_or(self.target);
                top_down(self.target, height).through(said.fov)
            }
            ShotSaid::SideOn {
                who,
                distance,
                height,
                angle,
            } => {
                self.target = chest(who).unwrap_or(self.target);
                side_on(self.target, distance, height, angle).through(said.fov)
            }
            ShotSaid::Orbit {
                about,
                distance,
                height,
                speed,
            } => {
                self.target = match about {
                    CameraTarget::Point(point) => point,
                    CameraTarget::Character(who) => chest(who).unwrap_or(self.target),
                };
                // Round from where it was when the shot was said.
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "an angle, taken round the circle"
                )]
                let round =
                    ((now - self.started) * f64::from(speed)).rem_euclid(f64::from(TAU)) as f32;
                let (sin, cos) = round.sin_cos();
                let [x, z] = self.trail;
                // The start direction turned to the left by `round`.
                let away = [x * cos + z * sin, z * cos - x * sin];
                let position = [
                    self.target[0] + away[0] * distance,
                    self.target[1] + height,
                    self.target[2] + away[1] * distance,
                ];
                Pose::looking(position, self.target, player.yaw).through(said.fov)
            }
            ShotSaid::Path {
                points,
                count,
                at,
                over,
                looping,
            } => {
                let points = &points[..usize::from(count).clamp(2, points.len())];
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "a share of the path, from 0 to 1"
                )]
                let gone = ((now - self.started).max(0.0) / f64::from(over.max(0.01))) as f32;
                let share = if looping {
                    gone.rem_euclid(1.0)
                } else {
                    gone.clamp(0.0, 1.0)
                };
                let position = along(points, share);
                let toward = match at {
                    Some(CameraTarget::Point(point)) => point,
                    Some(CameraTarget::Character(who)) => {
                        self.target = chest(who).unwrap_or(self.target);
                        self.target
                    }
                    // The way it is going: a little further along the path.
                    None => {
                        let ahead = along(points, (share + 0.01).min(1.0));
                        let behind = along(points, (share - 0.01).max(0.0));
                        std::array::from_fn(|i| position[i] + ahead[i] - behind[i])
                    }
                };
                Pose::looking(position, toward, player.yaw).through(said.fov)
            }
        };
        let pose = match self.from {
            Some(from) => {
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "a part of a travel, from 0 to 1"
                )]
                let part = ((now - self.started) / f64::from(said.seconds)) as f32;
                // A clock that went back (a loaded game) ends the travel.
                if (0.0..1.0).contains(&part) {
                    from.toward(wanted, part * part * (3.0 - 2.0 * part))
                } else {
                    self.from = None;
                    wanted
                }
            }
            None => wanted,
        };
        self.drawn = Some(pose);
        pose
    }
}

#[cfg(test)]
mod tests {
    use std::f32::consts::FRAC_PI_2;

    use super::*;

    const PLAYER: Pose = Pose {
        position: [0.0, 1.7, 0.0],
        yaw: 0.0,
        pitch: 0.0,
        lens: (0.0, 0.0),
    };

    fn said(shot: ShotSaid, seconds: f32, said: u32) -> CameraSaid {
        CameraSaid {
            shot,
            seconds,
            fov: None,
            steer: false,
            said,
        }
    }

    fn nobody(_: CharacterId) -> Option<[f32; 3]> {
        None
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 1e-4
    }

    #[test]
    fn the_view_is_the_players_until_a_shot_is_said() {
        let mut camera = ShotCamera::default();
        let pose = camera.pose(&CameraSaid::default(), PLAYER, 0.0, nobody);
        assert_eq!(pose, PLAYER);
    }

    #[test]
    fn a_fixed_shot_looks_at_its_target() {
        let mut camera = ShotCamera::default();
        // From the east, level with the target: it looks west, along -X.
        let shot = ShotSaid::Fixed {
            from: [10.0, 1.0, 0.0],
            at: CameraTarget::Point([0.0, 1.0, 0.0]),
        };
        let pose = camera.pose(&said(shot, 0.0, 1), PLAYER, 0.0, nobody);
        assert_eq!(pose.position, [10.0, 1.0, 0.0]);
        assert!(close(pose.yaw, FRAC_PI_2), "{}", pose.yaw);
        assert!(close(pose.pitch, 0.0));

        // From above, it looks down.
        let shot = ShotSaid::Fixed {
            from: [0.0, 11.0, 10.0],
            at: CameraTarget::Point([0.0, 1.0, 0.0]),
        };
        let pose = camera.pose(&said(shot, 0.0, 2), PLAYER, 0.0, nobody);
        assert!(close(pose.yaw, 0.0), "{}", pose.yaw);
        assert!(close(pose.pitch, -std::f32::consts::FRAC_PI_4));
    }

    #[test]
    fn a_fixed_shot_turns_to_keep_a_character_in_view() {
        let mut camera = ShotCamera::default();
        let shot = ShotSaid::Fixed {
            from: [0.0, CHEST, 10.0],
            at: CameraTarget::Character(CharacterId::PLAYER),
        };
        let pose = camera.pose(&said(shot, 0.0, 1), PLAYER, 0.0, |_| Some([0.0; 3]));
        assert!(close(pose.yaw, 0.0));
        let pose = camera.pose(&said(shot, 0.0, 1), PLAYER, 0.1, |_| {
            Some([10.0, 0.0, 10.0])
        });
        // They walked east: the camera faces +X, a quarter turn to the right.
        assert!(close(pose.yaw, TAU - FRAC_PI_2), "{}", pose.yaw);
        // Gone from the world, the shot stays on where they were.
        let after = camera.pose(&said(shot, 0.0, 1), PLAYER, 0.2, nobody);
        assert_eq!(after, pose);
    }

    #[test]
    fn a_shot_given_seconds_travels_there_and_back() {
        let mut camera = ShotCamera::default();
        camera.pose(&CameraSaid::default(), PLAYER, 0.0, nobody);
        let shot = ShotSaid::Fixed {
            from: [0.0, 1.7, 10.0],
            at: CameraTarget::Point([0.0, 1.7, 0.0]),
        };
        let going = CameraSaid {
            fov: Some(40.0),
            ..said(shot, 2.0, 1)
        };
        let start = camera.pose(&going, PLAYER, 5.0, nobody);
        assert_eq!(start, PLAYER, "it starts from where the view was");
        let half = camera.pose(&going, PLAYER, 6.0, nobody);
        assert!(close(half.position[2], 5.0), "{:?}", half.position);
        // Half way to a 40 degree lens from the player's own.
        assert!(
            close(half.lens.0, 40.0) && close(half.lens.1, 0.5),
            "{:?}",
            half.lens
        );
        let there = camera.pose(&going, PLAYER, 7.0, nobody);
        assert_eq!(there.position, [0.0, 1.7, 10.0]);
        assert_eq!(there.lens, (40.0, 1.0));

        // And back to the player, who has moved meanwhile.
        let moved = Pose {
            position: [4.0, 1.7, 0.0],
            ..PLAYER
        };
        let back = said(ShotSaid::Player, 1.0, 2);
        camera.pose(&back, moved, 8.0, nobody);
        let half = camera.pose(&back, moved, 8.5, nobody);
        assert!(close(half.position[0], 2.0) && close(half.position[2], 5.0));
        assert!(close(half.lens.1, 0.5));
        assert_eq!(camera.pose(&back, moved, 9.0, nobody), moved);
    }

    #[test]
    fn a_top_down_shot_is_overhead_and_a_side_on_shot_looks_one_way() {
        let mut camera = ShotCamera::default();
        let who = CharacterId::PLAYER;
        let feet = |_| Some([5.0, 0.0, -3.0]);
        let shot = ShotSaid::TopDown { who, height: 14.0 };
        let pose = camera.pose(&said(shot, 0.0, 1), PLAYER, 0.0, feet);
        assert!(close(pose.position[0], 5.0) && close(pose.position[2], -3.0));
        assert!(close(pose.position[1], CHEST + 14.0) && pose.pitch < -1.56);
        assert!(close(pose.yaw, 0.0));

        // Looking west (a quarter left of north): the camera is to the
        // east of them, level, and they are in the middle of the picture.
        let shot = ShotSaid::SideOn {
            who,
            distance: 10.0,
            height: 0.0,
            angle: FRAC_PI_2,
        };
        let pose = camera.pose(&said(shot, 0.0, 2), PLAYER, 0.0, feet);
        assert!(close(pose.position[0], 15.0) && close(pose.position[2], -3.0));
        assert!(close(pose.yaw, FRAC_PI_2) && close(pose.pitch, 0.0));
        // They walk north: it slides with them and does not turn.
        let pose = camera.pose(&said(shot, 0.0, 2), PLAYER, 1.0, |_| Some([5.0, 0.0, -9.0]));
        assert!(close(pose.position[2], -9.0) && close(pose.yaw, FRAC_PI_2));
    }

    #[test]
    fn an_orbit_goes_round_at_its_speed_and_a_path_passes_through_its_places() {
        let mut camera = ShotCamera::default();
        // The view was south of the middle, so the circle starts there.
        let from = Pose {
            position: [0.0, 2.0, 5.0],
            ..PLAYER
        };
        camera.pose(&CameraSaid::default(), from, 0.0, nobody);
        let shot = ShotSaid::Orbit {
            about: CameraTarget::Point([0.0; 3]),
            distance: 6.0,
            height: 2.0,
            // A quarter turn a second.
            speed: FRAC_PI_2,
        };
        let asked = said(shot, 0.0, 1);
        let start = camera.pose(&asked, from, 10.0, nobody);
        assert!(close(start.position[0], 0.0) && close(start.position[2], 6.0));
        assert!(close(start.position[1], 2.0) && close(start.yaw, 0.0));
        let later = camera.pose(&asked, from, 11.0, nobody);
        // A quarter round to the left of south is east.
        assert!(close(later.position[0], 6.0) && later.position[2].abs() < 1e-3);
        // Still looking at the middle: west.
        assert!(close(later.yaw, FRAC_PI_2), "{}", later.yaw);

        let mut points = [[0.0; 3]; lore_script::MOST_PATH_POINTS];
        points[..3].copy_from_slice(&[[0.0, 3.0, 10.0], [8.0, 3.0, 0.0], [0.0, 3.0, -10.0]]);
        let shot = ShotSaid::Path {
            points,
            count: 3,
            at: None,
            over: 4.0,
            looping: false,
        };
        let asked = said(shot, 0.0, 2);
        let at = |camera: &mut ShotCamera, now: f64| camera.pose(&asked, PLAYER, now, nobody);
        assert_eq!(at(&mut camera, 20.0).position, [0.0, 3.0, 10.0]);
        let middle = at(&mut camera, 22.0);
        assert!(close(middle.position[0], 8.0) && close(middle.position[2], 0.0));
        // Half way it is heading north, along the path.
        let north = (middle.yaw + PI).rem_euclid(TAU) - PI;
        assert!(north.abs() < 1e-3, "{}", middle.yaw);
        // It stops at the end unless it loops.
        assert_eq!(at(&mut camera, 24.0).position, [0.0, 3.0, -10.0]);
        assert_eq!(at(&mut camera, 99.0).position, [0.0, 3.0, -10.0]);
    }

    #[test]
    fn forward_is_away_from_a_shot_that_steers() {
        let steers = |shot: ShotSaid| CameraSaid {
            steer: true,
            ..said(shot, 0.0, 1)
        };
        let who = CharacterId::PLAYER;
        let feet = |_| Some([0.0; 3]);
        // Not unless it was asked for.
        let top = ShotSaid::TopDown { who, height: 9.0 };
        assert_eq!(steer_yaw(&said(top, 0.0, 1), feet), None);
        // Overhead, forward is north; side-on, the way the camera looks.
        assert_eq!(steer_yaw(&steers(top), feet), Some(0.0));
        let side = ShotSaid::SideOn {
            who,
            distance: 10.0,
            height: 0.0,
            angle: FRAC_PI_2,
        };
        assert!(close(steer_yaw(&steers(side), feet).unwrap(), FRAC_PI_2));
        // From a fixed camera to the east, forward is west.
        let fixed = ShotSaid::Fixed {
            from: [10.0, 4.0, 0.0],
            at: CameraTarget::Character(who),
        };
        assert!(close(steer_yaw(&steers(fixed), feet).unwrap(), FRAC_PI_2));
        // A camera that trails is not steered by.
        let follow = ShotSaid::Follow {
            who,
            distance: 4.0,
            height: 2.0,
        };
        assert_eq!(steer_yaw(&steers(follow), feet), None);
    }

    #[test]
    fn a_cut_is_at_once() {
        let mut camera = ShotCamera::default();
        camera.pose(&CameraSaid::default(), PLAYER, 0.0, nobody);
        let shot = ShotSaid::Fixed {
            from: [3.0, 4.0, 5.0],
            at: CameraTarget::Point([0.0; 3]),
        };
        let pose = camera.pose(&said(shot, 0.0, 1), PLAYER, 0.0, nobody);
        assert_eq!(pose.position, [3.0, 4.0, 5.0]);
        assert_eq!(
            camera.pose(&said(ShotSaid::Player, 0.0, 2), PLAYER, 0.0, nobody),
            PLAYER
        );
    }

    #[test]
    fn a_follow_shot_trails_at_its_distance_and_height() {
        let mut camera = ShotCamera::default();
        // The view was south of them (+Z), so that is where it trails from.
        let from = Pose {
            position: [0.0, 1.7, 2.0],
            ..PLAYER
        };
        camera.pose(&CameraSaid::default(), from, 0.0, nobody);
        let shot = ShotSaid::Follow {
            who: CharacterId::PLAYER,
            distance: 5.0,
            height: 3.0,
        };
        let asked = said(shot, 0.0, 1);
        let pose = camera.pose(&asked, from, 0.0, |_| Some([0.0; 3]));
        assert!(close(pose.position[0], 0.0) && close(pose.position[2], 5.0));
        assert!(close(pose.position[1], 3.0));
        assert!(close(pose.yaw, 0.0) && pose.pitch < 0.0);

        // They walk north; it is dragged along behind, still 5 m off.
        for step in 1..=50u8 {
            let z = -0.1 * f32::from(step);
            let pose = camera.pose(&asked, from, f64::from(step) / 60.0, |_| {
                Some([0.0, 0.0, z])
            });
            assert!(close(pose.position[2] - z, 5.0), "{:?}", pose.position);
        }
        // They turn east and walk on: the camera swings round to trail them.
        let mut last = [0.0; 3];
        for step in 1..=400u16 {
            let x = 0.1 * f32::from(step);
            last = [x, 0.0, -5.0];
            let pose = camera.pose(&asked, from, 1.0 + f64::from(step) / 60.0, |_| Some(last));
            let off = (pose.position[0] - x).hypot(pose.position[2] + 5.0);
            assert!(close(off, 5.0), "{off}");
        }
        let pose = camera.pose(&asked, from, 9.0, |_| Some(last));
        assert!(
            pose.position[0] < last[0] - 4.5,
            "behind them: {:?}",
            pose.position
        );
    }
}
