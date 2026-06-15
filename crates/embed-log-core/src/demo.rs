use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use tokio::net::UdpSocket;

use crate::config::AppConfig;

pub fn prepare_demo_file_sources(config: &AppConfig) -> Result<()> {
    for source in config
        .sources
        .iter()
        .filter(|source| source.source_type.eq_ignore_ascii_case("file"))
    {
        let Some(path) = source.port.as_str().map(Path::new) else {
            continue;
        };
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent)
                .with_context(|| format!("create demo file source dir {}", parent.display()))?;
        }
        if !path.exists() {
            std::fs::File::create(path)
                .with_context(|| format!("create demo file source {}", path.display()))?;
        }
    }
    Ok(())
}

pub fn spawn_demo_traffic(config: &AppConfig) {
    let udp_sources: Vec<DemoUdpSource> = config
        .sources
        .iter()
        .filter(|source| source.source_type.eq_ignore_ascii_case("udp"))
        .filter_map(|source| {
            let port = source
                .port
                .as_i64()
                .and_then(|port| u16::try_from(port).ok())?;
            Some(DemoUdpSource {
                name: source.name.clone(),
                port,
                role: DemoSourceRole::from_source(&source.name, &source.parser.parser_type),
            })
        })
        .collect();

    let file_sources: Vec<DemoFileSource> = config
        .sources
        .iter()
        .filter(|source| source.source_type.eq_ignore_ascii_case("file"))
        .filter_map(|source| {
            Some(DemoFileSource {
                name: source.name.clone(),
                path: PathBuf::from(source.port.as_str()?),
            })
        })
        .collect();

    if !udp_sources.is_empty() {
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(750)).await;

            let socket = match UdpSocket::bind(("127.0.0.1", 0)).await {
                Ok(socket) => socket,
                Err(error) => {
                    tracing::warn!("demo UDP traffic disabled: {error}");
                    return;
                }
            };

            let mut tick: u64 = 0;
            let mut interval = tokio::time::interval(Duration::from_millis(400));

            loop {
                interval.tick().await;
                tick += 1;

                for source in &udp_sources {
                    let payload = demo_payload_for_source(source, tick);
                    if let Err(error) = socket.send_to(&payload, ("127.0.0.1", source.port)).await {
                        tracing::warn!(
                            "failed to send demo traffic to {} on UDP {}: {error}",
                            source.name,
                            source.port
                        );
                    }
                }
            }
        });
    }

    if !file_sources.is_empty() {
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(1000)).await;

            let mut tick: u64 = 0;
            let mut interval = tokio::time::interval(Duration::from_millis(750));

            loop {
                interval.tick().await;
                tick += 1;

                for source in &file_sources {
                    let line = demo_file_line(source, tick);
                    if let Err(error) = append_demo_file_line(&source.path, &line) {
                        tracing::warn!(
                            "failed to append demo file traffic to {}: {error}",
                            source.path.display()
                        );
                    }
                }
            }
        });
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DemoSourceRole {
    Device,
    Host,
    UartMain,
    UartDebug,
    CoapRaw,
    CborSensor,
    Generic,
}

impl DemoSourceRole {
    pub fn from_source(name: &str, parser_type: &str) -> Self {
        if parser_type == "cbor-datagram" {
            return Self::CborSensor;
        }

        let name = name.to_ascii_uppercase();
        if name.contains("COAP") {
            Self::CoapRaw
        } else if name.contains("UART_DEBUG") || name.ends_with("_DEBUG") {
            Self::UartDebug
        } else if name.contains("UART") {
            Self::UartMain
        } else if name.contains("HOST") {
            Self::Host
        } else if name.contains("DUT") || name.contains("DEVICE") {
            Self::Device
        } else {
            Self::Generic
        }
    }
}

pub struct DemoUdpSource {
    pub name: String,
    pub port: u16,
    pub role: DemoSourceRole,
}

pub struct DemoFileSource {
    pub name: String,
    pub path: PathBuf,
}

pub fn demo_payload_for_source(source: &DemoUdpSource, tick: u64) -> Vec<u8> {
    if source.role == DemoSourceRole::CborSensor {
        return demo_cbor_sensor_payload(tick);
    }

    demo_line_for_source(source, tick).into_bytes()
}

fn demo_cbor_sensor_payload(tick: u64) -> Vec<u8> {
    let mut payload = Vec::with_capacity(96);
    payload.push(0xa6);
    push_cbor_text(&mut payload, "seq");
    push_cbor_uint(&mut payload, tick);
    push_cbor_text(&mut payload, "temp_centi_c");
    push_cbor_uint(&mut payload, 2_100 + (tick * 37) % 1_200);
    push_cbor_text(&mut payload, "hum_tenth_pct");
    push_cbor_uint(&mut payload, 420 + (tick * 19) % 380);
    push_cbor_text(&mut payload, "press_pa");
    push_cbor_uint(&mut payload, 100_900 + (tick * 23) % 900);
    push_cbor_text(&mut payload, "batt_mv");
    push_cbor_uint(&mut payload, 3_700 + (tick * 11) % 400);
    push_cbor_text(&mut payload, "state");
    push_cbor_text(
        &mut payload,
        demo_pick(&["idle", "sampling", "publish"], tick),
    );
    payload
}

