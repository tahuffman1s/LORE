//! The netcode on real UDP sockets.
//!
//! A server holds one [`UdpHost`], a player one [`UdpLink`]. Neither ever
//! waits: [`UdpHost::poll`] and [`UdpLink::poll`] hand over what has
//! arrived and come straight back, so both fit in a game's tick.
//!
//! A player in the same program as the server needs no socket: see
//! [`UdpHost::link_in_program`].

use std::fmt;
use std::io;
use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, ToSocketAddrs, UdpSocket};
use std::time::{Duration, Instant};

use crate::connect::{
    Closed, DATA_HEADER, Gone, Heard, Host, HostSettings, JoinRequest, Link, LinkHeard,
    LinkSettings, MAX_DATAGRAM_BYTES, Refusal,
};
use crate::inside::{Clock, HostEnd, LinkEnd, is_inside};
use crate::snapshot::PlayerId;

/// How many times a goodbye is sent, since any one copy may be lost.
const GOODBYES: usize = 3;

/// How many socket errors one `poll` shrugs off before coming back.
const MOST_ERRORS: usize = 16;

/// How many datagrams are read before `poll` says "nothing more" whatever
/// is still waiting, so a flood of rubbish cannot hold up the tick. The
/// rest is read on the next tick. A full server's players send 64 a tick.
const MOST_DATAGRAMS: usize = 1024;

/// Room for one datagram and one byte more, so one that is too long is
/// seen to be too long instead of being cut to fit.
const INBOX_BYTES: usize = MAX_DATAGRAM_BYTES + 1;

/// What a socket was being opened for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Purpose {
    Hosting,
    Joining,
}

/// Why a socket could not be opened.
#[derive(Debug)]
pub struct SocketError {
    purpose: Purpose,
    address: String,
    source: io::Error,
}

impl fmt::Display for SocketError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let Self {
            purpose,
            address,
            source,
        } = self;
        if *purpose == Purpose::Hosting {
            write!(
                f,
                "could not open `{address}` for players to join: {source}. Another server may \
                 already be using that port; stop it or choose another port."
            )
        } else {
            write!(
                f,
                "could not find a server at `{address}`: {source}. Write the address with its \
                 port, like `203.0.113.5:27015` or `example.com:27015`."
            )
        }
    }
}

impl std::error::Error for SocketError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}

/// Reads the next datagram, or `None` when nothing more is waiting.
///
/// Errors other than "nothing waiting" are stepped over: Windows reports
/// that an earlier datagram found nobody listening as an error on the
/// next read, and that must not stop the others being read.
fn next_datagram<T>(budget: &mut Budget, mut read: impl FnMut() -> io::Result<T>) -> Option<T> {
    while budget.datagrams < MOST_DATAGRAMS && budget.errors <= MOST_ERRORS {
        match read() {
            Ok(read) => {
                budget.datagrams += 1;
                return Some(read);
            }
            Err(error) if error.kind() == io::ErrorKind::WouldBlock => return None,
            Err(_) => budget.errors += 1,
        }
    }
    None
}

/// What has been read since `poll` last said "nothing more".
#[derive(Debug, Default)]
struct Budget {
    datagrams: usize,
    errors: usize,
}

/// Reads a host's next datagram: from the players in its own program
/// first, then from the network.
fn host_read(
    inside: &HostEnd,
    socket: Option<&UdpSocket>,
    inbox: &mut [u8],
) -> io::Result<(usize, SocketAddr)> {
    match inside.recv_from(inbox) {
        Err(error) if error.kind() == io::ErrorKind::WouldBlock => {}
        heard => return heard,
    }
    let Some(socket) = socket else {
        return Err(io::ErrorKind::WouldBlock.into());
    };
    let (len, from) = socket.recv_from(inbox)?;
    if is_inside(from) {
        // No network delivers this; it must not pass for a player here.
        return Err(io::Error::other("a datagram from no address"));
    }
    Ok((len, from))
}

/// What a link's datagrams travel over.
#[derive(Debug)]
enum LinkWire {
    /// A socket that hears only the server.
    Udp(UdpSocket),
    /// The server is in this program.
    Inside(LinkEnd),
}

