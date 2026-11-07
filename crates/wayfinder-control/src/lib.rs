//! Client for Savage Wind's optional local control server.
//!
//! Start Savage Wind with `SAVAGEWIND_CONTROL=7777` and it listens on
//! `127.0.0.1:7777`. The protocol is one JSON request per line and one JSON
//! reply per line; it is deliberately not HTTP. Only read-only, player-safe
//! commands are exposed by this crate.
//!
//! ```no_run
//! # async fn demo() -> Result<(), wayfinder_control::ControlError> {
//! use wayfinder_control::ControlClient;
//!
//! let client = ControlClient::from_port(7777);
//! let pos = client.pos().await?;
//! println!("Link is at x={} y={} z={}", pos.x, pos.y, pos.z);
//! # Ok(())
//! # }
//! ```

use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::net::tcp::OwnedWriteHalf;
use tokio::time::timeout;

pub const DEFAULT_PORT: u16 = 7777;
pub const DEFAULT_ADDR: &str = "127.0.0.1:7777";

#[derive(Debug, thiserror::Error)]
pub enum ControlError {
    #[error("could not connect to Savage Wind at {addr}: {source}")]
    Connect {
        addr: SocketAddr,
        #[source]
        source: std::io::Error,
    },

    #[error("control request timed out after {0:?}")]
    Timeout(Duration),

    #[error("control connection closed before a reply arrived")]
    Closed,

    #[error("invalid JSON from Savage Wind: {0}")]
    Json(#[from] serde_json::Error),

    #[error("Savage Wind rejected the command: {0}")]
    Remote(String),

    #[error("malformed control reply: {0}")]
    Protocol(String),

    #[error(transparent)]
    Io(#[from] std::io::Error),
}

/// A connection to one Savage Wind control server.
#[derive(Debug, Clone)]
pub struct ControlClient {
    addr: SocketAddr,
    timeout: Duration,
}

impl ControlClient {
    pub fn new(addr: SocketAddr) -> Self {
        Self {
            addr,
            timeout: Duration::from_secs(2),
        }
    }

    pub fn from_port(port: u16) -> Self {
        Self::new(SocketAddr::from(([127, 0, 0, 1], port)))
    }

    pub fn from_addr(addr: &str) -> Result<Self, std::net::AddrParseError> {
        Ok(Self::new(addr.parse()?))
    }

    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    pub fn addr(&self) -> SocketAddr {
        self.addr
    }

    /// Open a connection. The caller can reuse it for many commands.
    pub async fn connect(&self) -> Result<ControlConnection, ControlError> {
        let stream = timeout(self.timeout, TcpStream::connect(self.addr))
            .await
            .map_err(|_| ControlError::Timeout(self.timeout))?
            .map_err(|source| ControlError::Connect {
                addr: self.addr,
                source,
            })?;
        stream.set_nodelay(true)?;
        let (reader, writer) = stream.into_split();
        Ok(ControlConnection {
            reader: BufReader::new(reader),
            writer,
            timeout: self.timeout,
            next_id: 0,
        })
    }

    /// Run one command on a fresh connection.
    pub async fn command(&self, command: &str) -> Result<Value, ControlError> {
        let mut connection = self.connect().await?;
        connection.command(command).await
    }

    /// Read Link's position and stamina (`pos`).
    pub async fn pos(&self) -> Result<Pos, ControlError> {
        let mut connection = self.connect().await?;
        connection.pos().await
    }

    /// Read frame/uptime/FPS/hour information (`state`).
    pub async fn state(&self) -> Result<State, ControlError> {
        let mut connection = self.connect().await?;
        connection.state().await
    }

    /// Read the player-safe version information (`version`).
    pub async fn version(&self) -> Result<Value, ControlError> {
        self.command("version").await
    }
}

/// A persistent control connection.
#[derive(Debug)]
pub struct ControlConnection {
    reader: BufReader<tokio::net::tcp::OwnedReadHalf>,
    writer: OwnedWriteHalf,
    timeout: Duration,
    next_id: u64,
}

impl ControlConnection {
    /// Send a command and return the `data` object from the reply.
    pub async fn command(&mut self, command: &str) -> Result<Value, ControlError> {
        if command.contains('\n') || command.contains('\r') {
            return Err(ControlError::Protocol(
                "command must be a single line".into(),
            ));
        }

        self.next_id += 1;
        let request = format!("{command}\n");
        timeout(self.timeout, self.writer.write_all(request.as_bytes()))
            .await
            .map_err(|_| ControlError::Timeout(self.timeout))??;
        timeout(self.timeout, self.writer.flush())
            .await
            .map_err(|_| ControlError::Timeout(self.timeout))??;

        let mut line = String::new();
        let read = timeout(self.timeout, self.reader.read_line(&mut line))
            .await
            .map_err(|_| ControlError::Timeout(self.timeout))??;
        if read == 0 {
            return Err(ControlError::Closed);
        }

        let reply: Reply = serde_json::from_str(&line)?;
        if !reply.ok {
            return Err(ControlError::Remote(
                reply.error.unwrap_or_else(|| "unknown error".into()),
            ));
        }
        Ok(reply.data)
    }

    pub async fn pos(&mut self) -> Result<Pos, ControlError> {
        let data = self.command("pos").await?;
        serde_json::from_value(data).map_err(ControlError::Json)
    }

    pub async fn state(&mut self) -> Result<State, ControlError> {
        let data = self.command("state").await?;
        serde_json::from_value(data).map_err(ControlError::Json)
    }

    pub async fn camera(&mut self) -> Result<Camera, ControlError> {
        let data = self.command("camera").await?;
        serde_json::from_value(data).map_err(ControlError::Json)
    }

    pub async fn version(&mut self) -> Result<Value, ControlError> {
        self.command("version").await
    }
}

#[derive(Debug, Deserialize)]
struct Reply {
    ok: bool,
    #[serde(default)]
    data: Value,
    #[serde(default)]
    error: Option<String>,
}

/// Link's live position and stamina from the `pos` command.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Pos {
    /// Left/right in the game world. Leaflet longitude.
    pub x: f64,
    /// Height. Not used for horizontal map placement.
    pub y: f64,
    /// Forward/back in the game world. Leaflet latitude is `-z`.
    pub z: f64,
    #[serde(default)]
    pub stamina: Option<f64>,
    /// A place line such as `swr:1 x=-1234.56 ...`.
    #[serde(default)]
    pub place: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Engine state from the `state` command.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct State {
    #[serde(default)]
    pub frame: i64,
    #[serde(default)]
    pub uptime_s: f64,
    #[serde(default)]
    pub fps: Option<f64>,
    #[serde(default)]
    pub hour: Option<f64>,
    #[serde(default)]
    pub hour_held: bool,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Camera state from the `camera` command.
#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct Camera {
    #[serde(default)]
    pub x: Option<f64>,
    #[serde(default)]
    pub y: Option<f64>,
    #[serde(default)]
    pub z: Option<f64>,
    #[serde(default)]
    pub yaw: Option<f64>,
    #[serde(default)]
    pub pitch: Option<f64>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
    use tokio::net::TcpListener;

    #[tokio::test]
    async fn sends_commands_and_decodes_pos() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();

        let server = tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (reader, mut writer) = stream.into_split();
            let mut reader = BufReader::new(reader);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            assert_eq!(line, "pos\n");
            writer
                .write_all(
                    b"{\"ok\":true,\"data\":{\"x\":1.5,\"y\":2.0,\"z\":-3.25,\"stamina\":1.0,\"place\":\"swr:1\"}}\n",
                )
                .await
                .unwrap();
        });

