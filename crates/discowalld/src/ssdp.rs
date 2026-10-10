//! SSDP (UPnP device discovery, 239.255.255.250:1900 / ff02::c): HTTP-like text over UDP.
//!
//! `NOTIFY * HTTP/1.1` carries `NTS: ssdp:alive|ssdp:byebye|ssdp:update`, `NT` (the type), `USN`
//! (`uuid:…` optionally `::<type>`), `LOCATION` (the device description URL) and `CACHE-CONTROL:
//! max-age=N`. `M-SEARCH * HTTP/1.1` carries `ST` (the search target) and `MX` (seconds a responder
//! may wait). `HTTP/1.1 200 OK` is the unicast answer to a search, with `ST`, `USN`, `LOCATION`.
//! For the rules: `service` is the NT/ST, `instance` the uuid, `host` the LOCATION's host.

use std::collections::BTreeMap;
use std::fmt;
use std::net::IpAddr;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SsdpKind {
    Search,
    Alive,
    Byebye,
    Update,
    Response,
}

impl fmt::Display for SsdpKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            SsdpKind::Search => "m-search",
            SsdpKind::Alive => "alive",
            SsdpKind::Byebye => "byebye",
            SsdpKind::Update => "update",
            SsdpKind::Response => "response",
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsdpMessage {
    pub kind: SsdpKind,
    /// NT of a notification, ST of a search or a response (`upnp:rootdevice`, `ssdp:all`,
    /// `urn:schemas-upnp-org:device:MediaRenderer:1`, `uuid:…`).
    pub target: Option<String>,
    /// The uuid part of the USN, lowercased (`uuid:…`).
    pub uuid: Option<String>,
    /// The LOCATION URL as sent.
    pub location: Option<String>,
    /// The LOCATION's host, lowercased, and its port.
    pub location_host: Option<String>,
    pub location_port: Option<u16>,
    /// `CACHE-CONTROL: max-age` of a notification or response, seconds.
    pub max_age: Option<u32>,
    /// `MX` of a search, seconds.
    pub mx: Option<u32>,
    pub headers: BTreeMap<String, String>,
}

/// Why the bytes are not an SSDP message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SsdpError(pub String);

impl fmt::Display for SsdpError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

fn parse_url_host(url: &str) -> Option<(String, Option<u16>)> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    let authority = authority
        .rsplit_once('@')
        .map(|(_, h)| h)
        .unwrap_or(authority);
    if let Some(v6) = authority.strip_prefix('[') {
        let (host, tail) = v6.split_once(']')?;
        let port = tail.strip_prefix(':').and_then(|p| p.parse().ok());
        return Some((host.to_lowercase(), port));
    }
    match authority.rsplit_once(':') {
        Some((host, port)) if port.chars().all(|c| c.is_ascii_digit()) && !port.is_empty() => {
            Some((host.to_lowercase(), port.parse().ok()))
        }
        _ => Some((authority.to_lowercase(), None)),
    }
}

impl SsdpMessage {
    pub fn parse(bytes: &[u8]) -> Result<Self, SsdpError> {
        if bytes.len() < 12 || bytes.len() > 8192 {
            return Err(SsdpError("not an SSDP message (size)".into()));
        }
        let text = std::str::from_utf8(bytes).map_err(|_| SsdpError("not UTF-8".into()))?;
        let mut lines = text.split("\r\n");
        let start = lines.next().unwrap_or("").trim_end();
        let mut headers = BTreeMap::new();
        for line in lines {
            if line.is_empty() {
                break;
            }
            if let Some((k, v)) = line.split_once(':') {
                headers.insert(k.trim().to_ascii_uppercase(), v.trim().to_string());
            }
        }
        let kind = if start.starts_with("M-SEARCH ") {
            if !headers
                .get("MAN")
                .is_some_and(|m| m.trim_matches('"') == "ssdp:discover")
            {
                return Err(SsdpError("M-SEARCH without MAN: \"ssdp:discover\"".into()));
            }
            SsdpKind::Search
        } else if start.starts_with("NOTIFY ") {
            match headers.get("NTS").map(String::as_str) {
                Some("ssdp:alive") => SsdpKind::Alive,
                Some("ssdp:byebye") => SsdpKind::Byebye,
                Some("ssdp:update") => SsdpKind::Update,
                other => return Err(SsdpError(format!("NOTIFY with NTS {other:?}"))),
            }
        } else if start.starts_with("HTTP/1.1 200") || start.starts_with("HTTP/1.0 200") {
            SsdpKind::Response
        } else {
            return Err(SsdpError(format!("not an SSDP start line: {start:?}")));
        };
        let target = headers
            .get(if kind == SsdpKind::Search || kind == SsdpKind::Response {
                "ST"
            } else {
                "NT"
            })
            .map(|t| t.to_lowercase());
        let uuid = headers
            .get("USN")
            .map(|u| u.split("::").next().unwrap_or(u).to_lowercase())
            .filter(|u| u.starts_with("uuid:"));
        let location = headers.get("LOCATION").cloned();
        let (location_host, location_port) = location
            .as_deref()
            .and_then(parse_url_host)
            .map(|(h, p)| (Some(h), p))
            .unwrap_or((None, None));
        let max_age = headers.get("CACHE-CONTROL").and_then(|c| {
            c.split(',').find_map(|part| {
                let (k, v) = part.trim().split_once('=')?;
                (k.trim().eq_ignore_ascii_case("max-age"))
                    .then(|| v.trim().parse().ok())
                    .flatten()
            })
        });
        let mx = headers.get("MX").and_then(|m| m.trim().parse().ok());
        Ok(Self {
            kind,
            target,
            uuid,
            location,
            location_host,
            location_port,
            max_age,
            mx,
            headers,
        })
    }

