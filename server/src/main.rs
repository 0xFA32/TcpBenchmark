mod blocking_tcp;
mod common_utils;

use crossbeam_channel::unbounded;
use strum::Display;
use tracing_appender::rolling::{RollingFileAppender, Rotation};
use tracing_subscriber::{EnvFilter, fmt, prelude::*};

use crate::blocking_tcp::BlockingTcpServer;

#[derive(Display)]
enum ServerType {
    Blocking,
    AsyncTokio,
    AsyncIOUring,
}

// Configuration for the server.
pub struct ServerConfig {
    pub bind_address: String,
    pub port: usize,
    pub max_concurrent_streams: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            bind_address: "127.0.0.1".to_string(),
            max_concurrent_streams: 1024,
            port: 80,
        }
    }
}

fn main() {
    let server_type = ServerType::Blocking;
    match server_type {
        ServerType::Blocking => {
            setup_logger(server_type);
            if let Err(e) = start_blocking_server(ServerConfig::default()) {
                tracing::error!("Server failed to start: {}", e);
                std::process::exit(1);
            }
        }
        _ => {
            panic!("Not yet implemented server type");
        }
    }
}

fn start_blocking_server(server_config: ServerConfig) -> std::io::Result<()> {
    let (tx, rx) = unbounded();
    let mut blocking_tcp_server = BlockingTcpServer::new(server_config);
    blocking_tcp_server.start(tx, rx)
}

fn setup_logger(server_type: ServerType) {
    let file_appender =
        RollingFileAppender::new(Rotation::HOURLY, "logs", format!("{server_type}.log"));

    tracing_subscriber::registry()
        .with(fmt::layer().with_writer(file_appender))
        .with(EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .init();
}
