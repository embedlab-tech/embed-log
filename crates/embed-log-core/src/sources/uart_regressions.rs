//! Exercise independent async UART sources through pseudo-terminals.
use super::*;
use serialport::SerialPort;
use std::io::Write;
use tokio::time::timeout;

fn normal_record() -> Vec<u8> {
    let mut wire = vec![0, 0x30, 8, 0, 0, 0];
    wire.extend(1u32.to_le_bytes());
    wire.extend(42u32.to_le_bytes());
    wire.extend([2, 0, 0, 0]);
    wire.extend(0x1000u32.to_le_bytes());
    wire
}

#[tokio::test]
async fn incomplete_source_does_not_block_peer_or_boot_output() {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("embed-log-pty-dictionary-{unique}.json"));
    std::fs::write(&path, r#"{"version":3,"target":{"bits":32,"little_endianness":true},"arch":"arm","log_subsys":{"log_instances":{"1":{"name":"app"}}},"string_mappings":{"4096":"alive"}}"#).unwrap();
    let (mut master_a, mut port_a) = serialport::TTYPort::pair().unwrap();
    let (mut master_b, mut port_b) = serialport::TTYPort::pair().unwrap();
    port_a.set_timeout(Duration::from_millis(100)).unwrap();
    port_b.set_timeout(Duration::from_millis(100)).unwrap();
    let (tx, mut rx) = mpsc::channel::<LogEntry>(1024);
    let source_a = UartSource::from_port("device-a", Box::new(port_a), "zephyr-dict")
        .with_parser_database(Some(path.to_string_lossy().into_owned()));
    let source_b = UartSource::from_port("device-b", Box::new(port_b), "zephyr-dict")
        .with_parser_database(Some(path.to_string_lossy().into_owned()));
    let task_a = tokio::spawn(Box::new(source_a).run(tx.clone()));
    let task_b = tokio::spawn(Box::new(source_b).run(tx));
    let good = normal_record();
    // A plausible stale header advertises 65535 missing hexdump bytes.
    let mut stale = good.clone();
    stale[4..6].copy_from_slice(&65535u16.to_le_bytes());
    master_a.write_all(&stale).unwrap();
    master_b.write_all(&good).unwrap();
    let first = timeout(Duration::from_secs(2), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.source, "device-b");
    assert!(first.message.contains("app: alive"));
    master_a
        .write_all(b"*** Booting firmware ***\r\nBOOT COMPLETE\r\n")
        .unwrap();
    let waiting_since = tokio::time::Instant::now();
    let mut abandonments = 0;
    timeout(Duration::from_secs(7), async {
        loop {
            let entry = rx.recv().await.expect("UART tasks stopped");
            if entry.message.contains("abandoned after 5 s idle") {
                abandonments += 1;
            }
            if entry.source == "device-a" && entry.message.contains("BOOT COMPLETE") {
                break;
            }
        }
    })
    .await
    .expect("boot banner trapped behind stale BIN length");
    assert!(waiting_since.elapsed() >= Duration::from_secs(5));
    assert_eq!(abandonments, 1);
    // Drain a burst, then require fresh output from both independent sources.
    const BURST_SIZE: usize = 128;
    for _ in 0..BURST_SIZE {
        master_a.write_all(&good).unwrap();
    }
    let burst = timeout(Duration::from_secs(3), async {
        let mut count = 0;
        while count < BURST_SIZE {
            let entry = rx.recv().await.unwrap();
            if entry.source == "device-a" && entry.message.contains("app: alive") {
                count += 1;
            }
        }
        count
    })
    .await
    .expect("burst stalled");
    assert_eq!(burst, BURST_SIZE);
    master_b.write_all(&good).unwrap();
    master_a.write_all(&good).unwrap();
    let mut sources = std::collections::HashSet::new();
    timeout(Duration::from_secs(2), async {
        while sources.len() < 2 {
            let entry = rx.recv().await.unwrap();
            if entry.message.contains("app: alive") {
                sources.insert(entry.source);
            }
        }
    })
    .await
    .expect("one source stopped after the burst");
    task_a.abort();
    task_b.abort();
    let _ = task_a.await;
    let _ = task_b.await;
    drop((master_a, master_b));
    std::fs::remove_file(path).unwrap();
}

