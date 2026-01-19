use bytes::Bytes;
use crossbeam_channel::{Receiver, Sender};
use std::io::Write;
use std::net::{SocketAddr, TcpListener, TcpStream};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::{ServerConfig, common_utils};

// Maximum number of concurrent streams.
const MAX_CONCURRENT_STREAMS: usize = 1024;

/// A blocking TCP server.
pub struct BlockingTcpServer {
    server_config: ServerConfig,
    streams: Arc<Mutex<Vec<ClientConnection>>>,
    total_streams: usize,
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

    fn write(&mut self, data: &[u8]) -> bool {
        match self.stream.write(data) {
            Ok(_) => true,
            Err(_) => false,
        }
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
            total_streams: 0,
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
            if self.total_streams > MAX_CONCURRENT_STREAMS {
                tracing::info!("Rejecting connection as it is full.");
                continue;
            }

            if stream.is_err() {
                continue;
            }

            let tcp_stream = stream.expect("Expect tcp stream to be available at this point");
            self.total_streams += 1;
            if let Ok(mut stream_collection) = self.streams.lock() {
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
        loop {
            if close.load(std::sync::atomic::Ordering::Relaxed) {
                tracing::info!("Closing the client handlers");
                return;
            }

            match receiver.recv_timeout(Duration::from_millis(5)) {
                Ok(data) => {
                    Self::push(data, &clients);
                }
                Err(_) => {
                    continue;
                }
            }
        }
    }

    // Push data to all the clients.
    fn push(data: Bytes, clients: &Arc<Mutex<Vec<ClientConnection>>>) -> bool {
        if let Ok(mut clients) = clients.lock() {
            for client in clients.iter_mut() {
                if !client.write(&data) {
                    client.close();
                }

                // Potentially batch.
                client.flush();
            }


            // Removed closed connections.
            clients.retain_mut(|client| {
                client.state != ConnectionState::Closed
            });
        
        }

        return true;
    }
}
