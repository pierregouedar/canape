//! Motion server: publishes the gyroscope on a local UDP port, in the
//! binary protocol that emulators read for motion controls. It listens on
//! the loopback address only, answers only the three messages below and
//! never accepts a command. The sensor is read only while a client is
//! subscribed.
//!
//! The wire format ([`Packet`], [`Clients`]) is pure and tested on any OS;
//! the thread ([`spawn`], [`follow`]) reads the sensor through
//! `canape_hw::motion`.
//!
//! Layout of every packet, little-endian: a 16-byte header (4 magic bytes,
//! u16 protocol version, u16 length of what follows the header, u32 CRC-32
//! of the whole packet computed with this field zeroed, u32 sender id),
//! then a u32 message type and its payload.

use std::net::{SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

use canape_hw::motion::{AngularVelocity, Gyro};

const VERSION: u16 = 1001;
const SERVER_MAGIC: &[u8; 4] = b"DSUS";
const CLIENT_MAGIC: &[u8; 4] = b"DSUC";
const HEADER: usize = 16;

const MSG_VERSION: u32 = 0x10_0000;
const MSG_PORTS: u32 = 0x10_0001;
const MSG_DATA: u32 = 0x10_0002;

/// The one pad slot published.
const SLOT: u8 = 0;
/// Slots a ports request may ask about.
const SLOTS: usize = 4;
/// Identifies the pad to clients that select it by address.
const PAD_ADDRESS: [u8; 6] = *b"CANAPE";
/// A client that has not asked again for this long is dropped.
const CLIENT_TIMEOUT: Duration = Duration::from_secs(5);
/// Clients served at once.
const MAX_CLIENTS: usize = 4;

/// CRC-32 (the common reflected polynomial 0xEDB88320).
fn crc32(data: &[u8]) -> u32 {
    const TABLE: [u32; 256] = {
        let mut table = [0; 256];
        let mut i = 0;
        while i < 256 {
            let mut crc = i as u32;
            let mut bit = 0;
            while bit < 8 {
                crc = if crc & 1 != 0 {
                    (crc >> 1) ^ 0xEDB8_8320
                } else {
                    crc >> 1
                };
                bit += 1;
            }
            table[i] = crc;
            i += 1;
        }
        table
    };
    !data.iter().fold(!0u32, |crc, &byte| {
        TABLE[((crc ^ u32::from(byte)) & 0xFF) as usize] ^ (crc >> 8)
    })
}

/// Builds a server packet: header, message type, payload.
fn packet(server_id: u32, message: u32, payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(HEADER + 4 + payload.len());
    out.extend_from_slice(SERVER_MAGIC);
    out.extend_from_slice(&VERSION.to_le_bytes());
    out.extend_from_slice(&((4 + payload.len()) as u16).to_le_bytes());
    out.extend_from_slice(&[0; 4]);
    out.extend_from_slice(&server_id.to_le_bytes());
    out.extend_from_slice(&message.to_le_bytes());
    out.extend_from_slice(payload);
    let crc = crc32(&out);
    out[8..12].copy_from_slice(&crc.to_le_bytes());
    out
}

/// A well-formed client packet: its message type and payload.
fn parse(datagram: &[u8]) -> Option<(u32, &[u8])> {
    if datagram.len() < HEADER + 4 || &datagram[..4] != CLIENT_MAGIC {
        return None;
    }
    let version = u16::from_le_bytes(datagram[4..6].try_into().ok()?);
    let length = usize::from(u16::from_le_bytes(datagram[6..8].try_into().ok()?));
    if version > VERSION || length < 4 || datagram.len() < HEADER + length {
        return None;
    }
    let datagram = &datagram[..HEADER + length];
    let sent = u32::from_le_bytes(datagram[8..12].try_into().ok()?);
    let mut zeroed = datagram.to_vec();
    zeroed[8..12].fill(0);
    if crc32(&zeroed) != sent {
        return None;
    }
    let message = u32::from_le_bytes(datagram[HEADER..HEADER + 4].try_into().ok()?);
    Some((message, &datagram[HEADER + 4..]))
}

/// The 11 bytes describing a pad, at the start of a ports reply and of
/// every data packet.
fn pad_info(slot: u8, connected: bool) -> [u8; 11] {
    let mut info = [0; 11];
    info[0] = slot;
    if connected {
        info[1] = 2; // connected
        info[2] = 2; // full gyroscope
        info[3] = 1; // wired
        info[4..10].copy_from_slice(&PAD_ADDRESS);
    }
    info
}

/// Who is subscribed to the readings, and the replies to their requests.
pub struct Clients {
    server_id: u32,
    list: Vec<(SocketAddr, Instant)>,
    counter: u32,
}

impl Clients {
    pub fn new(server_id: u32) -> Self {
        Self {
            server_id,
            list: Vec::new(),
            counter: 0,
        }
    }

    /// Replies to one datagram; a data request also subscribes its sender.
    /// Anything malformed gets no reply.
    pub fn handle(&mut self, datagram: &[u8], from: SocketAddr, now: Instant) -> Vec<Vec<u8>> {
        let Some((message, payload)) = parse(datagram) else {
            return Vec::new();
        };
        match message {
            MSG_VERSION => vec![packet(self.server_id, MSG_VERSION, &VERSION.to_le_bytes())],
            MSG_PORTS => {
                let Some(count) = payload
                    .get(..4)
                    .and_then(|b| b.try_into().ok())
                    .map(i32::from_le_bytes)
                else {
                    return Vec::new();
                };
                let count = usize::try_from(count).unwrap_or(0).min(SLOTS);
                payload
                    .get(4..)
                    .unwrap_or_default()
                    .iter()
                    .take(count)
                    .map(|&slot| {
                        let mut body = pad_info(slot, slot == SLOT).to_vec();
                        body.push(0);
                        packet(self.server_id, MSG_PORTS, &body)
                    })
                    .collect()
            }
            MSG_DATA => {
                // Which pad: every one, one slot, or one address.
                let wanted = match payload {
                    [0, ..] => true,
                    [flags, slot, rest @ ..] => {
                        (flags & 1 == 0 || *slot == SLOT)
                            && (flags & 2 == 0 || rest.get(..6) == Some(&PAD_ADDRESS))
                    }
                    _ => false,
                };
                if wanted {
                    self.subscribe(from, now);
                }
                Vec::new()
            }
            _ => Vec::new(),
        }
    }

    fn subscribe(&mut self, from: SocketAddr, now: Instant) {
        if let Some(client) = self.list.iter_mut().find(|(addr, _)| *addr == from) {
            client.1 = now;
        } else if self.list.len() < MAX_CLIENTS {
            self.list.push((from, now));
        }
    }

    /// Drops the clients that stopped asking; whether any is left.
    pub fn any(&mut self, now: Instant) -> bool {
        self.list
            .retain(|(_, seen)| now.saturating_duration_since(*seen) < CLIENT_TIMEOUT);
        !self.list.is_empty()
    }

    pub fn addresses(&self) -> impl Iterator<Item = SocketAddr> + '_ {
        self.list.iter().map(|(addr, _)| *addr)
    }

    /// The data packet for one reading, `since_start` after the server
    /// began. The accelerometer is not read: its three values are zero.
    pub fn data(&mut self, rate: AngularVelocity, since_start: Duration) -> Vec<u8> {
        self.counter = self.counter.wrapping_add(1);
        let mut body = Vec::with_capacity(80);
        body.extend_from_slice(&pad_info(SLOT, true));
        body.push(1); // active
        body.extend_from_slice(&self.counter.to_le_bytes());
        body.extend_from_slice(&[0; 4]); // buttons and home, touch
        body.extend_from_slice(&[128; 4]); // sticks centred
        body.extend_from_slice(&[0; 12]); // analog directions and buttons
        body.extend_from_slice(&[0; 12]); // two touch points
        body.extend_from_slice(&(since_start.as_micros() as u64).to_le_bytes());
        body.extend_from_slice(&[0; 12]); // acceleration
        for value in [rate.x, rate.y, rate.z] {
            body.extend_from_slice(&value.to_le_bytes());
        }
        packet(self.server_id, MSG_DATA, &body)
    }
}