fn push_cbor_text(out: &mut Vec<u8>, value: &str) {
    let bytes = value.as_bytes();
    if bytes.len() < 24 {
        out.push(0x60 | bytes.len() as u8);
    } else if let Ok(len) = u8::try_from(bytes.len()) {
        out.extend_from_slice(&[0x78, len]);
    } else if let Ok(len) = u16::try_from(bytes.len()) {
        out.push(0x79);
        out.extend_from_slice(&len.to_be_bytes());
    } else {
        return;
    }
    out.extend_from_slice(bytes);
}

fn push_cbor_uint(out: &mut Vec<u8>, value: u64) {
    if value < 24 {
        out.push(value as u8);
    } else if let Ok(value) = u8::try_from(value) {
        out.extend_from_slice(&[0x18, value]);
    } else if let Ok(value) = u16::try_from(value) {
        out.push(0x19);
        out.extend_from_slice(&value.to_be_bytes());
    } else if let Ok(value) = u32::try_from(value) {
        out.push(0x1a);
        out.extend_from_slice(&value.to_be_bytes());
    } else {
        out.push(0x1b);
        out.extend_from_slice(&value.to_be_bytes());
    }
}

pub fn demo_line_for_source(source: &DemoUdpSource, tick: u64) -> String {
    match source.role {
        DemoSourceRole::Device => demo_device_line(&source.name, tick),
        DemoSourceRole::Host => demo_host_line(&source.name, tick),
        DemoSourceRole::UartMain => demo_uart_main_line(&source.name, tick),
        DemoSourceRole::UartDebug => demo_uart_debug_line(&source.name, tick),
        DemoSourceRole::CoapRaw => demo_coap_raw_line(&source.name, tick),
        DemoSourceRole::Generic => format!(
            "[{}] [INFO] [{}] demo log line {tick}",
            demo_hms(tick),
            source.name
        ),
        DemoSourceRole::CborSensor => unreachable!("CBOR sources are encoded as datagrams"),
    }
}

fn demo_device_line(source: &str, tick: u64) -> String {
    let message = match tick % 8 {
        0 => format!(
            "boot complete, firmware v2.1.{} uptime={}s",
            tick % 10,
            tick * 2
        ),
        1 => format!(
            "sensor reading: temperature={}.{}C humidity={}%",
            21 + tick % 9,
            (tick * 7) % 10,
            45 + tick % 30
        ),
        2 => format!(
            "WiFi connected, RSSI=-{}dBm ip=192.168.1.{}",
            35 + tick % 45,
            100 + tick % 80
        ),
        3 => format!(
            "SPI flash: read {} bytes at 0x{:06X}",
            128 + tick * 17 % 4096,
            tick * 4099 % 0xFF_FFFF
        ),
        4 => format!(
            "heap: free={}KB allocated={}KB fragmentation={}%",
            180 - tick % 70,
            40 + tick % 90,
            tick % 27
        ),
        5 => format!(
            "MQTT publish topic=/sensors/temp payload={{\"t\":{}}}",
            20 + tick % 15
        ),
        6 => format!("GPIO interrupt on pin {}, edge=rising", tick % 40),
        _ => "watchdog fed, system healthy".to_string(),
    };
    format!("[{}] [INFO] [{source}] {message}", demo_hms(tick))
}

fn demo_host_line(source: &str, tick: u64) -> String {
    let message = match tick % 6 {
        0 => format!(
            "pytest: test_boot_sequence PASSED ({}.{:02}s)",
            tick % 8,
            tick % 100
        ),
        1 => format!("ci: build #{} started (branch: main)", 1000 + tick),
        2 => format!("dut: heartbeat OK (latency={}ms)", 5 + tick % 90),
        3 => format!(
            "dut: firmware flash complete ({}kB in {}.{}s)",
            256 + tick % 512,
            2 + tick % 6,
            tick % 10
        ),
        4 => format!("ci: step {}/8 running — test", 1 + tick % 8),
        _ => format!("log: session rotated — {} lines captured", 500 + tick * 3),
    };
    format!("[{}] [HOST] [{source}] {message}", demo_hms(tick))
}

fn demo_uart_main_line(source: &str, tick: u64) -> String {
    let message = match tick % 5 {
        0 => "ROM bootloader banner: chip=esp32 reset=power-on".to_string(),
        1 => format!("uart rx command id={} len={} crc=ok", tick, 12 + tick % 48),
        2 => format!(
            "task telemetry stack={} watermark={} heap={}KB",
            4096 - tick % 512,
            1024 - tick % 128,
            128 + tick % 64
        ),
        3 => format!(
            "adc sample ch={} raw={} mv={}",
            tick % 4,
            1800 + tick % 900,
            900 + tick % 500
        ),
        _ => "main loop heartbeat".to_string(),
    };
    format!("[{}] [UART] [{source}] {message}", demo_hms(tick))
}