impl LinkWire {
    fn recv(&self, inbox: &mut [u8]) -> io::Result<usize> {
        match self {
            Self::Udp(socket) => socket.recv(inbox),
            Self::Inside(end) => end.recv(inbox),
        }
    }

    /// A datagram that cannot be sent is one more lost one.
    fn send(&self, datagram: &[u8]) {
        match self {
            Self::Udp(socket) => drop(socket.send(datagram)),
            Self::Inside(end) => end.send(datagram),
        }
    }
}

/// Something a server's game should act on, from [`UdpHost::poll`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HostEvent<'a> {
    /// Somebody wants to join. Answer before the next `poll` with
    /// [`UdpHost::accept`] or [`UdpHost::refuse`]. One address and port
    /// holds one place at a time; how many places one machine may take
    /// (see [`JoinRequest::address`]) is for the game to decide.
    Join(JoinRequest),
    /// A packet of commands from a player, for
    /// [`Server::receive`](crate::Server::receive).
    Packet(PlayerId, &'a [u8]),
    /// A player is gone. Call [`Server::leave`](crate::Server::leave).
    Left(PlayerId, Gone),
}

/// A server's UDP socket and the players joined through it.
///
/// ```no_run
/// use lore_net::{HostEvent, HostSettings, Refusal, Server, ServerSettings, UdpHost};
/// use lore_physics::{Level, MoveSettings, Player};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let level = Level::from_triangles(&[]);
/// let rules = MoveSettings::default();
/// let mut server = Server::new(1.0 / 60.0, ServerSettings::default());
/// let mut host = UdpHost::bind("0.0.0.0:27015", HostSettings::default())?;
/// let mut packet = Vec::new();
///
/// loop {
///     // Once per tick:
///     while let Some(event) = host.poll() {
///         match event {
///             HostEvent::Join(request) => {
///                 match server.join(Player::at([0.0, 1.0, 0.0], &rules)) {
///                     Ok(id) => host.accept(request, id),
///                     Err(_) => host.refuse(request, Refusal::Full),
///                 }
///             }
///             // A packet that cannot be read is thrown away.
///             HostEvent::Packet(id, commands) => drop(server.receive(id, commands)),
///             HostEvent::Left(id, _) => server.leave(id),
///         }
///     }
///     server.tick(&level, &rules);
///     for id in host.players() {
///         server.write_snapshot(id, &mut packet)?;
///         host.send(id, &packet);
///     }
/// #   break;
/// }
/// # Ok(())
/// # }
/// ```
///
/// Dropping the host tells every player the server has stopped.
#[derive(Debug)]
pub struct UdpHost {
    /// `None` for a server only players in this program join.
    socket: Option<UdpSocket>,
    /// Where players in this program are heard and answered.
    inside: HostEnd,
    host: Host,
    clock: Clock,
    inbox: Box<[u8]>,
    outbox: Vec<u8>,
    budget: Budget,
}

impl UdpHost {
    /// Opens a socket for players to join through. `address` is an
    /// address of this machine and a port: `0.0.0.0:27015` takes players
    /// from anywhere, `127.0.0.1:27015` only from this machine, and port
    /// `0` lets the system choose one ([`local_addr`](Self::local_addr)
    /// says which).
    ///
    /// # Errors
    ///
    /// When the address cannot be read or the port cannot be opened,
    /// most often because another program has it.
    pub fn bind(address: &str, settings: HostSettings) -> Result<Self, SocketError> {
        let open = || {
            let socket = UdpSocket::bind(address)?;
            socket.set_nonblocking(true)?;
            Ok(socket)
        };
        let socket = open().map_err(|source| SocketError {
            purpose: Purpose::Hosting,
            address: address.to_owned(),
            source,
        })?;
        Ok(Self::new(
            Some(socket),
            settings,
            Clock::Wall(Instant::now()),
        ))
    }

