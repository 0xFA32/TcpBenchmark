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

fn main() {
    let server_type = ServerType::Blocking;
    match server_type {
        ServerType::Blocking => {
            setup_logger(server_type);
            start_blocking_server();
        }
        _ => {
            panic!("Not yet implemented server type");
        }
    }
}

fn start_blocking_server() -> std::io::Result<()> {
    let (tx, rx) = unbounded();
    let mut blocking_tcp_server = BlockingTcpServer::new();
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
