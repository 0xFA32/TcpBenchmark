use std::cmp::min;
use std::error::Error;
use std::io::Read;
use std::net::SocketAddr;
use std::sync::atomic::AtomicBool;
use std::thread;

use bytes::{Buf, Bytes, BytesMut};
use mio::net::{TcpListener, TcpStream};
use mio::{Events, Interest, Poll, Token};

use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

// Configuration for the client.
pub struct ClientConfig {
    pub address: SocketAddr,
    pub max_concurrent_streams: usize,
}

impl Default for ClientConfig {
    fn default() -> Self {
        Self {
            address: "127.0.0.1:80".parse().unwrap(),
            max_concurrent_streams: 4,
        }
    }
}

/// Connection state.
#[derive(Eq, PartialEq)]
enum ConnectionState {
    Open,
    Closed,
}

/// Denoting the location on the payload.
enum Location {
    // 24 bytes.
    Header,
    // Offset within payload.
    Payload(usize),
}

struct Cursor {
    client: TcpStream,
    state: ConnectionState,
    staging: BytesMut,
    offset: Location,
    payloads: Vec<Bytes>,
}

fn main() {
    setup_logger();

    tracing::info!("Going to start working on the client");
    let worker = thread::spawn(|| {
        let client_config = ClientConfig::default();
        let _ = start(&client_config);
    });

    worker.join().unwrap();
    tracing::info!("Done spawning clients.");
}

fn start(client_config: &ClientConfig) -> Result<(), Box<dyn Error>> {
    let mut poll = Poll::new()?;
    let mut events = Events::with_capacity(client_config.max_concurrent_streams);

    let mut clients = Vec::new();
    for num in 0..client_config.max_concurrent_streams {
        let mut client = TcpStream::connect(client_config.address)?;
        poll.registry()
            .register(&mut client, Token(num), Interest::READABLE)?;
        let cursor = Cursor {
            client: client,
            state: ConnectionState::Open,
            staging: BytesMut::with_capacity(8 * 1024),
            offset: Location::Header,
            payloads: Vec::new(),
        };
        clients.push(cursor);

        tracing::info!("Opened a new connection.");
    }

    tracing::info!("Opened all the connections. Going to start working on it.");
    let mut buf = [0u8; 4096];
    loop {
        poll.poll(&mut events, None)?;

        if clients.iter().all(|c| c.state == ConnectionState::Closed) {
            tracing::info!("All connections are closed. Ending the client application");
            break;
        }

        for e in events.iter() {
            let index = e.token().0;
            if e.is_readable() {
                let cursor = clients
                    .get_mut(index)
                    .expect("Expect client to be available");
                match cursor.client.read(&mut buf) {
                    Ok(0) => {
                        tracing::error!("Recieved read 0 length");
                        cursor.state = ConnectionState::Closed;
                        continue;
                    }
                    Ok(len) => {
                        handle_client_read(cursor, &buf, len);
                    }
                    Err(e) => {
                        tracing::error!("Read error: {}", e);
                        cursor.state = ConnectionState::Closed;
                        continue;
                    }
                }
            }
        }
    }

    Ok(())
}

fn handle_client_read(cursor: &mut Cursor, buf: &[u8], len: usize) {
    tracing::info!("Reading from the socket!");
    if cursor.state == ConnectionState::Closed {
        tracing::info!("Connection is already closed!!");
        return;
    }

    cursor.staging.extend_from_slice(&buf[0..len]);

    loop {
        let should_continue = match cursor.offset {
            Location::Header => handle_header(cursor),
            Location::Payload(len) => handle_payload(cursor, len),
        };

        if !should_continue {
            break;
        }
    }
}

fn handle_header(cursor: &mut Cursor) -> bool {
    if cursor.staging.len() < 24 {
        return false;
    }

    let magic = u64::from_be_bytes(cursor.staging[..8].try_into().unwrap());

    if magic != 0x0123_4567_89ab_cdef {
        panic!("Invalid magic value!");
    }

    let id = u64::from_be_bytes(cursor.staging[8..16].try_into().unwrap());
    let len = u64::from_be_bytes(cursor.staging[16..24].try_into().unwrap());

    cursor.staging.advance(24);

    cursor.offset = Location::Payload(len as usize);

    return cursor.staging.len() > 0;
}

fn handle_payload(cursor: &mut Cursor, len: usize) -> bool {
    if cursor.staging.len() < len {
        return false;
    }

    let _payload = cursor.staging.split_to(len).freeze();

    cursor.offset = Location::Header;
    return cursor.staging.len() > 0;
}

fn setup_logger() {
    let file_appender = RollingFileAppender::new(Rotation::HOURLY, "logs", "client.log");

    tracing_subscriber::registry()
        .with(fmt::layer().with_ansi(false).with_writer(file_appender))
        .with(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .init();
}
