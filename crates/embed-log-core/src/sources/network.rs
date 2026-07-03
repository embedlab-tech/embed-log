#![cfg_attr(not(feature = "pcap-capture"), allow(dead_code))]

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr};

use anyhow::{bail, Result};
use chrono::Local;
use serde_json::json;
use tokio::sync::mpsc;
use tokio::time::{self, Duration};
use tracing::debug;

use super::traits::LogSource;
use crate::{
    config::{NetworkUdpCaptureConfig, PayloadConfig},
    models::LogEntry,
};

const DLT_NULL: i32 = 0;
const DLT_EN10MB: i32 = 1;
const DLT_RAW: i32 = 12;
const DLT_LINUX_SLL: i32 = 113;
const DLT_IPV4: i32 = 228;
const DLT_IPV6: i32 = 229;

/// Deterministic network-capture source.
///
/// - `mock` backend emits synthetic events for tests/demos.
/// - `pcap` backend performs a small UDP-only capture with kernel BPF.
pub struct NetworkCaptureSource {
    name: String,
    interface: String,
    bpf_filter: String,
    backend: String,
    interval: Duration,
    udp: Option<NetworkUdpCaptureConfig>,
    payload: PayloadConfig,
    snaplen: u32,
    promisc: bool,
}

struct ParsedUdpPacket<'a> {
    src_ip: IpAddr,
    dst_ip: IpAddr,
    src_port: u16,
    dst_port: u16,
    payload: &'a [u8],
    payload_len: usize,
}

impl NetworkCaptureSource {
    pub fn new(
        name: impl Into<String>,
        interface: impl Into<String>,
        bpf_filter: impl Into<String>,
        backend: impl Into<String>,
        mock_interval_secs: Option<f64>,
        udp: Option<NetworkUdpCaptureConfig>,
        payload: Option<PayloadConfig>,
        snaplen: Option<u32>,
        promisc: Option<bool>,
    ) -> Self {
        let interval = Duration::from_secs_f64(mock_interval_secs.unwrap_or(1.0).max(0.001));
        Self {
            name: name.into(),
            interface: interface.into(),
            bpf_filter: bpf_filter.into(),
            backend: backend.into(),
            interval,
            udp,
            payload: payload.unwrap_or_default(),
            snaplen: snaplen.unwrap_or(256).max(64),
            promisc: promisc.unwrap_or(false),
        }
    }

    fn effective_filter(&self) -> String {
        let structured = self.udp.as_ref().and_then(build_udp_capture_filter);
        let raw = self.bpf_filter.trim();
        match (structured, raw.is_empty()) {
            (Some(structured), false) => format!("({structured}) and ({raw})"),
            (Some(structured), true) => structured,
            (None, false) => raw.to_string(),
            (None, true) => "udp".to_string(),
        }
    }

    fn mock_message(&self, seq: u64) -> String {
        format!(
            "network interface={} backend=mock seq={} filter={}",
            self.interface,
            seq,
            self.effective_filter()
        )
    }

    #[cfg(feature = "pcap-capture")]
    fn pcap_loop(self, tx: mpsc::Sender<LogEntry>) -> Result<()> {
        use anyhow::anyhow;
        use chrono::{TimeZone, Utc};

        fn packet_timestamp_local(header: &pcap::PacketHeader) -> chrono::DateTime<Local> {
            let secs = header.ts.tv_sec;
            let micros = header.ts.tv_usec.max(0) as u32;
            let nanos = micros.saturating_mul(1_000).min(999_999_999);
            Utc.timestamp_opt(secs, nanos)
                .single()
                .map(|dt| dt.with_timezone(&Local))
                .unwrap_or_else(Local::now)
        }

        let filter = self.effective_filter();
        let inactive = pcap::Capture::from_device(self.interface.as_str())?;
        let mut capture = inactive
            .promisc(self.promisc)
            .snaplen(self.snaplen as i32)
            .timeout(250)
            .immediate_mode(true)
            .open()?;
        capture.filter(&filter, true)?;
        let linktype = capture.get_datalink().0;

        loop {
            match capture.next_packet() {
                Ok(packet) => {
                    let Some(parsed) = parse_udp_packet(linktype, packet.data) else {
                        continue;
                    };
                    let ts = packet_timestamp_local(&packet.header);
                    let message = format_udp_packet_line(&self.interface, &parsed, &self.payload);
                    let packet_meta = build_udp_packet_meta(&self.interface, &parsed, &self.payload);
                    if tx
                        .blocking_send(
                            LogEntry::new(ts, self.name.clone(), message).with_meta(packet_meta),
                        )
                        .is_err()
                    {
                        debug!("[{}] channel closed, stopping", self.name);
                        return Ok(());
                    }
                }
                Err(pcap::Error::TimeoutExpired) => continue,
                Err(err) => return Err(anyhow!(err)),
            }
        }
    }
}

