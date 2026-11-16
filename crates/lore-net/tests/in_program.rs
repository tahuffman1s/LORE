//! A server and its players in one program, with no socket between them:
//! everything goes as over UDP, but nothing waits on the machine, so a
//! run is counted in ticks and repeats to the last bit.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic, missing_docs)]

use std::time::Duration;

use lore_net::{
    Client, ClientSettings, Closed, Gone, HostEvent, HostSettings, LinkEvent, LinkSettings,
    PlayerId, PlayerInput, Refusal, Server, ServerSettings, UdpHost, UdpLink,
};
use lore_physics::{Level, MoveInput, MoveSettings, Player};

const STEP: f32 = 1.0 / 60.0;
/// One tick, as the host and its links count time.
const TICK: Duration = Duration::from_millis(20);

fn floor() -> Level {
    let s = 200.0;
    Level::from_triangles(&[
        [[-s, 0.0, s], [s, 0.0, s], [s, 0.0, -s]],
        [[-s, 0.0, s], [s, 0.0, -s], [-s, 0.0, -s]],
    ])
}

fn spawn() -> Player {
    Player::at([0.0, 0.5, 0.0], &MoveSettings::default())
}

fn walking(forward: f32, right: f32) -> PlayerInput {
    PlayerInput {
        movement: MoveInput {
            forward,
            right,
            ..MoveInput::default()
        },
        ..PlayerInput::default()
    }
}

/// A whole server with no socket.
struct Hosted {
    host: UdpHost,
    server: Server,
    level: Level,
    rules: MoveSettings,
    packet: Vec<u8>,
    places: usize,
    left: Vec<(PlayerId, Gone)>,
}

impl Hosted {
    fn new(settings: HostSettings) -> Self {
        Self {
            host: UdpHost::in_program(settings, TICK),
            server: Server::new(STEP, ServerSettings::default()),
            level: floor(),
            rules: MoveSettings::default(),
            packet: Vec::new(),
            places: usize::MAX,
            left: Vec::new(),
        }
    }

    fn tick(&mut self) {
        while let Some(event) = self.host.poll() {
            match event {
                HostEvent::Join(request) => {
                    if self.server.players().count() >= self.places {
                        self.host.refuse(request, Refusal::Full);
                    } else {
                        let id = self.server.join(spawn()).unwrap();
                        self.host.accept(request, id);
                    }
                }
                HostEvent::Packet(id, commands) => drop(self.server.receive(id, commands)),
                HostEvent::Left(id, why) => {
                    self.server.leave(id);
                    self.left.push((id, why));
                }
            }
        }
        self.server.tick(&self.level, &self.rules);
        for id in self.host.players() {
            self.server.write_snapshot(id, &mut self.packet).unwrap();
            self.host.send(id, &self.packet);
        }
    }

    fn join(&self) -> Joining {
        Joining {
            link: self.host.link_in_program(LinkSettings::default(), TICK),
            client: Client::new(spawn(), STEP, ClientSettings::default()),
            level: floor(),
            rules: MoveSettings::default(),
            packet: Vec::new(),
            closed: None,
        }
    }
}

/// A whole player in the server's program.
struct Joining {
    link: UdpLink,
    client: Client,
    level: Level,
    rules: MoveSettings,
    packet: Vec<u8>,
    closed: Option<Closed>,
}

impl Joining {
    fn tick(&mut self, input: &PlayerInput) {
        while let Some(event) = self.link.poll() {
            match event {
                LinkEvent::Joined(_) => {}
                LinkEvent::Packet(snapshot) => {
                    self.client
                        .receive(&self.level, &self.rules, snapshot)
                        .unwrap();
                }
                LinkEvent::Closed(why) => self.closed = Some(why),
            }
        }
        if self.link.id().is_some() {
            self.client
                .predict(&self.level, &self.rules, input, &mut self.packet);
            self.link.send(&self.packet);
        }
    }

    fn place(&self) -> [u32; 3] {
        self.client.player().position.map(f32::to_bits)
    }
}

