//! Datagrams handed between a host and players in the same program.
//!
//! A game played alone still has a server and a player's side. They are in
//! one program, so nothing has to cross a network: what one side sends is
//! put where the other side reads, at once and in order. Nothing is lost
//! or late, no port is opened, and the same game always goes the same way.
//!
//! Each player's end has a number, and the host knows it by an address
//! that cannot be on a network: `0.0.0.0` with the number as its port.

use std::collections::VecDeque;
use std::io;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

/// The most bytes that wait unread in one direction. Like a socket's
/// buffer: what does not fit is lost, so a side that has stopped reading
/// cannot fill the memory.
const MOST_WAITING: usize = 1 << 20;

/// The bytes that go before each datagram: who sent it and how long it is.
const HEADER: usize = 4;

/// Datagrams waiting to be read, end to end in one buffer, so handing one
/// over sets no memory aside once the buffer has grown to its size.
#[derive(Debug, Default)]
struct Queue {
    bytes: VecDeque<u8>,
}

impl Queue {
    fn push(&mut self, from: u16, datagram: &[u8]) {
        let Ok(len) = u16::try_from(datagram.len()) else {
            return;
        };
        if self.bytes.len() + HEADER + datagram.len() > MOST_WAITING {
            return;
        }
        self.bytes.extend(from.to_le_bytes());
        self.bytes.extend(len.to_le_bytes());
        self.bytes.extend(datagram);
    }

    /// Takes the next datagram into `into` and says how long it is and who
    /// sent it. One that is too long for `into` is cut to fit, as a socket
    /// cuts it.
    fn pop(&mut self, into: &mut [u8]) -> Option<(usize, u16)> {
        if self.bytes.len() < HEADER {
            return None;
        }
        let mut header = [0; HEADER];
        for (byte, waiting) in header.iter_mut().zip(self.bytes.drain(..HEADER)) {
            *byte = waiting;
        }
        let [from_low, from_high, len_low, len_high] = header;
        let len = usize::from(u16::from_le_bytes([len_low, len_high])).min(self.bytes.len());
        let mut kept = 0;
        // Dropping what is left of the drain takes the cut-off end too.
        for (byte, waiting) in into.iter_mut().zip(self.bytes.drain(..len)) {
            *byte = waiting;
            kept += 1;
        }
        Some((kept, u16::from_le_bytes([from_low, from_high])))
    }
}

/// What both sides hold: the datagrams on their way.
#[derive(Debug, Default)]
struct Exchange {
    to_host: Queue,
    /// One for each player's end ever made, by its number less one. `None`
    /// once that end is gone.
    to_links: Vec<Option<Queue>>,
}

fn locked(exchange: &Mutex<Exchange>) -> MutexGuard<'_, Exchange> {
    // A panic elsewhere leaves the queues as they were, which is whole.
    exchange.lock().unwrap_or_else(PoisonError::into_inner)
}

fn nothing_waiting() -> io::Error {
    io::ErrorKind::WouldBlock.into()
}

/// The address a host knows the player's end `number` by.
fn address_of(number: u16) -> SocketAddr {
    SocketAddr::from((Ipv4Addr::UNSPECIFIED, number))
}

/// Whether `address` is one only an end in this program has. Nothing that
/// comes over a network is from `0.0.0.0`.
pub(crate) fn is_inside(address: SocketAddr) -> bool {
    address.ip().is_unspecified()
}

/// The host's end.
#[derive(Debug, Default)]
pub(crate) struct HostEnd {
    exchange: Arc<Mutex<Exchange>>,
}

impl HostEnd {
    /// A new end for a player in this program.
    pub(crate) fn link(&self) -> LinkEnd {
        let mut exchange = locked(&self.exchange);
        // A port is 16 bits and 0 is no port. An end past the last number
        // is one nobody hears, as if its server were not there.
        let number = u16::try_from(exchange.to_links.len() + 1).unwrap_or(0);
        if number != 0 {
            exchange.to_links.push(Some(Queue::default()));
        }
        LinkEnd {
            exchange: Arc::clone(&self.exchange),
            number,
        }
    }

    /// As `UdpSocket::recv_from`, never waiting.
    pub(crate) fn recv_from(&self, into: &mut [u8]) -> io::Result<(usize, SocketAddr)> {
        let (len, from) = locked(&self.exchange)
            .to_host
            .pop(into)
            .ok_or_else(nothing_waiting)?;
        Ok((len, address_of(from)))
    }

    /// Hands `datagram` to the end known by `to`, if it is still there.
    pub(crate) fn send_to(&self, datagram: &[u8], to: SocketAddr) {
        let slot = usize::from(to.port()).checked_sub(1);
        let mut exchange = locked(&self.exchange);
        if let Some(Some(queue)) = slot.and_then(|slot| exchange.to_links.get_mut(slot)) {
            queue.push(0, datagram);
        }
    }
}

/// A player's end.
#[derive(Debug)]
pub(crate) struct LinkEnd {
    exchange: Arc<Mutex<Exchange>>,
    number: u16,
}