    /// A server with no socket, for a game that only players in this
    /// program join ([`link_in_program`](Self::link_in_program)): one
    /// played alone, or tested. Nothing on any network can find or reach
    /// it, and no port is opened.
    ///
    /// It reads no clock. `tick` is how long one of the game's ticks is,
    /// and time goes by one tick for each [`poll`](Self::poll) that comes
    /// back with `None`, so a player counts as silent after so many
    /// ticks, however long the computer took over them.
    #[must_use]
    pub fn in_program(settings: HostSettings, tick: Duration) -> Self {
        Self::new(None, settings, Clock::Ticks { tick, gone: 0 })
    }

    fn new(socket: Option<UdpSocket>, settings: HostSettings, clock: Clock) -> Self {
        Self {
            socket,
            inside: HostEnd::default(),
            host: Host::new(settings),
            clock,
            inbox: vec![0; INBOX_BYTES].into(),
            outbox: Vec::with_capacity(MAX_DATAGRAM_BYTES),
            budget: Budget::default(),
        }
    }

    /// Starts joining this server from the same program, with no socket:
    /// what the link sends is here at the server's next
    /// [`poll`](Self::poll), and what the server sends is there at the
    /// link's next, whole and in order. Joining, leaving and everything
    /// else go as over a network. Any server can be joined this way,
    /// whether or not it also has a socket.
    ///
    /// The link reads no clock: `tick` is how long one of the game's ticks
    /// is, and time goes by one tick for each [`UdpLink::poll`] that
    /// comes back with `None`.
    #[must_use]
    pub fn link_in_program(&self, settings: LinkSettings, tick: Duration) -> UdpLink {
        UdpLink::new(
            LinkWire::Inside(self.inside.link()),
            settings,
            Clock::Ticks { tick, gone: 0 },
        )
    }

    /// The address and port the socket is open on.
    ///
    /// # Errors
    ///
    /// When there is no socket ([`in_program`](Self::in_program)), or the
    /// system cannot say, which it always can for an open socket.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        match &self.socket {
            Some(socket) => socket.local_addr(),
            None => Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "this server has no address: it is joined only from inside its own program",
            )),
        }
    }

    /// Sends what is in the outbox to `to`, in this program or over the
    /// network. A datagram that cannot be sent is one more lost one.
    fn send_to(&self, to: SocketAddr) {
        if is_inside(to) {
            self.inside.send_to(&self.outbox, to);
        } else if let Some(socket) = &self.socket {
            let _ = socket.send_to(&self.outbox, to);
        }
    }

    /// The next thing the game should act on, or `None` when everything
    /// that has arrived is dealt with. Never waits. Call it in a loop
    /// once per tick.
    pub fn poll(&mut self) -> Option<HostEvent<'_>> {
        let now = self.clock.now();
        while let Some((len, from)) = next_datagram(&mut self.budget, || {
            host_read(&self.inside, self.socket.as_ref(), &mut self.inbox)
        }) {
            let datagram = self.inbox.get(..len).unwrap_or_default();
            let heard = self.host.receive(now, from, datagram, &mut self.outbox);
            if !self.outbox.is_empty() {
                self.send_to(from);
            }
            match heard {
                Some(Heard::Join(request)) => return Some(HostEvent::Join(request)),
                Some(Heard::Packet(id)) => {
                    let packet = self.inbox.get(DATA_HEADER..len).unwrap_or_default();
                    return Some(HostEvent::Packet(id, packet));
                }
                Some(Heard::Left(id)) => return Some(HostEvent::Left(id, Gone::Quit)),
                None => {}
            }
        }
        if let Some(id) = self.host.timed_out(now) {
            return Some(HostEvent::Left(id, Gone::TimedOut));
        }
        self.budget = Budget::default();
        self.clock.polled();
        None
    }

    /// Gives the player behind `request` the place `id`, the number
    /// [`Server::join`](crate::Server::join) gave, and tells them.
    pub fn accept(&mut self, request: JoinRequest, id: PlayerId) {
        self.host
            .accept(self.clock.now(), request, id, &mut self.outbox);
        if !self.outbox.is_empty() {
            self.send_to(request.address());
        }
    }

    /// Turns the player behind `request` away and tells them why.
    pub fn refuse(&mut self, request: JoinRequest, why: Refusal) {
        Host::refuse(request, why, &mut self.outbox);
        self.send_to(request.address());
    }

    /// Sends a snapshot to the player `id`. Does nothing if nobody has
    /// that place.
    pub fn send(&mut self, id: PlayerId, packet: &[u8]) {
        if let Some(to) = self.host.wrap(id, packet, &mut self.outbox) {
            self.send_to(to);
        }
    }

    /// Takes the player `id` off and tells them why. The game still has
    /// to call [`Server::leave`](crate::Server::leave); no
    /// [`HostEvent::Left`] follows.
    pub fn kick(&mut self, id: PlayerId, why: Refusal) {
        if let Some(to) = self.host.drop_player(id, why, &mut self.outbox) {
            for _ in 0..GOODBYES {
                self.send_to(to);
            }
        }
    }

    /// The numbers of the players joined, lowest first.
    pub fn players(&self) -> impl Iterator<Item = PlayerId> + use<> {
        let mut joined = 0_u64;
        for id in self.host.players() {
            joined |= 1 << id;
        }
        (0..64).filter(move |id| joined & (1 << id) != 0)
    }

    /// The address the player `id` joined from.
    #[must_use]
    pub fn address(&self, id: PlayerId) -> Option<SocketAddr> {
        self.host.address(id)
    }
}

