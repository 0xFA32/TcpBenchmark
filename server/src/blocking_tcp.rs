use bytes::Bytes;
use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};

/// A blocking TCP server.
struct BlockingTcpServer {}
