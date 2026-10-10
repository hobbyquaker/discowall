//! The control protocol between `discowalld` and `discoctl`: JSON lines over a unix socket, one
//! request per line, one response per line.

use serde::{Deserialize, Serialize};
use std::net::IpAddr;

/// The default path of the control socket.
pub const DEFAULT_SOCKET: &str = "/run/discowall/control.sock";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "cmd", rename_all = "snake_case")]
pub enum Request {
    Ping,
    Status,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum Response {
    Pong,
    Status(Status),
    Error { message: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Status {
    pub version: String,
    pub uptime_s: u64,
    pub config_path: String,
    pub interfaces: Vec<InterfaceStatus>,
    pub counters: Counters,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct InterfaceStatus {
    pub name: String,
    /// `None` while the interface does not exist.
    pub index: Option<u32>,
    pub up: bool,
    pub joined_v4: bool,
    pub joined_v6: bool,
    pub addresses: Vec<IpAddr>,
    pub rx: u64,
    pub tx: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Counters {
    pub rx: u64,
    pub tx: u64,
    pub dropped_own_source: u64,
    pub dropped_duplicate: u64,
    pub dropped_unknown_interface: u64,
    pub send_errors: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn request_round_trip() {
        let line = serde_json::to_string(&Request::Status).unwrap();
        assert_eq!(line, r#"{"cmd":"status"}"#);
        assert_eq!(
            serde_json::from_str::<Request>(&line).unwrap(),
            Request::Status
        );
    }

    #[test]
    fn response_round_trip() {
        let r = Response::Error {
            message: "x".into(),
        };
        let line = serde_json::to_string(&r).unwrap();
        assert_eq!(line, r#"{"result":"error","message":"x"}"#);
        assert_eq!(serde_json::from_str::<Response>(&line).unwrap(), r);
    }
}
