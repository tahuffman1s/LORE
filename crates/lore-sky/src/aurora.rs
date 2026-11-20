//! The aurora (#500): where the ring of light round the magnetic pole lies
//! for how stormy the sun's wind is, how far that is from a place, and so
//! how much of it the place sees.
//!
//! The Earth's field is taken as a magnet through its middle, its north end
//! at 80.8 degrees north, 72.6 west (IGRF-14, 2025). How stormy is the Kp
//! index forecasts give, 0 to 9: the ring's equatorward edge on the night
//! side lies at about 66.5 - 2.05 Kp degrees of magnetic latitude (NOAA's
//! table of Kp against where the aurora is overhead) and it reaches 7 + 1.2
//! Kp degrees towards the pole from there, so on a quiet night it is over
//! Tromsø, a Kp of 5 reaches Scotland, and a great storm (9), such as May
//! 2024's, is seen low in the north from the middle latitudes.

use crate::SkyPlace;

/// The north end of the Earth's magnet: latitude and longitude, degrees.
const MAGNETIC_POLE: (f64, f64) = (80.8, -72.6);

/// The magnetic latitude a world with no place on the Earth (the made-up
/// sky) is taken to lie at: a far north, under the ring at a Kp of 2.
pub const MADE_UP_MAGNETIC_LATITUDE: f64 = 62.0;

/// Kilometres a degree of latitude.
const KM_A_DEGREE: f64 = 111.2;

/// The Earth's radius, kilometres.
const EARTH: f64 = 6371.0;

/// How far north of the magnetic equator `place` lies, in degrees, and the
/// bearing from it to the magnetic pole, in degrees east of north.
#[must_use]
pub fn magnetic(place: &SkyPlace) -> (f64, f64) {
    let (lat, lon) = (place.latitude.to_radians(), place.longitude.to_radians());
    let (pole_lat, pole_lon) = (MAGNETIC_POLE.0.to_radians(), MAGNETIC_POLE.1.to_radians());
    let apart = lon - pole_lon;
    let latitude = (lat.sin() * pole_lat.sin() + lat.cos() * pole_lat.cos() * apart.cos())
        .clamp(-1.0, 1.0)
        .asin()
        .to_degrees();
    let bearing = (-apart.sin() * pole_lat.cos())
        .atan2(lat.cos() * pole_lat.sin() - lat.sin() * pole_lat.cos() * apart.cos())
        .to_degrees();
    (latitude, bearing)
}

/// The aurora as one place sees it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Seen {
    /// How far the middle of the ring is from here along the ground,
    /// towards the magnetic pole (below 0: it lies away from the pole), in
    /// kilometres.
    pub distance: f32,
    /// How far the ring reaches either side of its middle, kilometres.
    pub half_width: f32,
    /// The bearing to the magnetic pole, degrees east of north.
    pub bearing: f32,
    /// How bright its brightest curtains are, in candela a square metre.
    pub brightness: f32,
}

/// How bright the brightest curtains are at `kp`, in candela a square
/// metre: a faint arc at 3, a third as bright as the Milky Way's brightest
/// cloud; vivid green by 6; at 9 three hundredths of a candela, some
/// thirty times the Milky Way.
#[must_use]
pub fn brightness(kp: f32) -> f32 {
    if kp <= 0.0 {
        0.0
    } else {
        3e-5 * 10f32.powf(kp.min(9.0) / 3.0)
    }
}

/// The aurora at `kp` (0 to 9) seen from `place`, or from the made-up
/// world with none: nothing at 0.
#[must_use]
pub fn seen(kp: f32, place: Option<&SkyPlace>) -> Option<Seen> {
    if !(kp.is_finite() && kp > 0.0) {
        return None;
    }
    let kp = f64::from(kp.min(9.0));
    let (latitude, bearing) = place.map_or((MADE_UP_MAGNETIC_LATITUDE, 0.0), magnetic);
    // The south of the world sees the southern ring, its mirror.
    let (latitude, bearing) = if latitude < 0.0 {
        (-latitude, bearing + 180.0)
    } else {
        (latitude, bearing)
    };
    let edge = 66.5 - 2.05 * kp;
    let width = 7.0 + 1.2 * kp;
    let middle = edge + width / 2.0;
    #[allow(clippy::cast_possible_truncation)]
    Some(Seen {
        distance: ((middle - latitude) * KM_A_DEGREE) as f32,
        half_width: (width / 2.0 * KM_A_DEGREE) as f32,
        bearing: bearing as f32,
        brightness: brightness(kp as f32),
    })
}

impl Seen {
    /// How much of the aurora's light is seen from here, from 0 to 1:
    /// all of it under the ring, less as it sinks towards the horizon, none
    /// once its foot, 100 km up, is far below it.
    #[must_use]
    pub fn share_seen(&self) -> f32 {
        let near = (f64::from(self.distance.abs()) - f64::from(self.half_width)).max(0.0);
        // How high the foot of its nearest curtains stands, in degrees.
        let foot = (100.0 - near * near / (2.0 * EARTH))
            .atan2(near.max(1.0))
            .to_degrees();
        #[allow(clippy::cast_possible_truncation)]
        let up = ((foot + 2.0) / 12.0).clamp(0.0, 1.0) as f32;
        up * up * (3.0 - 2.0 * up)
    }

    /// The light the aurora gives open level ground, in lux: its curtains
    /// fill about a tenth of the sky, and less of it as they sink.
    #[must_use]
    pub fn lux(&self) -> f32 {
        0.1 * std::f32::consts::PI * self.brightness * self.share_seen()
    }
}

#[cfg(test)]
#[allow(clippy::expect_used)]
mod tests {
    use super::*;

    fn at(latitude: f64, longitude: f64) -> SkyPlace {
        SkyPlace {
            latitude,
            north: 0.0,
            first_day: 0,
            longitude,
        }
    }

    #[test]
    fn the_ring_lies_where_storms_bring_it() {
        // Tromsø is under the ring on a quiet night.
        let tromso = seen(3.0, Some(&at(69.65, 18.96))).expect("an aurora");
        assert!(tromso.distance.abs() < tromso.half_width, "{tromso:?}");
        assert!(tromso.share_seen() > 0.99);
        // Madrid sees nothing of a middling storm, but low in the north in
        // the greatest.
        assert_eq!(
            seen(5.0, Some(&at(40.4, -3.7)))
                .expect("an aurora")
                .share_seen(),
            0.0
        );
        let great = seen(9.0, Some(&at(40.4, -3.7))).expect("an aurora");
        assert!(
            great.share_seen() > 0.0 && great.distance > 0.0,
            "{great:?}"
        );
        // The magnetic pole (72.6 W) lies a little east of north from
        // Chicago (87.6 W), whose magnetic latitude is about 52.
        let (latitude, bearing) = magnetic(&at(41.9, -87.6));
        assert!((50.0..54.0).contains(&latitude), "{latitude}");
        assert!((0.0..10.0).contains(&bearing), "{bearing}");
        // The south of the world sees the southern ring.
        let hobart = seen(6.0, Some(&at(-42.9, 147.3))).expect("an aurora");
        assert!(hobart.share_seen() > 0.0, "{hobart:?}");
        assert!(seen(0.0, None).is_none());
        // Brighter the stormier.
        assert!(brightness(9.0) > 100.0 * brightness(2.0));
    }
}