    pub fn is_search(&self) -> bool {
        self.kind == SsdpKind::Search
    }

    /// The LOCATION's host as an address, when it is one.
    pub fn location_ip(&self) -> Option<IpAddr> {
        self.location_host.as_deref().and_then(|h| h.parse().ok())
    }

    pub fn summary(&self) -> String {
        let mut s = format!("ssdp {}", self.kind);
        if let Some(t) = &self.target {
            s.push(' ');
            s.push_str(t);
        }
        if let Some(u) = &self.uuid {
            s.push(' ');
            s.push_str(u);
        }
        if let Some(h) = &self.location_host {
            s.push_str(" @");
            s.push_str(h);
        }
        s
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const ALIVE: &str = "NOTIFY * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nCACHE-CONTROL: max-age=1800\r\nLOCATION: http://192.168.30.40:49152/description.xml\r\nNT: urn:schemas-upnp-org:device:MediaRenderer:1\r\nNTS: ssdp:alive\r\nSERVER: Linux/5.4 UPnP/1.0 Sonos/70\r\nUSN: uuid:RINCON_12345::urn:schemas-upnp-org:device:MediaRenderer:1\r\n\r\n";
    const SEARCH: &str = "M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: 2\r\nST: ssdp:all\r\n\r\n";
    const RESPONSE: &str = "HTTP/1.1 200 OK\r\nCACHE-CONTROL: max-age=120\r\nEXT:\r\nLOCATION: http://[fd00::40]:8080/d.xml\r\nST: upnp:rootdevice\r\nUSN: uuid:abc::upnp:rootdevice\r\n\r\n";

    #[test]
    fn alive() {
        let m = SsdpMessage::parse(ALIVE.as_bytes()).unwrap();
        assert_eq!(m.kind, SsdpKind::Alive);
        assert_eq!(
            m.target.as_deref(),
            Some("urn:schemas-upnp-org:device:mediarenderer:1")
        );
        assert_eq!(m.uuid.as_deref(), Some("uuid:rincon_12345"));
        assert_eq!(m.location_host.as_deref(), Some("192.168.30.40"));
        assert_eq!(m.location_port, Some(49152));
        assert_eq!(m.max_age, Some(1800));
        assert_eq!(m.location_ip(), Some("192.168.30.40".parse().unwrap()));
        assert_eq!(m.summary(), "ssdp alive urn:schemas-upnp-org:device:mediarenderer:1 uuid:rincon_12345 @192.168.30.40");
    }

    #[test]
    fn search_and_response() {
        let m = SsdpMessage::parse(SEARCH.as_bytes()).unwrap();
        assert!(m.is_search());
        assert_eq!(m.target.as_deref(), Some("ssdp:all"));
        assert_eq!(m.mx, Some(2));
        let r = SsdpMessage::parse(RESPONSE.as_bytes()).unwrap();
        assert_eq!(r.kind, SsdpKind::Response);
        assert_eq!(r.location_host.as_deref(), Some("fd00::40"));
        assert_eq!(r.location_port, Some(8080));
        assert_eq!(r.uuid.as_deref(), Some("uuid:abc"));
        assert_eq!(r.max_age, Some(120));
    }

    #[test]
    fn rejects() {
        assert!(SsdpMessage::parse(b"GET / HTTP/1.1\r\n\r\n").is_err());
        assert!(
            SsdpMessage::parse(b"M-SEARCH * HTTP/1.1\r\nST: ssdp:all\r\n\r\n").is_err(),
            "no MAN"
        );
        assert!(
            SsdpMessage::parse(b"NOTIFY * HTTP/1.1\r\nNT: x\r\n\r\n").is_err(),
            "no NTS"
        );
        assert!(SsdpMessage::parse(b"\x00\x01").is_err());
        assert!(SsdpMessage::parse(&[0xff; 40]).is_err());
    }

    #[test]
    fn url_hosts() {
        assert_eq!(
            parse_url_host("http://10.0.0.1/x"),
            Some(("10.0.0.1".into(), None))
        );
        assert_eq!(
            parse_url_host("http://Host.local:80/"),
            Some(("host.local".into(), Some(80)))
        );
        assert_eq!(
            parse_url_host("http://[fe80::1]/"),
            Some(("fe80::1".into(), None))
        );
        assert_eq!(parse_url_host("nonsense"), None);
    }
}