/// Two players join and walk apart, then one leaves and the other stops. Gives
/// where everyone was after every tick, on each player's own side and on
/// the server.
fn a_game() -> Vec<[u32; 3]> {
    let mut hosted = Hosted::new(HostSettings::default());
    let (mut first, mut second) = (hosted.join(), hosted.join());
    let mut places = Vec::new();
    for tick in 0..150 {
        hosted.tick();
        // Walks, then stands for the server to catch up with what was
        // predicted.
        let forward = if tick < 110 { 1.0 } else { 0.0 };
        first.tick(&walking(forward, 0.0));
        if tick < 120 {
            second.tick(&walking(-0.5, 1.0));
        } else if tick == 120 {
            second.link.leave();
        }
        places.push(first.place());
        places.push(second.place());
        places.extend(
            hosted
                .server
                .players()
                .map(|(_, player)| player.position.map(f32::to_bits)),
        );
    }
    // Both were in by the third tick, with no waiting: asked, challenged,
    // answered, accepted.
    assert_eq!((first.link.id(), second.link.id()), (Some(0), None));
    assert_eq!(hosted.left, [(1, Gone::Quit)]);
    assert_eq!(second.closed, None);
    assert_eq!(hosted.host.players().collect::<Vec<_>>(), [0]);
    // The one who stayed walked, sees nobody now, and agrees with the
    // server about where they are.
    let here = first.client.player().position;
    assert!(here[2] < -5.0, "{here:?}");
    assert_eq!(first.client.others().count(), 0);
    let (_, on_server) = hosted.server.players().next().unwrap();
    let apart = (0..3)
        .map(|axis| (on_server.position[axis] - here[axis]).abs())
        .fold(0.0, f32::max);
    assert!(apart < 0.001, "server {on_server:?}, player {here:?}");
    places
}

#[test]
fn a_game_in_one_program_goes_the_same_way_every_time() {
    let first = a_game();
    for _ in 0..3 {
        assert!(first == a_game(), "two runs of the same game differed");
    }
}

#[test]
fn a_player_is_in_after_three_ticks_and_no_socket_is_opened() {
    let mut hosted = Hosted::new(HostSettings::default());
    assert!(hosted.host.local_addr().is_err());
    let mut player = hosted.join();
    let mut joined_on = None;
    for tick in 1..=10 {
        player.tick(&PlayerInput::default());
        hosted.tick();
        if joined_on.is_none() && player.link.id().is_some() {
            joined_on = Some(tick);
        }
    }
    assert_eq!(joined_on, Some(3));
    // The address the server knows the player by is on no network.
    let address = hosted.host.address(0).unwrap();
    assert!(address.ip().is_unspecified(), "{address}");
    assert_eq!(player.link.local_addr().unwrap(), address);
}

#[test]
fn silence_is_counted_in_ticks_and_not_by_the_clock() {
    let mut hosted = Hosted::new(HostSettings {
        timeout: Duration::from_millis(200),
    });
    let mut player = hosted.join();
    for _ in 0..5 {
        player.tick(&PlayerInput::default());
        hosted.tick();
    }
    assert_eq!(hosted.host.players().count(), 1);

    // Held still for longer than the time allowed, as a game is behind a
    // menu or under a debugger: no tick went by, so nobody went silent.
    std::thread::sleep(Duration::from_millis(300));
    player.tick(&PlayerInput::default());
    hosted.tick();
    assert_eq!(hosted.left, []);
    assert_eq!(player.closed, None);

    // The player's side stops being run. A fifth of a second is 10 ticks
    // of 20 ms: the server gives up on the tick after.
    let mut gave_up_after = None;
    for tick in 1..=20 {
        hosted.tick();
        if gave_up_after.is_none() && !hosted.left.is_empty() {
            gave_up_after = Some(tick);
        }
    }
    assert_eq!(hosted.left, [(0, Gone::TimedOut)]);
    assert_eq!(gave_up_after, Some(11));
}

#[test]
fn a_server_that_stops_or_is_full_says_so_at_once() {
    let mut hosted = Hosted::new(HostSettings::default());
    hosted.places = 1;
    let (mut player, mut late) = (hosted.join(), hosted.join());
    for _ in 0..5 {
        player.tick(&PlayerInput::default());
        hosted.tick();
    }
    for _ in 0..5 {
        late.tick(&PlayerInput::default());
        hosted.tick();
    }
    assert_eq!(late.closed, Some(Closed::Full));
    assert_eq!(player.closed, None);

    drop(hosted);
    player.tick(&PlayerInput::default());
    assert_eq!(player.closed, Some(Closed::ServerStopped));
}