/// Longest wait for a request; also the pace of the readings sent out.
const TICK: Duration = Duration::from_millis(5);
/// Wait before looking for a sensor again after a failure.
const SENSOR_RETRY: Duration = Duration::from_secs(5);

/// Answers requests on `socket` and streams the sensor to the subscribed
/// clients, until `keep` says to stop. The sensor is opened with
/// `open_sensor` when the first client subscribes and released after the
/// last one is gone.
fn serve(
    socket: &UdpSocket,
    keep: impl Fn() -> bool,
    open_sensor: impl Fn() -> canape_hw::Result<Box<dyn Gyro>>,
) -> std::io::Result<()> {
    socket.set_read_timeout(Some(TICK))?;
    let start = Instant::now();
    let mut clients = Clients::new(start_id());
    let mut sensor: Option<Box<dyn Gyro>> = None;
    let mut retry_at = start;
    let mut buffer = [0u8; 512];
    while keep() {
        match socket.recv_from(&mut buffer) {
            Ok((len, from)) => {
                for reply in clients.handle(&buffer[..len], from, Instant::now()) {
                    let _ = socket.send_to(&reply, from);
                }
            }
            Err(err)
                if matches!(
                    err.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            // Windows reports a client that went away as a failed read.
            Err(err) if err.kind() == std::io::ErrorKind::ConnectionReset => {}
            Err(err) => {
                release(&mut sensor);
                return Err(err);
            }
        }
        let now = Instant::now();
        if !clients.any(now) {
            release(&mut sensor);
            continue;
        }
        if sensor.is_none() && now >= retry_at {
            match open_sensor() {
                Ok(mut gyro) => {
                    crate::log(&gyro.describe());
                    gyro.set_active(true);
                    sensor = Some(gyro);
                }
                Err(err) => {
                    crate::log(&format!("motion server: {err}"));
                    retry_at = now + SENSOR_RETRY;
                }
            }
        }
        let Some(gyro) = sensor.as_mut() else {
            continue;
        };
        match gyro.read() {
            Ok(Some(rate)) => {
                let data = clients.data(rate, now - start);
                for client in clients.addresses() {
                    let _ = socket.send_to(&data, client);
                }
            }
            Ok(None) => {}
            Err(err) => {
                crate::log(&format!("motion server: {err}"));
                release(&mut sensor);
                retry_at = now + SENSOR_RETRY;
            }
        }
    }
    release(&mut sensor);
    Ok(())
}

/// Puts the sensor back to its default rate, which saves power.
fn release(sensor: &mut Option<Box<dyn Gyro>>) {
    if let Some(mut gyro) = sensor.take() {
        gyro.set_active(false);
    }
}

/// Tells this run of the server from the previous ones to the clients.
fn start_id() -> u32 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(1, |d| d.subsec_nanos() ^ d.as_secs() as u32)
}