fn demo_uart_debug_line(source: &str, tick: u64) -> String {
    let level = demo_pick(&["DEBUG", "TRACE", "WARN"], tick);
    let message = match tick % 5 {
        0 => format!("scheduler tick={} ready_tasks={}", tick, 2 + tick % 6),
        1 => format!(
            "i2c transaction addr=0x{:02X} bytes={} status=ok",
            0x40 + tick % 32,
            2 + tick % 16
        ),
        2 => format!(
            "ringbuf fill={} high_water={}",
            tick % 256,
            128 + tick % 128
        ),
        3 => format!("radio irq flags=0x{:04X}", tick * 37 % 0xffff),
        _ => "verbose trace checkpoint reached".to_string(),
    };
    format!("[{}] [{level}] [{source}] {message}", demo_hms(tick))
}

fn demo_coap_raw_line(source: &str, tick: u64) -> String {
    const COAP_HEX_SAMPLES: [&str; 4] = [
        "45 01 00 01 11 22 33 44 b3 74 65 6d 70 ff 19 01 00",
        "65 45 00 01 11 22 33 44 ff 48 65 6c 6c 6f",
        "42 01 00 01 11 22",
        "40 01 12 34 b3 66 6f 6f 03 62 61 72",
    ];
    let sample = demo_pick(&COAP_HEX_SAMPLES, tick);
    let direction = if tick % 2 == 0 {
        "recv ACK"
    } else {
        "send CON"
    };
    format!(
        "[{}] [INFO] [{source}] coap {direction} mid=0x{:04X} code=2.05 payload=hex:{sample}",
        demo_hms(tick),
        0x1200 + (tick % 0x0fff)
    )
}

pub fn demo_file_line(source: &DemoFileSource, tick: u64) -> String {
    format!(
        "[{}] [FILE] [{}] appended watched-file event tick={} path={}",
        demo_hms(tick),
        source.name,
        tick,
        source.path.display()
    )
}

fn append_demo_file_line(path: &Path, line: &str) -> Result<()> {
    use std::io::Write;

    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .with_context(|| format!("open demo file source {}", path.display()))?;
    writeln!(file, "{line}").with_context(|| format!("append demo file source {}", path.display()))
}

fn demo_hms(tick: u64) -> String {
    let seconds = tick % 86_400;
    format!(
        "{:02}:{:02}:{:02}.{:03}",
        seconds / 3_600,
        (seconds / 60) % 60,
        seconds % 60,
        (tick * 137) % 1000
    )
}

fn demo_pick<'a>(values: &'a [&str], tick: u64) -> &'a str {
    values[(tick as usize - 1) % values.len()]
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parsers::cbor::CborDatagramParser;
    use crate::parsers::traits::StreamParser;

    #[test]
    fn demo_role_classification_uses_name_and_parser() {
        assert_eq!(
            DemoSourceRole::from_source("COAP_RAW", "text"),
            DemoSourceRole::CoapRaw
        );
        assert_eq!(
            DemoSourceRole::from_source("UART_DEBUG", "text"),
            DemoSourceRole::UartDebug
        );
        assert_eq!(
            DemoSourceRole::from_source("SENSORS", "cbor-datagram"),
            DemoSourceRole::CborSensor
        );
    }

    #[test]
    fn demo_device_line_is_embedded_log_style() {
        let source = DemoUdpSource {
            name: "DUT".to_string(),
            port: 6000,
            role: DemoSourceRole::Device,
        };

        let line = demo_line_for_source(&source, 7);
        assert!(line.contains("[DUT]"));
        assert!(line.contains("watchdog fed"));
    }

    #[test]
    fn demo_coap_line_contains_raw_hex_for_plugin() {
        let source = DemoUdpSource {
            name: "COAP_RAW".to_string(),
            port: 6005,
            role: DemoSourceRole::CoapRaw,
        };

        let line = demo_line_for_source(&source, 1);
        assert!(line.contains("payload=hex:"));
        assert!(line.contains("45 01 00 01 11 22 33 44"));
    }

    #[test]
    fn demo_payload_for_cbor_source_decodes_as_sensor_map() {
        let source = DemoUdpSource {
            name: "SENSORS".to_string(),
            port: 6002,
            role: DemoSourceRole::CborSensor,
        };
        let mut parser = CborDatagramParser::new();

        let lines = parser.feed(&demo_payload_for_source(&source, 1));
        assert_eq!(lines.len(), 1);
        assert!(lines[0].contains("seq=1"));
        assert!(lines[0].contains("temp_centi_c="));
        assert!(lines[0].contains("batt_mv="));
    }

    #[test]
    fn demo_file_line_identifies_watched_source() {
        let source = DemoFileSource {
            name: "FILE_WATCH".to_string(),
            path: PathBuf::from(".tmp/demo-watch.log"),
        };

        let line = demo_file_line(&source, 3);
        assert!(line.contains("[FILE_WATCH]"));
        assert!(line.contains("watched-file event tick=3"));
    }
}