impl Drop for UdpHost {
    fn drop(&mut self) {
        for id in self.players() {
            self.kick(id, Refusal::Stopping);
        }
    }
}

/// Something a player's game should act on, from [`UdpLink::poll`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LinkEvent<'a> {
    /// The server gave this player a place, with this number.
    Joined(PlayerId),
    /// A snapshot from the server, for
    /// [`Client::receive`](crate::Client::receive).
    Packet(&'a [u8]),
    /// The game is over for this player; show them why. Nothing more
    /// comes from this link.
    Closed(Closed),
}

/// A player's UDP socket and their stay on one server.
///
/// ```no_run
/// use lore_net::{Client, ClientSettings, LinkEvent, LinkSettings, PlayerInput, UdpLink};
/// use lore_physics::{Level, MoveSettings, Player};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let level = Level::from_triangles(&[]);
/// let rules = MoveSettings::default();
/// let spawn = Player::at([0.0, 1.0, 0.0], &rules);
/// let mut client = Client::new(spawn, 1.0 / 60.0, ClientSettings::default());
/// let mut link = UdpLink::connect("127.0.0.1:27015", LinkSettings::default())?;
/// let mut packet = Vec::new();
///
/// loop {
///     // Once per tick:
///     while let Some(event) = link.poll() {
///         match event {
///             LinkEvent::Joined(_) => println!("joined"),
///             // A packet that cannot be read is thrown away.
///             LinkEvent::Packet(snapshot) => drop(client.receive(&level, &rules, snapshot)),
///             LinkEvent::Closed(why) => {
///                 println!("{why}");
///                 return Ok(());
///             }
///         }
///     }
///     if link.id().is_some() {
///         client.predict(&level, &rules, &PlayerInput::default(), &mut packet);
///         link.send(&packet);
///     }
/// #   break;
/// }
/// # Ok(())
/// # }
/// ```
///
/// Dropping the link tells the server this player has left.
#[derive(Debug)]
pub struct UdpLink {
    wire: LinkWire,
    link: Link,
    clock: Clock,
    inbox: Box<[u8]>,
    outbox: Vec<u8>,
    budget: Budget,
}

