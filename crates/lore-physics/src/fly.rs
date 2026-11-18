//! Flying: moving along the view with no gravity, for a game's debug tools.

use crate::level::{Level, Vector};
use crate::player::Player;
use crate::settings::{MoveInput, MoveMode, MoveSettings, positive};
use crate::slide::{slide, unstick};

/// How many times faster sprinting flies.
const SPRINT_FACTOR: f32 = 2.0;

/// How quickly a flyer's velocity closes on the wanted one, per second:
/// starting and stopping are quick but not a jerk.
const EASE_RATE: f32 = 10.0;

impl Player {
    /// One tick of flying (`Fly` or `Noclip`): the velocity eases toward
    /// where the input points, and the body moves by it.
    pub(crate) fn fly(
        &mut self,
        level: &Level,
        settings: &MoveSettings,
        input: &MoveInput,
        dt: f32,
    ) {
        let (sin, cos) = (libm::sinf(input.yaw), libm::cosf(input.yaw));
        let (sin_pitch, cos_pitch) = (libm::sinf(input.pitch), libm::cosf(input.pitch));
        let ahead = Vector::new(-sin * cos_pitch, sin_pitch, -cos * cos_pitch);
        let right = Vector::new(cos, 0.0, -sin);
        let rise = f32::from(u8::from(input.jump)) - f32::from(u8::from(input.crouch));

        let wish = ahead * input.forward + right * input.right + Vector::Y * rise;
        let wish = wish.clamp_length_max(1.0);
        let speed = positive(settings.fly_speed) * if input.sprint { SPRINT_FACTOR } else { 1.0 };
        let wanted = wish * speed;

        let eased = 1.0 - libm::expf(-EASE_RATE * dt);
        let velocity = Vector::from_array(self.velocity);
        let velocity = velocity + (wanted - velocity) * eased;

        // The standing body, whatever it was before.
        let shape = settings.shape(false);
        let to_middle = Vector::Y * (settings.height(false) / 2.0);
        let feet = Vector::from_array(self.position);
        let (feet, velocity) = if input.mode == MoveMode::Noclip {
            (feet + velocity * dt, velocity)
        } else {
            let middle = unstick(level, &shape, feet + to_middle);
            let moved = slide(
                level,
                &shape,
                middle,
                velocity,
                dt,
                settings.flattest_wall(),
                false,
            );
            // Keep the speed the flyer asked for; sliding only turns the
            // position, so pushing on a wall does not build up speed.
            (moved.position - to_middle, moved.velocity)
        };
        if feet.is_finite() && velocity.is_finite() {
            self.position = feet.to_array();
            self.velocity = velocity.to_array();
        } else {
            self.velocity = [0.0; 3];
        }

        self.grounded = false;
        self.crouched = false;
        self.sprinting = input.sprint;
        self.eye_height = settings.eye_height(false);
        self.ground_normal = [0.0, 1.0, 0.0];
        self.jump_held = input.jump;
        self.swimming = false;
        self.submerged = 0.0;
        self.head_under = false;
        self.stroke = 0.0;
        self.tread = 0.0;
        self.hoist = 0.0;
    }
}
