//! Embedded demo configuration shared by `embed-log demo` and written as the
//! template for `embed-log init`.

/// Demo config: UDP sources for DUT/HOST/UART/CoAP/Sensors, a mock network
/// capture source, a file watcher, and the six tabs that lay them out.
pub(crate) const DEMO_CONFIG: &str = r#"version: 1
server:
  host: 127.0.0.1
  ws_port: 8080
  app_name: embed-log demo
  timestamp_mode: absolute

logs:
  dir: logs/

baudrate: 115200

frontend_plugins:
  hex-coap:
    builtin: hex-coap

sources:
  - name: DUT
    label: DUT Device
    type: udp
    port: 6000

  - name: HOST
    label: Host Debug
    type: udp
    port: 6001

  - name: UART_DUT
    label: UART Main
    type: udp
    port: 6100

  - name: UART_DEBUG
    label: UART Debug
    type: udp
    port: 6101

  - name: COAP_RAW
    label: CoAP Raw Hex
    type: udp
    port: 6005

  - name: SENSORS
    label: Sensor CBOR
    type: udp
    port: 6002
    parser:
      type: cbor-datagram

  - name: NET_CAPTURE
    label: Network Mock
    type: network_capture
    network_backend: mock
    interface: mock0
    mock_interval: 1.0
    bpf_filter: udp or coap

  - name: FILE_WATCH
    label: Watched File
    type: file
    port: .tmp/demo-watch.log

tabs:
  - label: Device
    panes:
      - DUT
      - HOST

  - label: UART
    panes:
      - UART_DUT
      - UART_DEBUG

  - label: CoAP
    panes:
      - source: COAP_RAW
        plugins: [hex-coap]

  - label: Sensors
    panes:
      - SENSORS

  - label: Network
    panes:
      - NET_CAPTURE

  - label: File
    panes:
      - FILE_WATCH
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn demo_config_parses_as_valid_yaml() {
        // Catches accidental corruption of the embedded demo template.
        let parsed: serde_yaml::Value =
            serde_yaml::from_str(DEMO_CONFIG).expect("DEMO_CONFIG must be valid YAML");
        let server = parsed
            .get("server")
            .expect("demo config has a server section");
        assert_eq!(server["ws_port"], 8080);
        assert!(parsed["sources"].is_sequence());
        assert!(parsed["tabs"].is_sequence());
    }

    #[test]
    fn demo_config_every_tab_pane_references_a_known_source() {
        let parsed: serde_yaml::Value = serde_yaml::from_str(DEMO_CONFIG).unwrap();
        let sources: std::collections::HashSet<String> = parsed["sources"]
            .as_sequence()
            .unwrap()
            .iter()
            .map(|s| s["name"].as_str().unwrap().to_string())
            .collect();
        for tab in parsed["tabs"].as_sequence().unwrap() {
            for pane in tab["panes"].as_sequence().unwrap() {
                // Pane is either a bare source-name string or a { source: NAME } object.
                let name = pane
                    .as_str()
                    .map(str::to_owned)
                    .or_else(|| {
                        pane.get("source")
                            .and_then(|s| s.as_str())
                            .map(str::to_owned)
                    })
                    .unwrap();
                assert!(
                    sources.contains(&name),
                    "tab `{}` references unknown source `{name}`",
                    tab["label"].as_str().unwrap()
                );
            }
        }
    }
}
