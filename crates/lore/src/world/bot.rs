//! The bot in a world: what the script asked of it (`game.bot`), played
//! with the player's own controls.
//!
//! Before each tick [`Playing`](super::Playing) asks the world what the
//! bot presses ([`World::bot_frame`]): the bot reads what a player could
//! know (the player's body and view, the walkable ground, what is in view,
//! what the crosshair is on) and its hands are written into the tick's
//! input as a device would write them. From there the tick is any other:
//! the player's body moves by `Player::step` from what is pressed, the
//! weapon fires from the trigger, the use button uses. So a recording of
//! the input replays the bot's game exactly with no bot, and the same plan
//! plays the same game every time.
//!
//! A world whose script never mentions the bot has none, and this costs it
//! one look at an empty list a tick.

use lore_input::{ActionId, ActionState, InputFrame};
use lore_nav::{GateId, Openings, Scratch};
use lore_npc::{Arms, Around, Bot, BotNews, Hands, Seen, Skill, Spot, Step, Stuck};
use lore_script::{BotAsk, BotTarget, CharacterId};

use super::{Controls, World, using};
use crate::look;

/// How far above an NPC's feet the bot looks to look at them: the chest.
const CHEST: f32 = 1.3;

/// How long the bot's game has run, for its story: minutes, seconds and
/// tenths.
fn clock(tick: u32, tick_rate: u32) -> String {
    let tenths = u64::from(tick) * 10 / u64::from(tick_rate.max(1));
    format!("{}:{:02}.{}", tenths / 600, tenths / 10 % 60, tenths % 10)
}

/// What the bot's story says when it has done every step of its plan.
pub const FINISHED: &str = "finished the plan";

/// One thing the bot did, or could not do, for `lore check --play` to say.
#[derive(Debug, Clone, PartialEq)]
pub struct BotLine {
    /// The tick it happened on.
    pub tick: u32,
    /// Where the player's feet were.
    pub at: [f32; 3],
    /// What happened, in words: "got to Lookout", or why it is stuck.
    pub said: String,
    /// Whether the bot is stuck, and gave up its plan.
    pub stuck: bool,
}

impl BotLine {
    /// The line as `lore check --play` prints it: game time, place, words.
    #[must_use]
    pub fn written(&self, tick_rate: u32) -> String {
        let [x, y, z] = self.at;
        format!(
            "{}  at ({x:.1}, {y:.1}, {z:.1})  {}",
            clock(self.tick, tick_rate),
            self.said
        )
    }
}

/// The bot of a world and what it has done.
#[derive(Debug)]
pub(crate) struct BotPlay {
    bot: Bot<BotTarget, ActionId>,
    scratch: Scratch,
    may_open: Openings,
    /// What its hands held on the last tick, for what is pressed and let
    /// go on this one.
    last: Hands<ActionId>,
    /// Whether it drove the controls on the last tick.
    drove: bool,
    story: Vec<BotLine>,
}

impl BotPlay {
    fn new(seed: u64) -> Self {
        Self {
            bot: Bot::new(seed),
            scratch: Scratch::default(),
            may_open: Openings::default(),
            last: Hands::default(),
            drove: false,
            story: Vec::new(),
        }
    }
}

/// `state` for a button held (`now`) after `was`.
fn button(now: bool, was: bool) -> ActionState {
    ActionState {
        pressed: now,
        just_pressed: now && !was,
        just_released: was && !now,
        axis: [if now { 1.0 } else { 0.0 }, 0.0],
        delta: [0.0; 2],
    }
}

impl World {
    /// The bot's story: everything it did and where it got stuck, oldest
    /// first. Empty in a world whose script never gave it anything to do.
    #[must_use]
    pub fn bot_story(&self) -> &[BotLine] {
        self.bot.as_ref().map_or(&[], |play| play.story.as_slice())
    }

    /// Whether the bot has steps left to do.
    #[must_use]
    pub fn bot_playing(&self) -> bool {
        self.bot.as_ref().is_some_and(|play| play.bot.playing())
    }

    /// Adds `step` to the end of the bot's plan, as `game.bot` does: for
    /// the engine's own bots (a level walked by the editor, a story played
    /// through by `lore check`).
    pub fn bot_add(&mut self, step: lore_script::BotStep) {
        let seed = self.bot_seed();
        let play = self.bot.get_or_insert_with(|| Box::new(BotPlay::new(seed)));
        play.bot.add(step);
    }