impl UdpLink {
    /// Starts joining the server at `address`, an address or a name and
    /// a port: `203.0.113.5:27015` or `example.com:27015`. A name with
    /// both kinds of address is joined over IPv4. Looking a name
    /// up may take a moment; nothing else here waits. Whether the server
    /// answered is told later, by [`poll`](Self::poll).
    ///
    /// # Errors
    ///
    /// When the address cannot be read or the name is not known.
    pub fn connect(address: &str, settings: LinkSettings) -> Result<Self, SocketError> {
        let open = || {
            // A name such as `localhost` often has an IPv6 address listed
            // first, and a server opened on `0.0.0.0` does not hear it.
            let mut found = address.to_socket_addrs()?.peekable();
            let first = found.peek().copied();
            let server = found.find(SocketAddr::is_ipv4).or(first).ok_or_else(|| {
                io::Error::new(io::ErrorKind::NotFound, "the name has no address")
            })?;
            let socket = if server.is_ipv4() {
                UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0))
            } else {
                UdpSocket::bind((Ipv6Addr::UNSPECIFIED, 0))
            }?;
            // Only datagrams from the server are handed over after this.
            socket.connect(server)?;
            socket.set_nonblocking(true)?;
            Ok(socket)
        };
        let socket = open().map_err(|source| SocketError {
            purpose: Purpose::Joining,
            address: address.to_owned(),
            source,
        })?;
        Ok(Self::new(
            LinkWire::Udp(socket),
            settings,
            Clock::Wall(Instant::now()),
        ))
    }

    fn new(wire: LinkWire, settings: LinkSettings, clock: Clock) -> Self {
        Self {
            wire,
            link: Link::new(clock.now(), settings),
            clock,
            inbox: vec![0; INBOX_BYTES].into(),
            outbox: Vec::with_capacity(MAX_DATAGRAM_BYTES),
            budget: Budget::default(),
        }
    }

    /// The next thing the game should act on, or `None` when everything
    /// that has arrived is dealt with. Never waits. Call it in a loop
    /// once per tick, joined or not: it is also what keeps asking the
    /// server for a place.
    pub fn poll(&mut self) -> Option<LinkEvent<'_>> {
        let now = self.clock.now();
        while let Some(len) = next_datagram(&mut self.budget, || self.wire.recv(&mut self.inbox)) {
            let datagram = self.inbox.get(..len).unwrap_or_default();
            match self.link.receive(now, datagram) {
                Some(LinkHeard::Joined(id)) => return Some(LinkEvent::Joined(id)),
                Some(LinkHeard::Packet) => {
                    let packet = self.inbox.get(DATA_HEADER..len).unwrap_or_default();
                    return Some(LinkEvent::Packet(packet));
                }
                Some(LinkHeard::Closed(why)) => return Some(LinkEvent::Closed(why)),
                None => {}
            }
        }
        let closed = self.link.update(now, &mut self.outbox);
        if !self.outbox.is_empty() {
            self.wire.send(&self.outbox);
        }
        if closed.is_none() {
            self.budget = Budget::default();
            self.clock.polled();
        }
        closed.map(LinkEvent::Closed)
    }

    /// Sends a packet of commands to the server. Does nothing until
    /// joined. Send one every tick: a server that hears nothing for a
    /// while takes the player to be gone.
    pub fn send(&mut self, packet: &[u8]) {
        if self.link.wrap(packet, &mut self.outbox) {
            self.wire.send(&self.outbox);
        }
    }

    /// Leaves the server and tells it so. Dropping the link does the
    /// same.
    pub fn leave(&mut self) {
        if self.link.leave(&mut self.outbox) {
            for _ in 0..GOODBYES {
                self.wire.send(&self.outbox);
            }
        }
    }

    /// This player's number on the server, once joined and until the
    /// stay ends.
    #[must_use]
    pub fn id(&self) -> Option<PlayerId> {
        self.link.id()
    }

    /// Why the stay ended, once it has.
    #[must_use]
    pub fn closed(&self) -> Option<Closed> {
        self.link.closed()
    }

    /// The address and port this side's socket is open on. A link to a
    /// server in the same program ([`UdpHost::link_in_program`]) has no
    /// socket and gives the address that server knows it by, `0.0.0.0`
    /// and a number.
    ///
    /// # Errors
    ///
    /// When the system cannot say, which it always can for an open
    /// socket.
    pub fn local_addr(&self) -> io::Result<SocketAddr> {
        match &self.wire {
            LinkWire::Udp(socket) => socket.local_addr(),
            LinkWire::Inside(end) => Ok(end.address()),
        }
    }
}

impl Drop for UdpLink {
    fn drop(&mut self) {
        self.leave();
    }
}
