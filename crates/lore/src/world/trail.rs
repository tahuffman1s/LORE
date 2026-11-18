//! What whoever walks does to the snow that lies: a print at each step,
//! and in snow deep enough to wade through, a furrow.
//!
//! This only says where the snow is pressed, for the picture to show
//! (`lore_render::Tread`): the rules do not read tracks back. That deep
//! snow is slow to walk through is the climate's
//! (`Climate::snow_slows`).

use lore_physics::Player;
use lore_render::Tread;

/// How deep snow has to lie before a foot leaves a mark in it, in metres.
const MARKS_FROM: f32 = 0.004;
/// How far to the side of the way they go each foot comes down, in metres,
/// and how far in front of the middle of a boot its sole is pressed and
/// behind it its heel.
const FEET_APART: f32 = 0.11;
const BOOT: f32 = 0.075;
/// How deep snow is waded through and no longer stepped over, in metres:
/// from where a furrow begins to where it is as deep as it gets.
const WADED: (f32, f32) = (0.1, 0.3);
/// How far apart the treads of a furrow are, in metres.
const FURROW_EVERY: f32 = 0.07;

/// Whoever is leaving a trail: which foot comes down next, where they
/// were a tick ago if they were on snow then, and how far they have come
/// since the last print and since the furrow was last pressed.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Trail {
    left_next: bool,
    last: Option<[f32; 2]>,
    strode: f32,
    ploughed: f32,
}

