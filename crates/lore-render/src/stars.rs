//! The stars on the graphics card (#479): every star of
//! [`lore_sky::stars::stars`], sorted once into a grid on the six faces of
//! a cube among the stars, so that a pixel of the night sky looks only at
//! the few stars in its own square of the grid.
//!
//! A square holds every star that lies in it and every one near enough its
//! edge to light a pixel inside it, so no pixel ever looks in a second
//! square. `night_sky` in `mesh.wgsl` reads the grid: the faces, the
//! squares and the order of their numbers here must match it.

use std::sync::Arc;

use ash::vk;
use lore_sky::stars::{Star, stars};

use crate::buffer::Buffer;
use crate::error::RenderError;
use crate::gpu::Gpu;

/// How many squares along each edge of a face of the cube.
pub(crate) const FACE_SQUARES: usize = 32;

/// How far a star's light reaches from where it is, at most, in radians:
/// three times the widest spot a star is drawn as, in a wide view on a
/// small screen. A star that far outside a square still lights it.
const REACH: f32 = 0.007;

/// What the renderer is doing when it makes the grid.
const DOING: &str = "making the grid of the stars";

/// Which face of the cube `towards` goes through, and where on it, from
/// -1 to 1 across and up: faces 0 and 1 are +x and -x, 2 and 3 +y and -y,
/// 4 and 5 +z and -z.
pub(crate) fn on_face(towards: [f32; 3], face: usize) -> Option<[f32; 2]> {
    let axis = face / 2;
    let sign = if face.is_multiple_of(2) { 1.0 } else { -1.0 };
    let along = towards[axis] * sign;
    if along <= 0.0 {
        return None;
    }
    let [first, second] = match axis {
        0 => [1, 2],
        1 => [0, 2],
        _ => [0, 1],
    };
    Some([towards[first] / along, towards[second] / along])
}

/// The square of a face at `place` on it (from -1 to 1).
fn square(place: f32) -> usize {
    // A share of the edge, made a whole number of squares: small and not
    // below 0.
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_sign_loss,
        clippy::cast_precision_loss
    )]
    let at = ((place + 1.0) * 0.5 * FACE_SQUARES as f32).floor().max(0.0) as usize;
    at.min(FACE_SQUARES - 1)
}

/// The grid: for each square, where its stars start in the list and how
/// many there are; and the list, two rows of four numbers a star: towards
/// it and a number to turn its twinkling by, then its light in lux in red,
/// green and blue.
pub(crate) fn sorted(stars: &[Star]) -> (Vec<[u32; 2]>, Vec<[f32; 4]>) {
    let squares = 6 * FACE_SQUARES * FACE_SQUARES;
    let mut held: Vec<Vec<u32>> = vec![Vec::new(); squares];
    for (number, star) in stars.iter().enumerate() {
        for face in 0..6 {
            let Some([across, up]) = on_face(star.towards, face) else {
                continue;
            };
            let along = star.towards[face / 2].abs();
            // How far a reach of `REACH` goes on the face there: farther
            // from the face's middle a face is seen more slantwise.
            let reach = REACH / (along * along).max(0.1);
            if across.abs() > 1.0 + reach || up.abs() > 1.0 + reach {
                continue;
            }
            for row in square(up - reach)..=square(up + reach) {
                for column in square(across - reach)..=square(across + reach) {
                    let index = (face * FACE_SQUARES + row) * FACE_SQUARES + column;
                    if let (Some(list), Ok(number)) = (held.get_mut(index), u32::try_from(number)) {
                        list.push(number);
                    }
                }
            }
        }
    }
    let mut cells = Vec::with_capacity(squares);
    let mut list = Vec::new();
    for star_numbers in &held {
        let start = u32::try_from(list.len() / 2).unwrap_or(u32::MAX);
        let count = u32::try_from(star_numbers.len()).unwrap_or(0);
        cells.push([start, count]);
        for &number in star_numbers {
            let Some(star) = stars.get(number as usize) else {
                continue;
            };
            let [x, y, z] = star.towards;
            // Each star twinkles at its own moment.
            #[allow(clippy::cast_precision_loss)]
            let turn = ((number.wrapping_mul(2_654_435_761) >> 8) as f32) / 16_777_216.0;
            let lux = star.lux();
            list.push([x, y, z, turn]);
            let [red, green, blue] = star.colour;
            list.push([red * lux, green * lux, blue * lux, 0.0]);
        }
    }
    (cells, list)
}

