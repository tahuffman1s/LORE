//! What the NPCs see and hear, each tick.
//!
//! The pure part is `lore_npc`'s [`Senses`]: this fills in what it reads
//! from the world (who can be seen, how light it is where they stand, the
//! noises of the tick) and tells the script what came of it. It runs at the
//! end of the tick, after the script's own, so a sound the script played in
//! its tick is heard in the same one. NPCs go in the order of their ids and
//! each tells its news in order, so the same input tells the same events on
//! the same ticks.
//!
//! An NPC looks once every [`LOOK_EVERY`] ticks, the crowd taking turns by
//! id ([`Senses::looks_on`]); it listens every tick, since a noise lasts
//! one.
//! Who is out of reach or out of the cone costs no ray; one who may be
//! seen costs three, one at each of head, chest and knees.

use lore_npc::{LOOK_EVERY, Looker, Seeable, Sensed, Sight, Sound, daylight, haze, lamp_lux, lit};
use lore_physics::{Level, MoveSettings, Player};
use lore_script::{CharacterId, ScriptHost};

use super::npcs::{Npc, Npcs, height_of};
use crate::noise::{Names, Noise, Which};

/// How tall a body lying on the ground is, in metres, to be seen.
const LYING: f32 = 0.4;

/// What sensing keeps from tick to tick, so a tick sets no memory aside.
#[derive(Debug, Default)]
pub(crate) struct Sensing {
    /// Who can be seen this tick: the player, then every NPC by id.
    seeable: Vec<Seeable<CharacterId>>,
    /// How light it is where each of them stands, once worked out this
    /// tick; `NaN` until then.
    lights: Vec<f32>,
    /// The noises of the tick as the NPCs hear them, one for each noise.
    sounds: Vec<Sound<CharacterId, Which>>,
    /// What each NPC sensed this tick, in order, to tell the script.
    told: Vec<(CharacterId, Sensed<CharacterId>)>,
    /// How many rays were cast at the level in the newest tick.
    pub(crate) rays: u32,
}

/// What the world says of a tick to the senses.
pub(crate) struct Tick<'a> {
    /// The tick it is.
    pub(crate) tick: u32,
    /// The level's shapes, and the things fixed in it.
    pub(crate) level: &'a Level,
    /// How the bodies of people are sized.
    pub(crate) rules: &'a MoveSettings,
    /// The player's body.
    pub(crate) player: &'a Player,
    /// The noises of the tick, and the names they carry by id.
    pub(crate) noises: &'a [Noise],
    pub(crate) names: &'a Names,
    /// The weather, for fog, rain and clouds.
    pub(crate) weather: &'a lore_render::Weather,
}

impl Sensing {
    /// Each NPC looks, if it is its turn, and listens; then the script is
    /// told what they sensed.
    pub(crate) fn tick(&mut self, npcs: &mut Npcs, host: &mut ScriptHost, world: &Tick<'_>) {
        self.rays = 0;
        if npcs.list().is_empty() {
            return;
        }
        self.fill(npcs.list(), host, world);
        let sight = Sight {
            level: world.level,
            tick: world.tick,
            seeable: &self.seeable,
            haze: haze(world.weather.fog, world.weather.precipitation),
        };
        let open = daylight(host.time_of_day(), world.weather.cloud_cover);
        let (lights, told, rays) = (&mut self.lights, &mut self.told, &mut self.rays);
        told.clear();
        for npc in npcs.list_mut() {
            let who = npc.who;
            // Taken out by the script this tick: its body goes next tick.
            if !host.is_npc(who) {
                continue;
            }
            if npc.down {
                // A body sees and hears nothing, and says nothing of it.
                if npc.senses.sighted().next().is_some() || npc.senses.last_heard().is_some() {
                    npc.senses.forget();
                    host.npc_blinded(who);
                }
                continue;
            }
            let said = host.npc_senses(who);
            npc.senses.keen = lore_npc::Keenness {
                sight: said.sight_distance,
                width: said.sight_angle,
                hearing: said.hearing,
                night_sight: said.night_sight,
            };
            npc.senses.every = look_every(npc, world.player);
            let me = Looker {
                who,
                eyes: npc.eyes(),
                facing: npc.facing(),
            };
            if npc.senses.looks_on(world.tick, who.number()) {
                let light = |target: usize| light_at(lights, &sight, target, open, host);
                npc.senses
                    .look(&me, &sight, light, rays, |sensed| told.push((who, sensed)));
            }
            npc.senses
                .listen(&me, world.level, world.tick, &self.sounds, rays, |sensed| {
                    told.push((who, sensed))
                });
        }
        for &(npc, sensed) in &self.told {
            match sensed {
                Sensed::Saw(who) => host.npc_saw(npc, who),
                Sensed::Lost { who, at } => host.npc_lost_sight(npc, who, at),
                Sensed::Heard(nth) => {
                    if let Some(noise) = world.noises.get(nth) {
                        host.npc_heard(npc, nth, &noise.told(world.names));
                    }
                }
            }
        }
    }