impl Trail {
    /// Adds to `treads` what `player`, who has just moved and faces
    /// `yaw`, did to snow that lies `snow` metres deep in the tick just
    /// run. `landed` says that they came down from a jump or a fall in it.
    ///
    /// Prints are a stride apart whatever is heard of the feet: the sound
    /// of a step is paced for the ear, and a trail is looked at.
    pub(crate) fn tick(
        &mut self,
        player: &Player,
        yaw: f32,
        (snow, landed): (f32, bool),
        treads: &mut Vec<Tread>,
    ) {
        // In the air, or in water, or on bare ground: no trail, and the
        // next begins where they come down.
        if snow < MARKS_FROM || !player.grounded || player.submerged > 0.0 {
            self.last = None;
            return;
        }
        let [x, _, z] = player.position;
        let [along_x, _, along_z] = player.velocity;
        let speed = along_x.hypot(along_z);
        // The way they are going, or the way they face if they are
        // standing: feet point somewhere.
        let way = if speed > 0.05 {
            [along_x / speed, along_z / speed]
        } else if yaw.is_finite() {
            [-yaw.sin(), -yaw.cos()]
        } else {
            [0.0, -1.0]
        };
        // How far they came in this tick: nothing if they have only now
        // come onto the snow, or were put somewhere else.
        let came = (self.last.replace([x, z]))
            .map(|last| (x - last[0]).hypot(z - last[1]))
            .filter(|came| *came < 2.0);
        let foot = |side: f32, treads: &mut Vec<Tread>| {
            // A heel and a sole, which together are as long as a boot.
            let middle = [
                x - way[1] * side * FEET_APART,
                z + way[0] * side * FEET_APART,
            ];
            for along in [-BOOT, BOOT] {
                treads.push(Tread {
                    place: [middle[0] + way[0] * along, middle[1] + way[1] * along],
                    wide: 0.085,
                    deep: 1.0,
                });
            }
        };
        let Some(came) = came else {
            // Both feet, where they stand or have come down; harder from
            // a height.
            if landed {
                treads.push(Tread {
                    place: [x, z],
                    wide: 0.3,
                    deep: 1.0,
                });
            } else {
                foot(-1.0, treads);
                foot(1.0, treads);
            }
            (self.strode, self.ploughed) = (0.0, 0.0);
            return;
        };
        if landed {
            treads.push(Tread {
                place: [x, z],
                wide: 0.3,
                deep: 1.0,
            });
        }
        // A stride is longer the faster they go: three quarters of a
        // metre at a walk, half as much again at a run.
        self.strode += came;
        if self.strode >= (0.5 + 0.12 * speed).clamp(0.6, 1.25) {
            self.strode = 0.0;
            foot(if self.left_next { -1.0 } else { 1.0 }, treads);
            self.left_next = !self.left_next;
        }
        // Deep snow is not stepped over: legs plough through it.
        let waded = ((snow - WADED.0) / (WADED.1 - WADED.0)).clamp(0.0, 1.0);
        self.ploughed += came;
        if waded > 0.0 && self.ploughed >= FURROW_EVERY {
            self.ploughed = 0.0;
            // Not as deep as the feet go: their prints are in its floor.
            treads.push(Tread {
                place: [x, z],
                wide: 0.19 + 0.07 * waded,
                deep: 0.25 + 0.5 * waded,
            });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn walker(x: f32) -> Player {
        Player {
            velocity: [3.0, 0.0, 0.0],
            grounded: true,
            ..Player::at([x, 0.0, 0.0], &lore_physics::MoveSettings::default())
        }
    }

    #[test]
    fn feet_leave_prints_to_either_side_and_deep_snow_is_ploughed() {
        let mut trail = Trail::default();
        let mut treads = Vec::new();
        // Bare ground takes no print.
        trail.tick(&walker(0.0), 0.0, (0.0, false), &mut treads);
        assert!(treads.is_empty());
        // A dusting: both feet where they come onto it, a heel and a sole
        // to each, then a print a stride, left and right in turn.
        trail.tick(&walker(0.0), 0.0, (0.03, false), &mut treads);
        assert_eq!(treads.len(), 4);
        assert!((treads[0].place[1] + treads[2].place[1]).abs() < 1e-6);
        assert!((treads[0].place[1].abs() - FEET_APART).abs() < 1e-6);
        assert!((treads[1].place[0] - treads[0].place[0] - 2.0 * BOOT).abs() < 1e-6);
        treads.clear();
        // Three metres a second for five metres: a stride is 0.86 m.
        for step in 1..=100 {
            trail.tick(&walker(step as f32 * 0.05), 0.0, (0.03, false), &mut treads);
        }
        assert_eq!(treads.len(), 2 * 5, "{treads:?}");
        assert!(treads[0].place[1] * treads[2].place[1] < 0.0, "the other foot");
        // To the knee: a furrow as well, a tread every ten centimetres,
        // and nothing while they stand.
        treads.clear();
        for step in 101..=120 {
            trail.tick(&walker(step as f32 * 0.05), 0.0, (0.4, false), &mut treads);
        }
        let furrow = treads.iter().filter(|tread| tread.deep < 1.0).count();
        assert_eq!(furrow, 10);
        let stood = treads.len();
        trail.tick(&walker(6.0), 0.0, (0.4, false), &mut treads);
        assert_eq!(treads.len(), stood);
        // Off the ground they press nothing, and where they come down
        // again is one deep mark.
        let jumping = Player {
            grounded: false,
            ..walker(7.0)
        };
        trail.tick(&jumping, 0.0, (0.4, false), &mut treads);
        assert_eq!(treads.len(), stood);
        trail.tick(&walker(8.0), 0.0, (0.4, true), &mut treads);
        assert_eq!(treads.len(), stood + 1);
        assert!(treads[stood].wide > 0.25);
        // Whoever stands still on new snow has two feet side by side,
        // pointing the way they face: here along x.
        let mut trail = Trail::default();
        treads.clear();
        let standing = Player {
            velocity: [0.0; 3],
            ..walker(0.0)
        };
        trail.tick(&standing, -std::f32::consts::FRAC_PI_2, (0.03, false), &mut treads);
        assert_eq!(treads.len(), 4);
        assert!((treads[0].place[1] - treads[2].place[1]).abs() > 2.0 * FEET_APART - 1e-4);
        assert!((treads[1].place[0] - treads[0].place[0] - 2.0 * BOOT).abs() < 1e-4);
    }
}