/// The grid and the list on the graphics card.
pub(crate) struct StarGrid {
    pub(crate) cells: Buffer,
    pub(crate) list: Buffer,
}

impl StarGrid {
    /// Sorts every star into the grid and copies both to the graphics card.
    pub(crate) fn new(gpu: &Arc<Gpu>) -> Result<Self, RenderError> {
        let (cells, list) = sorted(stars());
        let cell_bytes: Vec<u8> = cells
            .iter()
            .flatten()
            .flat_map(|part| part.to_le_bytes())
            .collect();
        let mut list_bytes: Vec<u8> = list
            .iter()
            .flatten()
            .flat_map(|part| part.to_le_bytes())
            .collect();
        // A buffer may not be empty.
        if list_bytes.is_empty() {
            list_bytes.resize(32, 0);
        }
        let usage = vk::BufferUsageFlags::STORAGE_BUFFER;
        Ok(Self {
            cells: Buffer::holding(gpu, &cell_bytes, usage, DOING)?,
            list: Buffer::holding(gpu, &list_bytes, usage, DOING)?,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The square a shader would look in for `towards`.
    fn looked_in(towards: [f32; 3]) -> usize {
        let axis = (0..3)
            .max_by(|one, other| towards[*one].abs().total_cmp(&towards[*other].abs()))
            .unwrap_or(0);
        let face = axis * 2 + usize::from(towards[axis] < 0.0);
        let [across, up] = on_face(towards, face).unwrap_or([0.0; 2]);
        (face * FACE_SQUARES + square(up)) * FACE_SQUARES + square(across)
    }

    fn unit(v: [f32; 3]) -> [f32; 3] {
        let long = v.iter().map(|part| part * part).sum::<f32>().sqrt();
        v.map(|part| part / long)
    }

    #[test]
    fn every_star_is_found_from_every_pixel_its_light_reaches() {
        let (cells, list) = sorted(stars());
        assert_eq!(cells.len(), 6 * FACE_SQUARES * FACE_SQUARES);
        let mut checked = 0;
        for star in stars().iter().step_by(37) {
            // Points round the star, out to most of its reach, including
            // ones over an edge of the cube.
            for step in 0..8_u8 {
                let angle = f32::from(step) * std::f32::consts::FRAC_PI_4;
                let [x, y, z] = star.towards;
                let side = unit(if z.abs() < 0.9 {
                    [-y, x, 0.0]
                } else {
                    [0.0, -z, y]
                });
                let other = [
                    y * side[2] - z * side[1],
                    z * side[0] - x * side[2],
                    x * side[1] - y * side[0],
                ];
                let off = 0.8 * REACH;
                let at = unit([0, 1, 2].map(|axis| {
                    star.towards[axis]
                        + off * (angle.cos() * side[axis] + angle.sin() * other[axis])
                }));
                let [start, count] = cells[looked_in(at)];
                let found = (start..start + count).any(|entry| {
                    let row = list[entry as usize * 2];
                    row[..3] == star.towards[..]
                });
                assert!(found, "{star:?} not found from {at:?}");
                checked += 1;
            }
        }
        assert!(checked > 1000);
    }

    #[test]
    fn the_shader_reaches_as_far_as_a_square_holds() {
        let shader = include_str!("../shaders/mesh.wgsl");
        assert!(shader.contains(&format!("const STAR_REACH: f32 = {REACH};")));
        assert!(shader.contains(&format!("const STAR_FACE_SQUARES: u32 = {FACE_SQUARES}u;")));
    }

    #[test]
    fn a_square_holds_a_handful_of_stars() {
        let (cells, list) = sorted(stars());
        let most = cells.iter().map(|[_, count]| *count).max().unwrap_or(0);
        let average = list.len() as f32 / 2.0 / cells.len() as f32;
        assert!(average < 8.0, "{average}");
        assert!(most < 60, "{most}");
    }
}
