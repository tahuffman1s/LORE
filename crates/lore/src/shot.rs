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
            if let ShotSaid::Follow { who, .. } = said.shot {
                // It trails from the side it is already on.
                let about = chest(who).unwrap_or(self.target);
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