    /// Fills in who can be seen this tick and what can be heard.
    fn fill(&mut self, npcs: &[Npc], host: &ScriptHost, world: &Tick<'_>) {
        let rules = world.rules;
        let player = world.player;
        let player_down = host
            .health(CharacterId::PLAYER)
            .is_some_and(|(now, _)| now <= 0.0);
        self.seeable.clear();
        self.seeable
            .push(seeable(CharacterId::PLAYER, player, rules, player_down));
        // One the script took out this tick is gone already: its body
        // goes next tick.
        (self.seeable).extend(
            (npcs.iter())
                .filter(|npc| host.is_npc(npc.who))
                .map(|npc| seeable(npc.who, &npc.body, rules, npc.down)),
        );
        self.lights.clear();
        self.lights.resize(self.seeable.len(), f32::NAN);
        self.sounds.clear();
        // One for each noise, in step with the list; one the script
        // silenced (game.onNoise answered false) is not played, so no NPC
        // hears it either.
        (self.sounds).extend(world.noises.iter().map(|noise| Sound {
            at: noise.position,
            loudness: if noise.silenced { 0.0 } else { noise.loudness },
            near: noise.range.near,
            far: noise.range.far,
            by: noise.by,
            what: Which {
                kind: noise.kind,
                sound: noise.sound,
            },
            of_world: noise.kind.is_of_the_world(),
            weapon: noise.weapon.is_some() || noise.kind.is_of_a_weapon(),
        }));
    }
}

/// Someone with the body `body`, as the NPCs see them.
fn seeable(
    who: CharacterId,
    body: &Player,
    rules: &MoveSettings,
    down: bool,
) -> Seeable<CharacterId> {
    Seeable {
        who,
        feet: body.position,
        height: if down { LYING } else { height_of(body, rules) },
        speed: libm::hypotf(body.velocity[0], body.velocity[2]),
        crouched: body.crouched,
        down,
    }
}

/// How many ticks pass between the looks of `npc`: [`LOOK_EVERY`] for
/// every NPC today. This is the one place a crowd's sight is made cheaper
/// by looking less often far from the player.
fn look_every(_npc: &Npc, _player: &Player) -> u32 {
    LOOK_EVERY
}

/// How light it is where the one at `target` in `sight.seeable` stands,
/// from 0 to 1: the open air's `open` light with the lamps of the world
/// over it, worked out once a tick for each.
fn light_at(
    lights: &mut [f32],
    sight: &Sight<'_, CharacterId>,
    target: usize,
    open: f32,
    host: &ScriptHost,
) -> f32 {
    let Some(kept) = lights.get_mut(target) else {
        return open;
    };
    if !kept.is_nan() {
        return *kept;
    }
    let light = if open >= 1.0 {
        1.0
    } else {
        let at = sight.seeable.get(target).map_or([0.0; 3], |seen| {
            let [x, y, z] = seen.feet;
            [x, y + seen.height * 0.5, z]
        });
        let mut lux = 0.0;
        host.lamps(|lamp| {
            let beam = lamp
                .beam
                .map(|beam| (beam.pointing, libm::cosf(beam.outer)));
            lux += lamp_lux(at, lamp.at, lamp.candela, lamp.reach, beam);
        });
        lit(open, lux)
    };
    *kept = light;
    light
}