fn test_dictionary(label: &str) -> std::path::PathBuf {
    let unique = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let path = std::env::temp_dir().join(format!("embed-log-{label}-{unique}.json"));
    std::fs::write(&path, r#"{"version":3,"target":{"bits":32,"little_endianness":true},"arch":"arm","log_subsys":{"log_instances":{"1":{"name":"app"}}},"string_mappings":{"4096":"alive"}}"#).unwrap();
    path
}

async fn expect_alive(rx: &mut mpsc::Receiver<LogEntry>) {
    let entry = timeout(Duration::from_secs(2), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(entry.message.contains("app: alive"), "{}", entry.message);
}

#[tokio::test]
async fn uart_soft_idle_keeps_header_and_activity_restarts_hard_deadline() {
    let path = test_dictionary("idle");
    let (mut master, mut slave) = serialport::TTYPort::pair().unwrap();
    slave.set_timeout(Duration::from_millis(100)).unwrap();
    let (tx, mut rx) = mpsc::channel(64);
    let source = UartSource::from_port("dut", Box::new(slave), "zephyr-dict")
        .with_parser_database(Some(path.to_string_lossy().into_owned()));
    let task = tokio::spawn(Box::new(source).run(tx));
    let good = normal_record();
    master.write_all(&good).unwrap();
    expect_alive(&mut rx).await;
    master.write_all(&good[..14]).unwrap();
    tokio::time::sleep(Duration::from_millis(1300)).await;
    assert!(rx.try_recv().is_err(), "soft idle destroyed a valid header");
    master.write_all(&good[14..]).unwrap();
    expect_alive(&mut rx).await;
    // A second partial read resets elapsed silence, even after several idle ticks.
    master.write_all(&good[..14]).unwrap();
    tokio::time::sleep(Duration::from_millis(3500)).await;
    master.write_all(&good[14..18]).unwrap();
    tokio::time::sleep(Duration::from_millis(1800)).await;
    assert!(
        rx.try_recv().is_err(),
        "deadline was not reset by received bytes"
    );
    master.write_all(&good[18..]).unwrap();
    master.write_all(&[1, 69, 0]).unwrap();
    expect_alive(&mut rx).await;
    let drop = timeout(Duration::from_secs(2), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(drop.message, "--- 69 messages dropped ---");
    task.abort();
    let _ = task.await;
    std::fs::remove_file(path).unwrap();
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn uart_reconnect_recreates_parser_and_accepts_delayed_record() {
    let path = test_dictionary("reconnect");
    let link = path.with_extension("tty");
    let (mut old_master, old_slave) = serialport::TTYPort::pair().unwrap();
    let old_name = old_slave.name().unwrap();
    std::os::unix::fs::symlink(old_name, &link).unwrap();
    drop(old_slave);
    let (tx, mut rx) = mpsc::channel(64);
    let source = UartSource::new_with_parser("dut", link.to_string_lossy(), 115200, "zephyr-dict")
        .with_parser_database(Some(path.to_string_lossy().into_owned()));
    let task = tokio::spawn(Box::new(source).run(tx));
    tokio::time::sleep(Duration::from_millis(300)).await;
    let good = normal_record();
    old_master.write_all(&good).unwrap();
    expect_alive(&mut rx).await;
    old_master.write_all(&good[..14]).unwrap();
    tokio::time::sleep(Duration::from_millis(200)).await;
    let (mut master, slave) = serialport::TTYPort::pair().unwrap();
    std::fs::remove_file(&link).unwrap();
    std::os::unix::fs::symlink(slave.name().unwrap(), &link).unwrap();
    drop(slave);
    drop(old_master);
    tokio::time::sleep(Duration::from_millis(1500)).await;
    // No old pending header or old deadline may leak into the new parser.
    master.write_all(&good).unwrap();
    expect_alive(&mut rx).await;
    master.write_all(&good[..14]).unwrap();
    tokio::time::sleep(Duration::from_millis(1300)).await;
    assert!(rx.try_recv().is_err());
    master.write_all(&good[14..]).unwrap();
    master.write_all(&[1, 69, 0]).unwrap();
    expect_alive(&mut rx).await;
    let drop = timeout(Duration::from_secs(2), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(drop.message, "--- 69 messages dropped ---");
    task.abort();
    let _ = task.await;
    std::fs::remove_file(link).unwrap();
    std::fs::remove_file(path).unwrap();
}