#[async_trait::async_trait]
impl LogSource for NetworkCaptureSource {
    async fn run(self: Box<Self>, tx: mpsc::Sender<LogEntry>) -> Result<()> {
        match self.backend.as_str() {
            "mock" => {
                let mut seq: u64 = 0;
                let mut ticker = time::interval(self.interval);
                loop {
                    ticker.tick().await;
                    seq += 1;
                    let entry = LogEntry::new(Local::now(), self.name.clone(), self.mock_message(seq));
                    if tx.send(entry).await.is_err() {
                        debug!("[{}] channel closed, stopping", self.name);
                        return Ok(());
                    }
                }
            }
            #[cfg(feature = "pcap-capture")]
            "pcap" => tokio::task::spawn_blocking(move || self.pcap_loop(tx)).await?,
            #[cfg(not(feature = "pcap-capture"))]
            "pcap" => bail!(
                "network_capture backend 'pcap' requires the 'pcap-capture' cargo feature and libpcap/Npcap at build time"
            ),
            other => bail!(
                "network_capture backend {:?} is not supported; use network_backend: mock or pcap",
                other
            ),
        }
    }

    fn source_name(&self) -> &str {
        &self.name
    }

    fn source_type(&self) -> &str {
        "network_capture"
    }
}

fn build_udp_capture_filter(udp: &NetworkUdpCaptureConfig) -> Option<String> {
    let mut clauses = Vec::new();
    if !udp.ports.is_empty() {
        let ports = udp
            .ports
            .iter()
            .map(|port| format!("port {port}"))
            .collect::<Vec<_>>()
            .join(" or ");
        clauses.push(format!("({ports})"));
    }
    if let Some(host) = udp.host.as_deref().filter(|v| !v.trim().is_empty()) {
        clauses.push(format!("host {host}"));
    }
    if !udp.src_ips.is_empty() {
        let ips = udp
            .src_ips
            .iter()
            .map(|ip| format!("src host {ip}"))
            .collect::<Vec<_>>()
            .join(" or ");
        clauses.push(format!("({ips})"));
    }
    if !udp.dst_ips.is_empty() {
        let ips = udp
            .dst_ips
            .iter()
            .map(|ip| format!("dst host {ip}"))
            .collect::<Vec<_>>()
            .join(" or ");
        clauses.push(format!("({ips})"));
    }
    if clauses.is_empty() {
        None
    } else {
        Some(format!("udp and {}", clauses.join(" and ")))
    }
}

fn build_udp_packet_meta(
    interface: &str,
    parsed: &ParsedUdpPacket<'_>,
    payload_cfg: &PayloadConfig,
) -> serde_json::Value {
    let max_preview = payload_cfg.max_preview_bytes as usize;
    let preview_len = parsed.payload.len().min(max_preview);
    let payload_hex_preview = if payload_cfg.include_preview {
        hex_preview(&parsed.payload[..preview_len])
    } else {
        String::new()
    };
    json!({
        "packet_interface": interface,
        "src_ip": parsed.src_ip.to_string(),
        "dst_ip": parsed.dst_ip.to_string(),
        "src_port": parsed.src_port,
        "dst_port": parsed.dst_port,
        "payload_len": parsed.payload_len,
        "payload_preview_len": if payload_cfg.include_preview { preview_len } else { 0 },
        "payload_hex_preview": payload_hex_preview,
        "payload_truncated": payload_cfg.include_preview && preview_len < parsed.payload_len,
    })
}

fn format_udp_packet_line(
    interface: &str,
    parsed: &ParsedUdpPacket<'_>,
    payload_cfg: &PayloadConfig,
) -> String {
    let mut out = format!(
        "udp if={} src={}:{} dst={}:{} len={}",
        interface,
        parsed.src_ip,
        parsed.src_port,
        parsed.dst_ip,
        parsed.dst_port,
        parsed.payload_len,
    );
    if payload_cfg.include_preview {
        let max_preview = payload_cfg.max_preview_bytes as usize;
        let preview_len = parsed.payload.len().min(max_preview);
        let preview = hex_preview(&parsed.payload[..preview_len]);
        if preview.is_empty() {
            out.push_str(" payload=");
        } else {
            out.push_str(" payload=");
            out.push_str(&preview);
        }
        if preview_len < parsed.payload_len {
            out.push_str(" truncated");
        }
    }
    out
}