impl LinkEnd {
    fn slot(&self) -> Option<usize> {
        usize::from(self.number).checked_sub(1)
    }

    /// The address the host knows this end by.
    pub(crate) fn address(&self) -> SocketAddr {
        address_of(self.number)
    }

    /// As `UdpSocket::recv` on a connected socket, never waiting.
    pub(crate) fn recv(&self, into: &mut [u8]) -> io::Result<usize> {
        let mut exchange = locked(&self.exchange);
        self.slot()
            .and_then(|slot| exchange.to_links.get_mut(slot)?.as_mut()?.pop(into))
            .map(|(len, _)| len)
            .ok_or_else(nothing_waiting)
    }

    /// Hands `datagram` to the host.
    pub(crate) fn send(&self, datagram: &[u8]) {
        if self.number != 0 {
            locked(&self.exchange).to_host.push(self.number, datagram);
        }
    }
}

impl Drop for LinkEnd {
    fn drop(&mut self) {
        let mut exchange = locked(&self.exchange);
        if let Some(queue) = self.slot().and_then(|slot| exchange.to_links.get_mut(slot)) {
            *queue = None;
        }
    }
}

/// How a host or a link tells the time, which it needs only to notice
/// that the other side has gone silent.
#[derive(Debug)]
pub(crate) enum Clock {
    /// The computer's clock, for sides a network is between.
    Wall(Instant),
    /// Counted in ticks, for sides in one program: each `poll` that ends
    /// with nothing more to read is one tick gone by. A game that is held
    /// still, or a slow computer, then never looks like silence, and the
    /// same game takes the same time every time it is played.
    Ticks {
        /// How long one tick is.
        tick: Duration,
        /// How many have gone by.
        gone: u32,
    },
}

impl Clock {
    /// How long it has been since the clock was made.
    pub(crate) fn now(&self) -> Duration {
        match self {
            Self::Wall(started) => started.elapsed(),
            Self::Ticks { tick, gone } => tick.saturating_mul(*gone),
        }
    }

    /// A `poll` has read all there was.
    pub(crate) fn polled(&mut self) {
        if let Self::Ticks { gone, .. } = self {
            *gone = gone.saturating_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn datagrams_come_out_whole_in_the_order_they_went_in() {
        let host = HostEnd::default();
        let (first, second) = (host.link(), host.link());
        let mut inbox = [0; 8];

        assert!(host.recv_from(&mut inbox).is_err());
        first.send(b"one");
        second.send(b"");
        first.send(b"three");
        assert_eq!(host.recv_from(&mut inbox).unwrap(), (3, first.address()));
        assert_eq!(&inbox[..3], b"one");
        assert_eq!(host.recv_from(&mut inbox).unwrap(), (0, second.address()));
        assert_eq!(host.recv_from(&mut inbox).unwrap(), (5, first.address()));
        assert_eq!(&inbox[..5], b"three");
        assert_eq!(
            host.recv_from(&mut inbox).unwrap_err().kind(),
            io::ErrorKind::WouldBlock
        );

        // Each end hears only what was sent to it.
        host.send_to(b"to the second", second.address());
        assert!(first.recv(&mut inbox).is_err());
        // Too long for the room: cut, and the rest does not spill into
        // the next datagram.
        host.send_to(b"next", second.address());
        assert_eq!(second.recv(&mut inbox).unwrap(), 8);
        assert_eq!(&inbox, b"to the s");
        assert_eq!(second.recv(&mut inbox).unwrap(), 4);
        assert_eq!(&inbox[..4], b"next");
    }

    #[test]
    fn an_end_that_is_gone_or_not_reading_takes_no_memory() {
        let host = HostEnd::default();
        let gone = host.link();
        let address = gone.address();
        drop(gone);
        host.send_to(b"anyone?", address);
        assert!(locked(&host.exchange).to_links.iter().all(Option::is_none));
        // An address no end has, and one from a network.
        host.send_to(b"anyone?", address_of(0));
        host.send_to(b"anyone?", address_of(7));
        assert!(!is_inside(SocketAddr::from(([127, 0, 0, 1], 1))));
        assert!(is_inside(address));

        let deaf = host.link();
        let datagram = [0; 1200];
        for _ in 0..10_000 {
            host.send_to(&datagram, deaf.address());
            deaf.send(&datagram);
        }
        let exchange = locked(&host.exchange);
        assert!(exchange.to_host.bytes.len() <= MOST_WAITING);
        let waiting = exchange.to_links.iter().flatten().next().unwrap();
        assert!(waiting.bytes.len() <= MOST_WAITING);
    }

    #[test]
    fn a_clock_of_ticks_moves_only_when_polled() {
        let mut clock = Clock::Ticks {
            tick: Duration::from_millis(20),
            gone: 0,
        };
        assert_eq!(clock.now(), Duration::ZERO);
        clock.polled();
        clock.polled();
        assert_eq!(clock.now(), Duration::from_millis(40));
    }
}
