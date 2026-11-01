//! Fixed storage framing. No allocation or deserialization of peer-controlled lengths.
use crate::{Error, MAX_SNAPSHOT_BYTES};
use std::{
    io::{Read, Write},
    os::unix::net::UnixStream,
};
const HEADER: usize = 40;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub(crate) enum Kind {
    Start = 1,
    Hello = 2,
    Snapshot = 3,
    Invalidate = 4,
    Ready = 5,
    Action = 6,
}
#[derive(Clone)]
pub(crate) struct Packet {
    pub kind: Kind,
    pub generation: u64,
    pub epoch: u64,
    pub sequence: u64,
    pub len: usize,
    pub data: [u8; MAX_SNAPSHOT_BYTES],
}
impl Packet {
    pub fn empty(kind: Kind, generation: u64, epoch: u64, sequence: u64) -> Self {
        Self {
            kind,
            generation,
            epoch,
            sequence,
            len: 0,
            data: [0; MAX_SNAPSHOT_BYTES],
        }
    }
    pub fn set(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let target = self.data.get_mut(..bytes.len()).ok_or(Error::TooLarge)?;
        target.copy_from_slice(bytes);
        self.len = bytes.len();
        Ok(())
    }
}
fn put(bytes: &mut [u8], offset: usize, value: &[u8]) -> Result<(), Error> {
    let end = offset.checked_add(value.len()).ok_or(Error::Protocol)?;
    bytes
        .get_mut(offset..end)
        .ok_or(Error::Protocol)?
        .copy_from_slice(value);
    Ok(())
}
fn word(bytes: &[u8], offset: usize) -> Result<u64, Error> {
    let end = offset.checked_add(8).ok_or(Error::Protocol)?;
    Ok(u64::from_le_bytes(
        bytes
            .get(offset..end)
            .ok_or(Error::Protocol)?
            .try_into()
            .map_err(|_| Error::Protocol)?,
    ))
}
fn header(packet: &Packet) -> Result<[u8; HEADER], Error> {
    if packet.len > MAX_SNAPSHOT_BYTES {
        return Err(Error::TooLarge);
    }
    let mut out = [0; HEADER];
    put(&mut out, 0, b"FRP1")?;
    put(&mut out, 4, &[1, packet.kind as u8, 0, 0])?;
    put(&mut out, 8, &packet.generation.to_le_bytes())?;
    put(&mut out, 16, &packet.epoch.to_le_bytes())?;
    put(&mut out, 24, &packet.sequence.to_le_bytes())?;
    put(
        &mut out,
        32,
        &u32::try_from(packet.len)
            .map_err(|_| Error::TooLarge)?
            .to_le_bytes(),
    )?;
    Ok(out)
}
fn parse(bytes: &[u8; HEADER]) -> Result<Packet, Error> {
    if bytes.get(..4) != Some(b"FRP1")
        || bytes.get(4) != Some(&1)
        || bytes.get(6..8) != Some(&[0, 0])
        || bytes.get(36..40) != Some(&[0, 0, 0, 0])
    {
        return Err(Error::Protocol);
    }
    let kind = match bytes.get(5) {
        Some(1) => Kind::Start,
        Some(2) => Kind::Hello,
        Some(3) => Kind::Snapshot,
        Some(4) => Kind::Invalidate,
        Some(5) => Kind::Ready,
        Some(6) => Kind::Action,
        _ => return Err(Error::Protocol),
    };
    let len = u32::from_le_bytes(
        bytes
            .get(32..36)
            .ok_or(Error::Protocol)?
            .try_into()
            .map_err(|_| Error::Protocol)?,
    ) as usize;
    if len > MAX_SNAPSHOT_BYTES {
        return Err(Error::TooLarge);
    }
    let mut out = Packet::empty(kind, word(bytes, 8)?, word(bytes, 16)?, word(bytes, 24)?);
    if out.generation == 0 || out.epoch == 0 {
        return Err(Error::Protocol);
    }
    out.len = len;
    Ok(out)
}
pub(crate) struct Decoder {
    header: [u8; HEADER],
    used: usize,
    packet: Option<Packet>,
    received: usize,
}
impl Decoder {
    pub fn new() -> Self {
        Self {
            header: [0; HEADER],
            used: 0,
            packet: None,
            received: 0,
        }
    }
    pub fn read(&mut self, socket: &mut UnixStream) -> Result<Option<Packet>, Error> {
        for _ in 0..128 {
            if self.used < HEADER {
                let target = self.header.get_mut(self.used..).ok_or(Error::Protocol)?;
                match socket.read(target) {
                    Ok(0) => return Err(Error::Disconnected),
                    Ok(n) => self.used += n,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(None),
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => return Err(Error::Io),
                }
                if self.used < HEADER {
                    continue;
                }
                self.packet = Some(parse(&self.header)?);
            }
            let packet = self.packet.as_mut().ok_or(Error::Protocol)?;
            if self.received < packet.len {
                let target = packet
                    .data
                    .get_mut(self.received..packet.len)
                    .ok_or(Error::Protocol)?;
                match socket.read(target) {
                    Ok(0) => return Err(Error::Disconnected),
                    Ok(n) => self.received += n,
                    Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(None),
                    Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                    Err(_) => return Err(Error::Io),
                }
                if self.received < packet.len {
                    continue;
                }
            }
            self.used = 0;
            self.received = 0;
            return Ok(self.packet.take());
        }
        Ok(None)
    }
}
pub(crate) struct Encoder {
    header: [u8; HEADER],
    packet: Packet,
    sent: usize,
}
impl Encoder {
    pub fn new(packet: Packet) -> Result<Self, Error> {
        Ok(Self {
            header: header(&packet)?,
            packet,
            sent: 0,
        })
    }
    pub fn write(&mut self, socket: &mut UnixStream) -> Result<bool, Error> {
        for _ in 0..128 {
            let data = if self.sent < HEADER {
                self.header.get(self.sent..).ok_or(Error::Protocol)?
            } else {
                self.packet
                    .data
                    .get(self.sent - HEADER..self.packet.len)
                    .ok_or(Error::Protocol)?
            };
            if data.is_empty() {
                return Ok(true);
            }
            match socket.write(data) {
                Ok(0) => return Err(Error::Disconnected),
                Ok(n) => self.sent += n,
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(false),
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => return Err(Error::Io),
            }
        }
        Ok(false)
    }
}
pub(crate) fn wait(
    socket: Option<&UnixStream>,
    wake: &UnixStream,
    writing: bool,
    timeout: std::time::Duration,
) -> Result<(), Error> {
    use rustix::event::{PollFd, PollFlags, Timespec};
    let timespec = Timespec {
        tv_sec: timeout.as_secs().try_into().map_err(|_| Error::Config)?,
        tv_nsec: timeout.subsec_nanos().into(),
    };
    let mut fds = [
        PollFd::new(wake, PollFlags::IN),
        PollFd::new(wake, PollFlags::empty()),
    ];
    if let Some(socket) = socket {
        if let Some(fd) = fds.get_mut(1) {
            *fd = PollFd::new(
                socket,
                PollFlags::IN
                    | if writing {
                        PollFlags::OUT
                    } else {
                        PollFlags::empty()
                    },
            );
        }
    }
    match rustix::event::poll(&mut fds, Some(&timespec)) {
        Ok(_) | Err(rustix::io::Errno::INTR) => Ok(()),
        Err(_) => Err(Error::Io),
    }
}
pub(crate) fn drain_wake(wake: &mut UnixStream) {
    let mut buf = [0; 64];
    for _ in 0..128 {
        if !matches!(wake.read(&mut buf), Ok(1..)) {
            break;
        }
    }
}