fn hex_preview(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len().saturating_mul(3));
    for (idx, byte) in bytes.iter().enumerate() {
        if idx > 0 {
            out.push(' ');
        }
        out.push_str(&format!("{:02X}", byte));
    }
    out
}

fn parse_udp_packet(linktype: i32, data: &[u8]) -> Option<ParsedUdpPacket<'_>> {
    match linktype {
        DLT_EN10MB => parse_udp_from_ethernet(data),
        DLT_NULL => parse_udp_from_null_loopback(data),
        DLT_RAW | DLT_IPV4 | DLT_IPV6 => parse_udp_from_ip(data),
        DLT_LINUX_SLL => parse_udp_from_linux_sll(data),
        _ => None,
    }
}

fn parse_udp_from_ethernet(data: &[u8]) -> Option<ParsedUdpPacket<'_>> {
    if data.len() < 14 {
        return None;
    }
    let mut offset = 14usize;
    let mut ether_type = u16::from_be_bytes([data[12], data[13]]);
    while matches!(ether_type, 0x8100 | 0x88A8 | 0x9100) {
        if data.len() < offset + 4 {
            return None;
        }
        ether_type = u16::from_be_bytes([data[offset + 2], data[offset + 3]]);
        offset += 4;
    }
    match ether_type {
        0x0800 | 0x86DD => parse_udp_from_ip(&data[offset..]),
        _ => None,
    }
}

fn parse_udp_from_linux_sll(data: &[u8]) -> Option<ParsedUdpPacket<'_>> {
    if data.len() < 16 {
        return None;
    }
    let protocol = u16::from_be_bytes([data[14], data[15]]);
    match protocol {
        0x0800 | 0x86DD => parse_udp_from_ip(&data[16..]),
        _ => None,
    }
}

fn parse_udp_from_null_loopback(data: &[u8]) -> Option<ParsedUdpPacket<'_>> {
    if data.len() < 4 {
        return None;
    }
    let family_le = u32::from_le_bytes(data[..4].try_into().ok()?);
    let family_be = u32::from_be_bytes(data[..4].try_into().ok()?);
    let family = if matches!(family_le, 2 | 24 | 28 | 30) {
        family_le
    } else {
        family_be
    };
    match family {
        2 | 24 | 28 | 30 => parse_udp_from_ip(&data[4..]),
        _ => None,
    }
}

fn parse_udp_from_ip(data: &[u8]) -> Option<ParsedUdpPacket<'_>> {
    match data.first().map(|b| b >> 4) {
        Some(4) => parse_udp_from_ipv4(data),
        Some(6) => parse_udp_from_ipv6(data),
        _ => None,
    }
}

fn parse_udp_from_ipv4(data: &[u8]) -> Option<ParsedUdpPacket<'_>> {
    if data.len() < 20 {
        return None;
    }
    let ihl = ((data[0] & 0x0f) as usize).checked_mul(4)?;
    if ihl < 20 || data.len() < ihl + 8 {
        return None;
    }
    let fragment = u16::from_be_bytes([data[6], data[7]]);
    let fragment_offset = fragment & 0x1fff;
    if fragment_offset != 0 {
        return None;
    }
    if data[9] != 17 {
        return None;
    }
    let src_ip = IpAddr::V4(Ipv4Addr::new(data[12], data[13], data[14], data[15]));
    let dst_ip = IpAddr::V4(Ipv4Addr::new(data[16], data[17], data[18], data[19]));
    parse_udp_header(src_ip, dst_ip, &data[ihl..])
}

fn parse_udp_from_ipv6(data: &[u8]) -> Option<ParsedUdpPacket<'_>> {
    if data.len() < 40 {
        return None;
    }
    let src_ip = IpAddr::V6(Ipv6Addr::from(<[u8; 16]>::try_from(&data[8..24]).ok()?));
    let dst_ip = IpAddr::V6(Ipv6Addr::from(<[u8; 16]>::try_from(&data[24..40]).ok()?));
    let mut next_header = data[6];
    let mut offset = 40usize;
    loop {
        match next_header {
            17 => return parse_udp_header(src_ip, dst_ip, &data[offset..]),
            0 | 43 | 60 => {
                if data.len() < offset + 8 {
                    return None;
                }
                let hdr_len = (usize::from(data[offset + 1]) + 1).checked_mul(8)?;
                next_header = data[offset];
                offset = offset.checked_add(hdr_len)?;
            }
            44 => {
                if data.len() < offset + 8 {
                    return None;
                }
                let frag = u16::from_be_bytes([data[offset + 2], data[offset + 3]]);
                if (frag >> 3) != 0 {
                    return None;
                }
                next_header = data[offset];
                offset = offset.checked_add(8)?;
            }
            _ => return None,
        }
        if offset > data.len() {
            return None;
        }
    }
}

