//! Lamps between the script and the renderer: the script says them in its
//! own terms ([`LampLight`]), the renderer draws [`lore_render::Lamp`]s, and
//! a level's lights are handed to the script by name when it is loaded.

use lore_assets::Scene;
use lore_render::{Beam, Lamp};
use lore_script::{LampBeam, LampLight};

/// The lamp the renderer draws for one the script has lit.
pub(crate) fn drawn(light: &LampLight) -> Lamp {
    Lamp {
        at: light.at,
        candela: light.candela,
        colour: light.colour,
        reach: light.reach,
        radius: light.radius,
        beam: light.beam.map(|beam| Beam {
            pointing: beam.pointing,
            inner: beam.inner,
            outer: beam.outer,
        }),
        casts_shadows: light.casts_shadows,
    }
}

/// The lamps `level` places, each by the name of its light in the file, as
/// the script takes them. Names are told apart as the script tells them
/// (without case or spaces at either end), and glTF does not ask for them
/// to differ, so a name used before gets " 2", " 3" and so on, and a light
/// with none is "lamp 1", "lamp 2" and so on: no light of the file is lost.
pub(crate) fn of_level(level: &Scene) -> Vec<(String, LampLight)> {
    let mut taken = std::collections::HashSet::new();
    let mut unnamed = 0;
    let mut unique = |name: Option<&str>| {
        let base = match name.map(str::trim).filter(|name| !name.is_empty()) {
            Some(name) => name.to_owned(),
            None => {
                unnamed += 1;
                format!("lamp {unnamed}")
            }
        };
        let mut name = base.clone();
        let mut again = 1;
        while !taken.insert(name.to_lowercase()) {
            again += 1;
            name = format!("{base} {again}");
        }
        name
    };
    level
        .lights
        .iter()
        .filter_map(|light| {
            let lamp = Lamp::from_light(light)?;
            let name = unique(light.name.as_deref());
            let said = LampLight {
                at: lamp.at,
                candela: lamp.candela,
                colour: lamp.colour,
                reach: lamp.reach,
                radius: lamp.radius,
                beam: lamp.beam.map(|beam| LampBeam {
                    pointing: beam.pointing,
                    inner: beam.inner,
                    outer: beam.outer,
                }),
                casts_shadows: lamp.casts_shadows,
            };
            Some((name, said))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use lore_assets::{Light, LightKind};

    #[test]
    fn a_level_s_lamps_go_to_the_script_by_name_and_come_back_the_same() {
        let mut level = Scene::default();
        let light = |name: Option<&str>, kind| Light {
            name: name.map(str::to_owned),
            kind,
            color: [1.0, 0.8, 0.6],
            intensity: 40.0,
            range: Some(9.0),
            position: [1.0, 2.0, 3.0],
            direction: [0.0, -1.0, 0.0],
        };
        level.lights = vec![
            light(Some("Lantern"), LightKind::Point),
            light(None, LightKind::Sun),
            light(
                None,
                LightKind::Spot {
                    inner_angle: 0.2,
                    outer_angle: 0.4,
                },
            ),
        ];
        let lamps = of_level(&level);
        let names: Vec<&str> = lamps.iter().map(|(name, _)| name.as_str()).collect();
        assert_eq!(names, ["Lantern", "lamp 1"]);
        // Names that the script would take for one are told apart.
        level.lights.extend([
            light(Some("lantern "), LightKind::Point),
            light(Some("Lamp 1"), LightKind::Point),
        ]);
        let names: Vec<String> = of_level(&level).into_iter().map(|(name, _)| name).collect();
        assert_eq!(names, ["Lantern", "lamp 1", "lantern 2", "Lamp 1 2"]);
        let back = drawn(&lamps[1].1);
        assert_eq!(back, Lamp::from_light(&level.lights[2]).unwrap());
    }
}
