use bytes::Bytes;
use crossbeam_channel::{Receiver, Sender};
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex};
use std::thread;

use crate::common_utils;

// Maximum number of concurrent streams.
const MAX_CONCURRENT_STREAMS: usize = 1024;

/// A blocking TCP server.
pub struct BlockingTcpServer {
    streams: Arc<Mutex<Vec<TcpStream>>>,
    total_streams: usize,
}

impl BlockingTcpServer {
    // Constructor for the blocking tcp server implementation.
    pub fn new() -> BlockingTcpServer {
        Self {
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
        let listener = TcpListener::bind("127.0.0.1:80")?;

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

            self.total_streams += 1;
            if let Ok(mut stream_collection) = self.streams.lock() {
                stream_collection.push(stream?);
            } else {
                tracing::error!("Failed to get the stream collection. Lock is posioned!");
                close.store(false, std::sync::atomic::Ordering::Relaxed);
                break;
            }
        }

        let _ = client_handler.join();
        let _ = data_generator.join();
        Ok(())
    }

    // Handle clients.
    fn handle_client(
        clients: Arc<Mutex<Vec<TcpStream>>>,
        close: Arc<AtomicBool>,
        receiver: Receiver<Bytes>,
    ) {
    }
}