    /// Gives up the bot's plan: the controls are the person's again.
    pub fn bot_stop(&mut self) {
        if let Some(play) = &mut self.bot {
            play.bot.stop();
        }
    }

    /// Whether the bot drove the controls on the newest tick.
    pub(crate) fn bot_drove(&self) -> bool {
        self.bot.as_ref().is_some_and(|play| play.drove)
    }

    /// The bot's dice start from the world's own.
    fn bot_seed(&self) -> u64 {
        u64::from(self.dice) << 32 | 0xB07
    }

    /// The ground changed: ways found on the old one mean nothing.
    pub(super) fn bot_level_changed(&mut self) {
        if let Some(play) = &mut self.bot {
            play.bot.walk_mut().level_changed();
        }
    }

    /// Writes what the bot presses for the coming tick into `out`, over
    /// `person`, the input of the person's own devices: true when the bot
    /// drives the controls, and `out` is the tick's input. `view` is where
    /// the player looks (yaw, pitch); `controls` the player's actions.
    ///
    /// The actions the bot drives are the bot's alone while it plays. Any
    /// other button the person presses goes through; and if the game lets
    /// them (`game.bot.playerTakesOver`, on unless it says), a button the
    /// person presses or a turn of the mouse takes the controls back.
    pub(super) fn bot_frame(
        &mut self,
        person: &InputFrame,
        view: [f32; 2],
        controls: &Controls,
        out: &mut InputFrame,
    ) -> bool {
        let asks = self.game.host_mut().take_bot_asks();
        if !asks.is_empty() {
            let seed = self.bot_seed();
            let play = self.bot.get_or_insert_with(|| Box::new(BotPlay::new(seed)));
            for ask in asks {
                match ask {
                    BotAsk::Add(step) => play.bot.add(step),
                    BotAsk::Stop => play.bot.stop(),
                }
            }
        }
        let Some(play) = &self.bot else {
            return false;
        };
        // While time stands still (the game is paused, or a screen holds
        // it), the bot waits, and the person has the controls.
        if self.game.host().time_speed() == 0.0 {
            return false;
        }
        if !play.bot.playing() && !play.drove {
            return false;
        }
        if play.bot.playing()
            && self.game.host().bot_yields_to_player()
            && person.touched()
            && let Some(play) = &mut self.bot
        {
            // The person's own input goes through as it is, their press
            // with it: nothing of the bot's is let go over it.
            play.bot.stop();
            play.last = Hands::default();
            play.drove = false;
            log::info!("A button was pressed: the bot let go of the controls.");
            self.game.host_mut().set_bot_playing(false, 0);
            return false;
        }
        let hands = self.bot_hands(view);
        let Some(play) = &mut self.bot else {
            return false;
        };
        let last = play.last;
        play.last = hands;
        // A tick after it stops, to let go of what it held.
        play.drove = play.bot.playing() || hands.busy();
        let driving = play.drove || last.busy();
        if !driving {
            return false;
        }
        out.clone_from(person);
        let mut set = |action: Option<ActionId>, state: ActionState| {
            if let Some(action) = action {
                out.set(action, state);
            }
        };
        let walking = hands.walk != [0.0; 2];
        let walked = last.walk != [0.0; 2];
        set(
            controls.walk,
            ActionState {
                pressed: walking,
                just_pressed: walking && !walked,
                just_released: walked && !walking,
                axis: hands.walk,
                delta: [0.0; 2],
            },
        );
        // The view is turned as a mouse turns it: by counts.
        let counts = hands.turn.map(|turn| turn / look::MOUSE_TURN);
        let turning = counts != [0.0; 2];
        set(
            controls.look,
            ActionState {
                pressed: turning,
                just_pressed: turning && last.turn == [0.0; 2],
                just_released: !turning && last.turn != [0.0; 2],
                axis: [0.0; 2],
                delta: counts,
            },
        );
        set(controls.jump, button(hands.jump, last.jump));
        set(controls.crouch, button(hands.crouch, last.crouch));
        set(controls.sprint, button(hands.sprint, last.sprint));
        set(controls.fire, button(hands.fire, last.fire));
        set(controls.aim, button(hands.aim, last.aim));
        set(controls.reload, button(hands.reload, last.reload));
        set(controls.interact, button(hands.interact, last.interact));
        if last.action.is_some() && last.action != hands.action {
            set(last.action, button(false, true));
        }
        if hands.action.is_some() {
            set(hands.action, button(true, last.action == hands.action));
        }
        true
    }