        let client = ControlClient::new(addr);
        let pos = client.pos().await.unwrap();
        assert_eq!(pos.x, 1.5);
        assert_eq!(pos.y, 2.0);
        assert_eq!(pos.z, -3.25);
        assert_eq!(pos.stamina, Some(1.0));
        assert_eq!(pos.place.as_deref(), Some("swr:1"));
        server.await.unwrap();
    }

    #[tokio::test]
    async fn surfaces_remote_errors() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (reader, mut writer) = stream.into_split();
            let mut reader = BufReader::new(reader);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            writer
                .write_all(b"{\"ok\":false,\"error\":\"Unknown command\"}\n")
                .await
                .unwrap();
        });

        let error = ControlClient::new(addr).command("bogus").await.unwrap_err();
        assert!(matches!(error, ControlError::Remote(message) if message == "Unknown command"));
    }

    #[tokio::test]
    async fn decodes_state() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let (stream, _) = listener.accept().await.unwrap();
            let (reader, mut writer) = stream.into_split();
            let mut reader = BufReader::new(reader);
            let mut line = String::new();
            reader.read_line(&mut line).await.unwrap();
            writer
                .write_all(
                    b"{\"ok\":true,\"data\":{\"frame\":42,\"uptime_s\":10.5,\"fps\":59.9,\"hour\":18.5,\"hour_held\":false}}\n",
                )
                .await
                .unwrap();
        });

        let state = ControlClient::new(addr).state().await.unwrap();
        assert_eq!(state.frame, 42);
        assert_eq!(state.fps, Some(59.9));
        assert_eq!(state.hour, Some(18.5));
    }
}