#[cfg(windows)]
pub use engine::{follow, spawn};

#[cfg(windows)]
mod engine {
    use std::net::{Ipv4Addr, UdpSocket};
    use std::sync::{Condvar, Mutex};
    use std::thread;
    use std::time::Duration;

    use canape_core::GyroSettings;
    use canape_hw::motion;

    use super::serve;

    /// Wait before binding again after a failure.
    const RETRY: Duration = Duration::from_secs(30);

    /// The port to listen on, `None` while the server is off.
    static WANTED: Mutex<Option<u16>> = Mutex::new(None);
    static CHANGED: Condvar = Condvar::new();

    /// The settings in effect, from a status push.
    pub fn follow(settings: GyroSettings) {
        let wanted = settings.motion_server.then_some(settings.motion_port);
        let mut current = WANTED.lock().unwrap();
        if *current != wanted {
            *current = wanted;
            CHANGED.notify_all();
        }
    }

    fn wanted() -> Option<u16> {
        *WANTED.lock().unwrap()
    }

    pub fn spawn() {
        thread::spawn(|| {
            loop {
                let port = {
                    let mut current = WANTED.lock().unwrap();
                    while current.is_none() {
                        current = CHANGED.wait(current).unwrap();
                    }
                    current.expect("checked above")
                };
                match UdpSocket::bind((Ipv4Addr::LOCALHOST, port)) {
                    Ok(socket) => {
                        crate::log(&format!("motion server: listening on 127.0.0.1:{port}"));
                        let result = serve(&socket, || wanted() == Some(port), motion::open);
                        if let Err(err) = result {
                            crate::log(&format!("motion server: {err}"));
                            thread::sleep(RETRY);
                        } else {
                            crate::log("motion server: stopped");
                        }
                    }
                    Err(err) => {
                        crate::log(&format!("motion server: port {port}: {err}"));
                        // Try again when the setting changes, or after a while.
                        let current = WANTED.lock().unwrap();
                        let _ = CHANGED
                            .wait_timeout_while(current, RETRY, |w| *w == Some(port))
                            .unwrap();
                    }
                }
            }
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn client(message: u32, payload: &[u8]) -> Vec<u8> {
        let mut out = Vec::new();
        out.extend_from_slice(CLIENT_MAGIC);
        out.extend_from_slice(&VERSION.to_le_bytes());
        out.extend_from_slice(&((4 + payload.len()) as u16).to_le_bytes());
        out.extend_from_slice(&[0; 4]);
        out.extend_from_slice(&7u32.to_le_bytes());
        out.extend_from_slice(&message.to_le_bytes());
        out.extend_from_slice(payload);
        let crc = crc32(&out);
        out[8..12].copy_from_slice(&crc.to_le_bytes());
        out
    }

    fn addr(port: u16) -> SocketAddr {
        SocketAddr::from(([127, 0, 0, 1], port))
    }

    #[test]
    fn crc32_matches_the_reference_value() {
        assert_eq!(crc32(b"123456789"), 0xCBF4_3926);
        assert_eq!(crc32(b""), 0);
    }

    #[test]
    fn version_request_is_answered() {
        let mut clients = Clients::new(9);
        let reply = clients.handle(&client(MSG_VERSION, &[]), addr(1), Instant::now());
        assert_eq!(reply.len(), 1);
        assert_eq!(&reply[0][..4], SERVER_MAGIC);
        assert_eq!(reply[0].len(), HEADER + 4 + 2);
        assert_eq!(&reply[0][HEADER + 4..], &VERSION.to_le_bytes());
        // Its own checksum holds.
        let mut zeroed = reply[0].clone();
        zeroed[8..12].fill(0);
        assert_eq!(
            u32::from_le_bytes(reply[0][8..12].try_into().unwrap()),
            crc32(&zeroed)
        );
    }

    #[test]
    fn ports_request_lists_one_connected_pad() {
        let mut clients = Clients::new(9);
        let mut payload = 2i32.to_le_bytes().to_vec();
        payload.extend_from_slice(&[0, 1]);
        let reply = clients.handle(&client(MSG_PORTS, &payload), addr(1), Instant::now());
        assert_eq!(reply.len(), 2);
        assert!(reply.iter().all(|r| r.len() == HEADER + 4 + 12));
        let first = &reply[0][HEADER + 4..];
        assert_eq!((first[0], first[1], first[2]), (0, 2, 2));
        assert_eq!(&first[4..10], b"CANAPE");
        let second = &reply[1][HEADER + 4..];
        assert_eq!((second[0], second[1]), (1, 0));
        // More slots than exist are not all answered.
        let mut many = 100i32.to_le_bytes().to_vec();
        many.extend_from_slice(&[0; 100]);
        assert_eq!(
            clients
                .handle(&client(MSG_PORTS, &many), addr(1), Instant::now())
                .len(),
            SLOTS
        );
        // A negative count asks for nothing.
        let negative = (-1i32).to_le_bytes();
        assert!(
            clients
                .handle(&client(MSG_PORTS, &negative), addr(1), Instant::now())
                .is_empty()
        );
    }

    #[test]
    fn malformed_packets_are_ignored() {
        let mut clients = Clients::new(9);
        let now = Instant::now();
        let good = client(MSG_DATA, &[0, 0, 0, 0, 0, 0, 0, 0]);
        let mut bad_crc = good.clone();
        bad_crc[HEADER + 5] ^= 1;
        let mut bad_magic = good.clone();
        bad_magic[0] = b'X';
        for datagram in [&bad_crc[..], &bad_magic[..], &good[..10], &[]] {
            clients.handle(datagram, addr(1), now);
        }
        assert!(!clients.any(now));
        // Bytes after the announced length are ignored.
        let mut trailing = good.clone();
        trailing.extend_from_slice(&[1, 2, 3]);
        clients.handle(&trailing, addr(1), now);
        assert!(clients.any(now));
    }

    #[test]
    fn data_requests_subscribe_for_a_few_seconds() {
        let mut clients = Clients::new(9);
        let start = Instant::now();
        // Every pad, then the pad by slot, by address.
        let all = client(MSG_DATA, &[0, 0, 0, 0, 0, 0, 0, 0]);
        let by_slot = client(MSG_DATA, &[1, 0, 0, 0, 0, 0, 0, 0]);
        let other_slot = client(MSG_DATA, &[1, 2, 0, 0, 0, 0, 0, 0]);
        let mut by_address = vec![2, 0];
        by_address.extend_from_slice(&PAD_ADDRESS);
        let by_address = client(MSG_DATA, &by_address);
        let other_address = client(MSG_DATA, &[2, 0, 1, 2, 3, 4, 5, 6]);
        assert!(!clients.any(start));
        clients.handle(&other_slot, addr(1), start);
        clients.handle(&other_address, addr(1), start);
        assert!(!clients.any(start));
        for (port, datagram) in [(1, &all), (2, &by_slot), (3, &by_address)] {
            clients.handle(datagram, addr(port), start);
        }
        assert_eq!(clients.addresses().count(), 3);
        // Asking again keeps one entry, and keeps it alive.
        clients.handle(&all, addr(1), start + Duration::from_secs(4));
        assert_eq!(clients.addresses().count(), 3);
        assert!(clients.any(start + Duration::from_secs(6)));
        assert_eq!(clients.addresses().collect::<Vec<_>>(), vec![addr(1)]);
        assert!(!clients.any(start + Duration::from_secs(10)));
        // No more than a handful at once.
        for port in 10..20 {
            clients.handle(&all, addr(port), start);
        }
        assert_eq!(clients.addresses().count(), MAX_CLIENTS);
    }

    #[test]
    fn data_packet_has_the_published_size_and_carries_the_rates() {
        let mut clients = Clients::new(9);
        let rate = AngularVelocity {
            x: 1.5,
            y: -2.0,
            z: 30.0,
        };
        let first = clients.data(rate, Duration::from_millis(5));
        let second = clients.data(rate, Duration::from_millis(15));
        assert_eq!(first.len(), 100);
        assert_eq!(u16::from_le_bytes(first[6..8].try_into().unwrap()), 84);
        assert_eq!(&first[16..20], &MSG_DATA.to_le_bytes());
        // Connected, active, counting packets.
        assert_eq!(&first[20..22], &[SLOT, 2]);
        assert_eq!(first[31], 1);
        assert_eq!(u32::from_le_bytes(first[32..36].try_into().unwrap()), 1);
        assert_eq!(u32::from_le_bytes(second[32..36].try_into().unwrap()), 2);
        // Sticks centred.
        assert_eq!(&first[40..44], &[128; 4]);
        // Time in microseconds, then acceleration, then rotation.
        assert_eq!(u64::from_le_bytes(first[68..76].try_into().unwrap()), 5000);
        assert_eq!(&first[76..88], &[0; 12]);
        let float = |at: usize| f32::from_le_bytes(first[at..at + 4].try_into().unwrap());
        assert_eq!((float(88), float(92), float(96)), (1.5, -2.0, 30.0));
        let mut zeroed = first.clone();
        zeroed[8..12].fill(0);
        assert_eq!(
            u32::from_le_bytes(first[8..12].try_into().unwrap()),
            crc32(&zeroed)
        );
    }

    #[test]
    fn serving_over_loopback_streams_only_to_subscribers() {
        use canape_hw::motion::MockGyro;
        use std::sync::Arc;
        use std::sync::atomic::{AtomicBool, Ordering};

        let socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        let port = socket.local_addr().unwrap().port();
        let running = Arc::new(AtomicBool::new(true));
        let server = {
            let running = running.clone();
            std::thread::spawn(move || {
                serve(
                    &socket,
                    || running.load(Ordering::Relaxed),
                    || {
                        Ok(Box::new(MockGyro {
                            rate: AngularVelocity {
                                x: 0.0,
                                y: 12.5,
                                z: 0.0,
                            },
                            reads: 0,
                        }) as Box<dyn Gyro>)
                    },
                )
            })
        };
        let client_socket = UdpSocket::bind("127.0.0.1:0").unwrap();
        client_socket
            .set_read_timeout(Some(Duration::from_millis(300)))
            .unwrap();
        let server_addr = SocketAddr::from(([127, 0, 0, 1], port));
        let mut buffer = [0u8; 256];

        // Silent until it asks for data.
        client_socket
            .send_to(&client(MSG_VERSION, &[]), server_addr)
            .unwrap();
        let (len, _) = client_socket.recv_from(&mut buffer).unwrap();
        assert_eq!(len, HEADER + 4 + 2);
        assert!(client_socket.recv_from(&mut buffer).is_err());

        client_socket
            .send_to(&client(MSG_DATA, &[0; 8]), server_addr)
            .unwrap();
        let (len, _) = client_socket.recv_from(&mut buffer).unwrap();
        assert_eq!(len, 100);
        assert_eq!(&buffer[..4], SERVER_MAGIC);
        let yaw = f32::from_le_bytes(buffer[92..96].try_into().unwrap());
        assert_eq!(yaw, 12.5);

        running.store(false, Ordering::Relaxed);
        server.join().unwrap().unwrap();
    }
}