    /// What the bot's hands do this tick, and what came of its steps told
    /// to the script and kept in its story.
    fn bot_hands(&mut self, view: [f32; 2]) -> Hands<ActionId> {
        let tick = self.tick.saturating_add(1);
        let dt = self.ground.step;
        let skill = Skill::of_aim(self.difficulty().aim);
        let Some(play) = self.bot.as_deref_mut() else {
            return Hands::default();
        };
        let host = self.game.host();
        let body = &self.player.player;
        let eyes = body.eye_position();
        // A way, if it wants one: doors the player may open count as open.
        if let Some((from, to)) = play.bot.walk().wants_way(body) {
            play.may_open.clone_from(self.ground.openings());
            for (gate, id) in self.things.door_ids().enumerate() {
                if host.door_may_open(id) {
                    // Fewer doors than fit in a u32.
                    #[allow(clippy::cast_possible_truncation)]
                    play.may_open.set_gate(GateId(gate as u32), true);
                }
            }
            let stayed = play.bot.walk().stayed_shut();
            if let Some(gate) = stayed {
                play.may_open.set_gate(gate, false);
            }
            let found = self.ground.walkable().way_into(
                from,
                to,
                &play.may_open,
                &mut play.scratch,
                play.bot.walk_mut().way_mut(),
            );
            play.bot.walk_mut().found(found, tick, dt);
        }
        let level = &self.ground.level;
        let in_view = |at: [f32; 3]| {
            let to: [f32; 3] = std::array::from_fn(|i| at[i] - eyes[i]);
            let far = (to[0] * to[0] + to[1] * to[1] + to[2] * to[2]).sqrt();
            if !far.is_finite() || far < 1e-3 {
                return true;
            }
            let way = to.map(|part| part / far);
            // A thing's own shape may stand in the way of its middle.
            level
                .raycast(eyes, way, far)
                .is_none_or(|hit| hit.distance >= far * 0.8)
        };
        let meant_now = self.using.target.as_ref();
        let usable = |key: lore_script::Usable<'_>| -> Option<(using::Usable, bool)> {
            let nth = self.usable_ids.iter().position(|id| id.meant() == key)?;
            let meant = meant_now.is_some_and(|now| now.meant() == key);
            Some((self.usable[nth], meant))
        };
        let thing_spot = |id: &str| -> Option<Spot> {
            let (at, use_said) = match usable(lore_script::Usable::Thing(id)) {
                Some((thing, meant)) => (thing.at, Some((thing, meant))),
                None => (host.thing_place(id)?, None),
            };
            let feet = self.ground.feet_under(at).unwrap_or(at);
            Some(Spot {
                feet,
                aim: at,
                moves: false,
                in_view: in_view(at),
                meant: use_said.is_some_and(|(_, meant)| meant),
                reach: use_said.map_or(0.0, |(thing, _)| thing.reach),
                hold: use_said.map_or(0.0, |(thing, _)| thing.seconds),
                down: false,
            })
        };
        let target = match play.bot.step(tick).and_then(Step::target) {
            None => None,
            Some(BotTarget::Place { at, .. }) => Some(Spot {
                in_view: in_view([at[0], at[1] + 0.3, at[2]]),
                ..Spot::place(*at)
            }),
            Some(BotTarget::Thing(id)) => thing_spot(id),
            Some(BotTarget::Who(who)) => {
                let npc = self.npcs.list().iter().find(|npc| npc.who == *who);
                npc.map(|npc| {
                    let feet = npc.body.position;
                    let aim = [feet[0], feet[1] + CHEST, feet[2]];
                    let use_said = usable(lore_script::Usable::Npc(*who));
                    Spot {
                        feet,
                        aim,
                        moves: true,
                        in_view: in_view(aim),
                        meant: use_said.is_some_and(|(_, meant)| meant),
                        reach: use_said.map_or(0.0, |(thing, _)| thing.reach),
                        hold: use_said.map_or(0.0, |(thing, _)| thing.seconds),
                        down: npc.down,
                    }
                })
            }
        };
        let door = (play.bot.door())
            .and_then(|gate| self.things.door_ids().nth(gate.0 as usize))
            .and_then(thing_spot);
        let arms = host.held_weapon(CharacterId::PLAYER).map(|held| Arms {
            loaded: held.state.magazine,
            reloading: held.state.reload.is_some(),
        });
        let (places, going) = self.npcs.crowd();
        let around = Around {
            level,
            walkable: self.ground.walkable(),
            openings: self.ground.openings(),
            tick,
            seconds: dt,
            // The bot is the player: it does not walk round itself.
            player: [f32::INFINITY; 3],
            npcs: places,
            going,
        };
        let seen = Seen {
            around: &around,
            // The player is the last of the places, after every NPC.
            me: places.len().saturating_sub(1),
            body,
            view,
            eyes,
            target,
            door,
            arms,
            skill,
        };
        let hands = play.bot.play(&seen);
        let at = body.position;
        let host = self.game.host_mut();
        let mut done = false;
        let mut stuck = None;
        while let Some(news) = play.bot.take_news() {
            match news {
                BotNews::Did(step) => {
                    let said = did(host, &step);
                    play.story.push(BotLine {
                        tick,
                        at,
                        said,
                        stuck: false,
                    });
                }
                BotNews::Done => {
                    play.story.push(BotLine {
                        tick,
                        at,
                        said: FINISHED.to_owned(),
                        stuck: false,
                    });
                    done = true;
                }
                BotNews::Stuck { step, why } => {
                    let said = stuck_said(host, &step, &why);
                    play.story.push(BotLine {
                        tick,
                        at,
                        said: said.clone(),
                        stuck: true,
                    });
                    stuck = Some(said);
                }
            }
        }
        let playing = play.bot.playing();
        host.set_bot_playing(playing, play.bot.steps_left());
        if done {
            host.bot_done();
        }
        if let Some(said) = stuck {
            host.bot_stuck(&said);
        }
        hands
    }
}

