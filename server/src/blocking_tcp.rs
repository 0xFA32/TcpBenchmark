use bytes::Bytes;
use crossbeam_channel::{Receiver, Sender};
use std::io::{self, Write};
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::common_utils::{HEADER_LEN, MAGIC};
use crate::{ServerConfig, common_utils};

// Maximum number of concurrent streams.
const MAX_CONCURRENT_STREAMS: usize = 1024;

/// A blocking TCP server.
pub struct BlockingTcpServer {
    server_config: ServerConfig,
    streams: Arc<Mutex<Vec<ClientConnection>>>,
}

/// Connection state.
#[derive(Eq, PartialEq)]
enum ConnectionState {
    Open,
    Closed,
}

/// Client connection.
pub struct ClientConnection {
    stream: TcpStream,
    state: ConnectionState,
    addr: SocketAddr,
    connected_at: Instant,
}

impl ClientConnection {
    fn new(stream: TcpStream, addr: SocketAddr, connected_at: Instant) -> ClientConnection {
        Self {
            stream,
            state: ConnectionState::Open,
            addr,
            connected_at,
        }
    }

    fn write(&mut self, data: &[u8], id: u64) -> bool {
        match self.write_payload(data, id) {
            Ok(_) => true,
            Err(_) => false,
        }
    }

    // TODO: Make it vectored.
    fn write_payload(&mut self, data: &[u8], id: u64) -> io::Result<usize> {
        let len = data.len() as u64;
        self.stream.write_all(&MAGIC.to_be_bytes())?;
        self.stream.write_all(&id.to_be_bytes())?;
        self.stream.write_all(&len.to_be_bytes())?;
        self.stream.write_all(data)?;
        Ok(data.len() + HEADER_LEN)
    }

    fn close(&mut self) {
        self.state = ConnectionState::Closed;
        let _ = self.stream.shutdown(std::net::Shutdown::Both);
    }

    fn flush(&mut self) {
        let _ = self.stream.flush();
    }
}

impl BlockingTcpServer {
    // Constructor for the blocking tcp server implementation.
    pub fn new(server_config: ServerConfig) -> BlockingTcpServer {
        Self {
            server_config: server_config,
            streams: Arc::new(Mutex::new(Vec::new())),
        }
    }

    // Start the tcp server.
    pub fn start(
        &mut self,
        sender: Sender<Bytes>,
        receiver: Receiver<Bytes>,
    ) -> std::io::Result<()> {
        tracing::info!("Starting server and listening to localhost at port 80.");
        let listener = TcpListener::bind(format!(
            "{}:{}",
            self.server_config.bind_address, self.server_config.port
        ))?;

        let close = Arc::new(AtomicBool::new(false));
        let streams = self.streams.clone();
        let close_clone = close.clone();
        let client_handler = thread::spawn(|| {
            Self::handle_client(streams, close_clone, receiver);
        });

        let close_clone = close.clone();
        let data_generator = thread::spawn(|| {
            common_utils::generate_data(sender, close_clone);
        });

        for stream in listener.incoming() {
            if stream.is_err() {
                continue;
            }

            tracing::info!("Got a new connection. Going to start working on it.");
            let tcp_stream = stream.expect("Expect tcp stream to be available at this point");
            if let Ok(mut stream_collection) = self.streams.lock() {
                if stream_collection.len() >= MAX_CONCURRENT_STREAMS {
                    tracing::info!("Rejecting connection as it is full.");
                    continue;
                }

                match tcp_stream.peer_addr() {
                    Ok(remote_addr) => {
                        let client_connection =
                            ClientConnection::new(tcp_stream, remote_addr, Instant::now());
                        stream_collection.push(client_connection);
                    }
                    Err(e) => {
                        tracing::error!("Failed to get peer address: {}", e);
                        continue;
                    }
                }

                drop(stream_collection);
            } else {
                tracing::error!("Failed to get the stream collection. Lock is poisoned!");
                close.store(true, std::sync::atomic::Ordering::Relaxed);
                break;
            }
        }

        let _ = client_handler.join();
        let _ = data_generator.join();

        if let Ok(mut streams) = self.streams.lock() {
            for stream in streams.iter_mut() {
                stream.close();
            }
        }
        Ok(())
    }

    // Handle clients.
    fn handle_client(
        clients: Arc<Mutex<Vec<ClientConnection>>>,
        close: Arc<AtomicBool>,
        receiver: Receiver<Bytes>,
    ) {
        let mut id = 0u64;
        let mut throughput = 0u64;
        let log_duration = Duration::from_secs(5);
        let mut now = Instant::now();
        loop {
            if close.load(std::sync::atomic::Ordering::Relaxed) {
                tracing::info!("Closing the client handlers");
                return;
            }

            match receiver.recv_timeout(Duration::from_millis(5)) {
                Ok(data) => {
                    id += 1;
                    throughput += data.len() as u64;
                    Self::push(data, &clients, id);
                }
                Err(_) => {
                    continue;
                }
            }

            if now.elapsed() > log_duration {
                now = Instant::now();
                let bps = throughput / 5;
                throughput = 0;

                tracing::info!("Pushed {} bps across clients.", bps);
            }
        }
    }

    // Push data to all the clients.
    fn push(data: Bytes, clients: &Arc<Mutex<Vec<ClientConnection>>>, id: u64) -> bool {
        if let Ok(mut clients) = clients.lock() {
            for client in clients.iter_mut() {
                if !client.write(&data, id) {
                    tracing::info!("Going to close the client");
                    client.close();
                }

                // Potentially batch.
                client.flush();
            }

            // Removed closed connections.
            clients.retain_mut(|client| client.state != ConnectionState::Closed);
        }

        return true;
    }
}