fn parse_udp_header<'a>(
    src_ip: IpAddr,
    dst_ip: IpAddr,
    data: &'a [u8],
) -> Option<ParsedUdpPacket<'a>> {
    if data.len() < 8 {
        return None;
    }
    let src_port = u16::from_be_bytes([data[0], data[1]]);
    let dst_port = u16::from_be_bytes([data[2], data[3]]);
    let udp_len = usize::from(u16::from_be_bytes([data[4], data[5]]));
    if udp_len < 8 {
        return None;
    }
    let payload_len = udp_len.saturating_sub(8);
    let available_payload = &data[8..];
    let captured_payload_len = available_payload.len().min(payload_len);
    Some(ParsedUdpPacket {
        src_ip,
        dst_ip,
        src_port,
        dst_port,
        payload: &available_payload[..captured_payload_len],
        payload_len,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::time::{timeout, Duration};

    #[tokio::test]
    async fn mock_backend_emits_deterministic_network_events() {
        let (tx, mut rx) = mpsc::channel(2);
        let source = NetworkCaptureSource::new("net", "lo0", "udp", "mock", Some(0.001), None, None, None, None);
        let handle = tokio::spawn(async move {
            let _ = Box::new(source).run(tx).await;
        });

        let entry = timeout(Duration::from_secs(2), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(entry.source, "net");
        assert!(entry.message.contains("interface=lo0"));
        assert!(entry.message.contains("backend=mock"));
        assert!(entry.message.contains("filter=udp"));

        handle.abort();
    }

    #[test]
    fn builds_udp_filter_from_structured_config() {
        let filter = build_udp_capture_filter(&NetworkUdpCaptureConfig {
            ports: vec![8333, 5683, 5684],
            host: Some("192.168.1.10".into()),
            src_ips: vec!["192.168.1.20".into()],
            dst_ips: vec!["224.0.1.187".into()],
        })
        .unwrap();
        assert_eq!(
            filter,
            "udp and (port 8333 or port 5683 or port 5684) and host 192.168.1.10 and (src host 192.168.1.20) and (dst host 224.0.1.187)"
        );
    }

    #[test]
    fn udp_packet_meta_contains_ports_and_payload_preview() {
        let payload_cfg = PayloadConfig {
            include_preview: true,
            max_preview_bytes: 4,
        };
        let parsed = ParsedUdpPacket {
            src_ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20)),
            dst_ip: IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
            src_port: 49152,
            dst_port: 5683,
            payload: &[0x40, 0x01, 0x12, 0x34, 0xB3],
            payload_len: 5,
        };
        let meta = build_udp_packet_meta("lo", &parsed, &payload_cfg);
        assert_eq!(meta["packet_interface"], "lo");
        assert_eq!(meta["src_port"], 49152);
        assert_eq!(meta["dst_port"], 5683);
        assert_eq!(meta["payload_len"], 5);
        assert_eq!(meta["payload_preview_len"], 4);
        assert_eq!(meta["payload_hex_preview"], "40 01 12 34");
        assert_eq!(meta["payload_truncated"], true);
    }

    #[test]
    fn parses_ipv4_udp_payload() {
        let frame = [
            0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x08, 0x00, // ethernet
            0x45, 0x00, 0x00, 0x28, 0, 0, 0, 0, 64, 17, 0, 0, 192, 168, 1, 20, 192, 168, 1, 10,
            0xC0, 0x00, 0x16, 0x33, 0x00, 0x14, 0, 0, // udp
            0x40, 0x01, 0x12, 0x34, 0xB3, 0x66, 0x6F, 0x6F, 0x03, 0x62, 0x61, 0x72,
        ];
        let parsed = parse_udp_packet(DLT_EN10MB, &frame).unwrap();
        assert_eq!(parsed.src_ip, IpAddr::V4(Ipv4Addr::new(192, 168, 1, 20)));
        assert_eq!(parsed.dst_ip, IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)));
        assert_eq!(parsed.src_port, 49152);
        assert_eq!(parsed.dst_port, 5683);
        assert_eq!(parsed.payload_len, 12);
        assert_eq!(hex_preview(parsed.payload), "40 01 12 34 B3 66 6F 6F 03 62 61 72");
    }
}