/// What the bot did, in words: "got to Lookout".
fn did(host: &lore_script::ScriptHost, step: &lore_script::BotStep) -> String {
    let name = |target: &BotTarget| host.bot_target_name(target);
    match step {
        Step::GoTo { to, .. } => format!("got to {}", name(to)),
        Step::LookAt { at } => format!("looked at {}", name(at)),
        Step::Use { thing } => format!("used {}", name(thing)),
        Step::Press { action, seconds } if *seconds > 0.0 => {
            format!(
                "held \"{}\" for {seconds} seconds",
                host.action_name(*action)
            )
        }
        Step::Press { action, .. } => format!("pressed \"{}\"", host.action_name(*action)),
        Step::Attack { who } => format!("brought down {}", name(who)),
        Step::Wait { seconds } => format!("waited {seconds} seconds"),
    }
}

/// Why the bot is stuck, in a sentence: "The bot could not reach Gate: the
/// door \"Cellar door\" is locked".
fn stuck_said(host: &lore_script::ScriptHost, step: &lore_script::BotStep, why: &Stuck) -> String {
    let name = step
        .target()
        .map_or_else(|| "it".to_owned(), |target| host.bot_target_name(target));
    let doing = match step {
        Step::GoTo { .. } => "reach",
        Step::LookAt { .. } => "look at",
        Step::Use { .. } => "use",
        Step::Attack { .. } => "attack",
        Step::Press { .. } => "press",
        Step::Wait { .. } => "wait for",
    };
    match why {
        Stuck::Walk { why, to } => {
            let cause = why.cause(
                *to,
                |gate| host.door_name(gate.0),
                // Those in the way are not named by place here.
                |_| None,
            );
            format!("The bot could not {doing} {name}: {cause}")
        }
        Stuck::NeverInView {
            seconds,
            seen: true,
        } => format!(
            "The bot could see {name} for {seconds} seconds but could not {doing} it: \
             something may stand in front of it, or it may be too high to reach"
        ),
        Stuck::NeverInView { seconds, .. } => {
            format!("The bot looked for {name} for {seconds} seconds and never had it in view")
        }
        Stuck::CannotBeUsed => format!(
            "The bot could not use {name}: nothing happens when it is used. Give it a use \
             first, such as game.thing(\"{name}\"):onUse(function(thing, who) ... end), or \
             npc:onUse for an NPC"
        ),
        Stuck::Unarmed => format!(
            "The bot was to attack {name} but the player holds nothing to attack with: give \
             the player a weapon first, such as game.player:giveWeapon(\"rifle\")"
        ),
        Stuck::Gone => format!("The bot could not {doing} {name}: it is not in the world any more"),
    }
}