#[cfg(test)]
mod tests {
    #![allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
    use super::*;

    #[test]
    fn fragmented_frames_preserve_boundaries_and_empty_payloads() {
        for chunk_size in 1..=64 {
            let (mut tx, mut rx) = UnixStream::pair().unwrap();
            rx.set_nonblocking(true).unwrap();
            let mut first = Packet::empty(Kind::Snapshot, 7, 9, 11);
            first.set(&[0xa5; MAX_SNAPSHOT_BYTES]).unwrap();
            let second = Packet::empty(Kind::Ready, 7, 9, 11);
            let mut bytes = header(&first).unwrap().to_vec();
            bytes.extend_from_slice(&first.data);
            bytes.extend_from_slice(&header(&second).unwrap());
            let mut decoder = Decoder::new();
            let mut packets = Vec::new();
            for chunk in bytes.chunks(chunk_size) {
                tx.write_all(chunk).unwrap();
                while let Some(packet) = decoder.read(&mut rx).unwrap() {
                    packets.push(packet);
                }
            }
            assert_eq!(packets.len(), 2);
            assert_eq!(packets[0].len, MAX_SNAPSHOT_BYTES);
            assert_eq!(packets[0].data, [0xa5; MAX_SNAPSHOT_BYTES]);
            assert_eq!(packets[1].kind, Kind::Ready);
            assert_eq!(packets[1].len, 0);
        }
    }
    #[test]
    fn malformed_headers_and_peer_lengths_are_rejected_before_body_reads() {
        let good = header(&Packet::empty(Kind::Snapshot, 1, 1, 1)).unwrap();
        for offset in [0, 1, 2, 3, 4, 5, 6, 7, 36, 37, 38, 39] {
            let mut bad = good;
            bad[offset] = 0xff;
            assert!(parse(&bad).is_err(), "offset {offset}");
        }
        for offset in [8, 16] {
            let mut bad = good;
            bad[offset..offset + 8].fill(0);
            assert!(matches!(parse(&bad), Err(Error::Protocol)));
        }
        for length in [4097u32, u32::MAX] {
            let mut bad = good;
            bad[32..36].copy_from_slice(&length.to_le_bytes());
            let (mut tx, mut rx) = UnixStream::pair().unwrap();
            rx.set_nonblocking(true).unwrap();
            tx.write_all(&bad).unwrap();
            assert!(matches!(Decoder::new().read(&mut rx), Err(Error::TooLarge)));
        }
    }
    #[test]
    fn truncated_peer_close_never_yields_partial_application_data() {
        for cutoff in 0..45 {
            let mut packet = Packet::empty(Kind::Snapshot, 1, 1, 1);
            packet.set(b"12345").unwrap();
            let mut bytes = header(&packet).unwrap().to_vec();
            bytes.extend_from_slice(b"12345");
            let (mut tx, mut rx) = UnixStream::pair().unwrap();
            tx.write_all(&bytes[..cutoff]).unwrap();
            drop(tx);
            assert!(matches!(
                Decoder::new().read(&mut rx),
                Err(Error::Disconnected)
            ));
        }
    }
    #[test]
    fn max_model_roundtrip_then_idle_would_block() {
        let (mut tx, mut rx) = UnixStream::pair().unwrap();
        tx.set_nonblocking(true).unwrap();
        rx.set_nonblocking(true).unwrap();
        let mut data = Packet::empty(Kind::Snapshot, 1, 1, 1);
        data.set(&[42; MAX_SNAPSHOT_BYTES]).unwrap();
        let mut encoder = Encoder::new(data).unwrap();
        let mut decoder = Decoder::new();
        assert!(encoder.write(&mut tx).unwrap());
        let received = decoder.read(&mut rx).unwrap().unwrap();
        assert_eq!(received.data, [42; MAX_SNAPSHOT_BYTES]);
        assert!(decoder.read(&mut rx).unwrap().is_none());
    }
}
