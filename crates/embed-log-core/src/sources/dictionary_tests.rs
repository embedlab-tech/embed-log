use super::*;
use crate::models::LogEntry;
use crate::parsers::create_parser;
use crate::sources::traits::LogSource;
use crate::sources::uart::UartSource;
use serialport::SerialPort;
use tokio::time::timeout;

fn session(dir: PathBuf) -> SessionManager {
    SessionManager::new(
        "test",
        dir,
        &[],
        HashMap::new(),
        "combined.jsonl",
        HashMap::new(),
        HashMap::new(),
        serde_json::json!({}),
        serde_json::json!({}),
        serde_json::json!({}),
        serde_json::json!({}),
        "now",
        "test",
        None,
        None,
        "hms",
        None,
        serde_json::json!([]),
    )
}

fn setup() -> (PathBuf, Arc<Mutex<SessionManager>>) {
    static NEXT: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
    let path = std::env::temp_dir().join(format!(
        "embed-log-runtime-dictionary-{}-{}-{}",
        std::process::id(),
        chrono::Local::now().timestamp_nanos_opt().unwrap(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    (path.clone(), Arc::new(Mutex::new(session(path))))
}

fn dictionary(message: &str) -> String {
    serde_json::json!({"version":3,"build_id": message,"target":{"bits":32,"little_endianness":true},"arch":"arm","log_subsys":{"log_instances":{"1":{"name":"app"}}},"string_mappings":{"4096":message}}).to_string()
}

fn record() -> Vec<u8> {
    let mut bytes = vec![0, 0x30, 8, 0, 0, 0];
    bytes.extend(1u32.to_le_bytes());
    bytes.extend(42u32.to_le_bytes());
    bytes.extend([2, 0, 0, 0]);
    bytes.extend(0x1000u32.to_le_bytes());
    bytes
}

#[test]
fn staging_validates_and_preserves_exact_artifact_bytes() {
    let (dir, manager) = setup();
    let (control, _) =
        DictionaryControl::new("../UART_A", &ParserConfig::default(), manager).unwrap();
    for invalid in [
        "not JSON",
        r#"{"version":1}"#,
        r#"{"version":3}"#,
        r#"{"version":3,"target":{"bits":17},"string_mappings":{"4096":"hello"}}"#,
    ] {
        assert!(control.stage(invalid).is_err(), "{invalid}");
    }
    assert!(control
        .stage(&" ".repeat(MAX_DICTIONARY_BYTES + 1))
        .is_err());
    let text = format!("  {}\n", dictionary("image-A"));
    let info = control.stage(&text).unwrap();
    assert_eq!(
        info.dictionary_id,
        format!("{:x}", Sha256::digest(text.as_bytes()))
    );
    assert_eq!(
        std::fs::read(
            dir.join("dictionaries")
                .join(format!("{}.json", info.dictionary_id))
        )
        .unwrap(),
        text.as_bytes()
    );
    assert_eq!(
        control.stage(&text).unwrap().dictionary_id,
        info.dictionary_id
    );
    assert_eq!(control.status().generation, 0);
    assert_eq!(control.status().dictionary, None);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn cancelled_or_expired_activation_does_not_apply_later() {
    let (dir, manager) = setup();
    let (control, mut runtime) =
        DictionaryControl::new("UART_A", &ParserConfig::default(), manager).unwrap();
    let info = control.stage(&dictionary("image-A")).unwrap();
    let path = dir
        .join("dictionaries")
        .join(format!("{}.json", info.dictionary_id));
    let config = ParserConfig {
        parser_type: "zephyr-dict".into(),
        database: Some(path.to_string_lossy().into_owned().into()),
    };
    let mut current_config = ParserConfig::default();
    let mut current = create_parser(&current_config);
    for cancelled in [false, true] {
        let (ack, receive) = oneshot::channel();
        if cancelled {
            drop(receive);
        }
        control
            .updates
            .send(Update {
                parser: try_create_dictionary_parser(&config).unwrap(),
                config: config.clone(),
                info: info.clone(),
                expected_generation: 0,
                deadline: Instant::now() - Duration::from_secs(1),
                ack,
            })
            .await
            .unwrap();
        let (result, _) = runtime
            .apply_next(&mut current, &mut current_config)
            .unwrap();
        assert!(result.is_err());
        assert_eq!(control.status().generation, 0);
        assert_eq!(current_config.parser_type, "text");
    }
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn unused_runtime_does_not_write_rotated_session() {
    let (dir, manager) = setup();
    std::fs::create_dir_all(&dir).unwrap();
    let initial = dir.join("initial.json");
    std::fs::write(&initial, dictionary("image-A")).unwrap();
    let config = ParserConfig {
        parser_type: "zephyr-dict".into(),
        database: Some(initial.to_string_lossy().into_owned().into()),
    };
    let (_, runtime) = DictionaryControl::new("UART_A", &config, manager.clone()).unwrap();
    let (mut master, mut slave) = serialport::TTYPort::pair().unwrap();
    slave.set_timeout(Duration::from_millis(100)).unwrap();
    let (tx, mut rx) = mpsc::channel(16);
    let mut source = UartSource::from_port("UART_A", Box::new(slave), "text").with_parser(config);
    source.set_dictionary_runtime(runtime);
    let task = tokio::spawn(Box::new(source).run(tx));
    tokio::task::block_in_place(|| master.write_all(&record()).unwrap());
    assert!(timeout(Duration::from_secs(3), rx.recv())
        .await
        .unwrap()
        .unwrap()
        .message
        .ends_with("image-A"));
    let rotated = dir.join("rotated");
    *manager.lock().unwrap() = session(rotated.clone());
    tokio::time::sleep(Duration::from_millis(300)).await;
    tokio::task::block_in_place(|| master.write_all(&record()).unwrap());
    assert!(timeout(Duration::from_secs(3), rx.recv())
        .await
        .unwrap()
        .unwrap()
        .message
        .ends_with("image-A"));
    assert!(!rotated.join("dictionaries").exists());
    task.abort();
    let _ = task.await;
    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(target_os = "linux")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn raw_write_failure_does_not_stop_uart() {
    let (dir, manager) = setup();
    let (control, runtime) =
        DictionaryControl::new("UART_A", &ParserConfig::default(), manager).unwrap();
    control.stage(&dictionary("image-A")).unwrap();
    let raw = dir
        .join("dictionaries")
        .join(format!("{}-uart.bin", control.artifact_name));
    std::os::unix::fs::symlink("/dev/full", raw).unwrap();
    let (mut master, mut slave) = serialport::TTYPort::pair().unwrap();
    slave.set_timeout(Duration::from_millis(100)).unwrap();
    let (tx, mut rx) = mpsc::channel(16);
    let mut source = UartSource::from_port("UART_A", Box::new(slave), "text");
    source.set_dictionary_runtime(runtime);
    let task = tokio::spawn(Box::new(source).run(tx));
    tokio::task::block_in_place(|| master.write_all(b"first line\n").unwrap());
    let warning = timeout(Duration::from_secs(3), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(warning.message.contains("raw capture disabled"));
    let first = timeout(Duration::from_secs(3), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(first.message, "first line");
    assert!(control.status().raw_error.is_some());
    assert!(!control.capture_raw.load(Ordering::Acquire));
    tokio::task::block_in_place(|| master.write_all(b"second line\n").unwrap());
    let second = timeout(Duration::from_secs(3), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert_eq!(second.message, "second line");
    assert!(!task.is_finished());
    task.abort();
    let _ = task.await;
    std::fs::remove_dir_all(dir).unwrap();
}

#[test]
fn configured_dictionaries_are_snapshotted_and_bad_alternatives_are_visible() {
    let (dir, manager) = setup();
    std::fs::create_dir_all(&dir).unwrap();
    let first = dir.join("first.json");
    let second = dir.join("second.json");
    let missing = dir.join("missing.json");
    std::fs::write(&first, dictionary("image-A")).unwrap();
    std::fs::write(&second, dictionary("image-B")).unwrap();
    let config = ParserConfig {
        parser_type: "zephyr-dict".into(),
        database: Some(crate::config::models::DictionaryPaths::Multiple(
            [&first, &second, &missing]
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
        )),
    };
    let (control, runtime) = DictionaryControl::new("UART_A", &config, manager).unwrap();
    let status = control.status();
    assert_eq!(status.configured.len(), 3);
    assert_eq!(status.generation, 0);
    assert_eq!(status.dictionary, status.configured[0].dictionary);
    assert_ne!(status.dictionary, status.configured[1].dictionary);
    assert!(status.configured[2].error.is_some());
    assert!(status.configured[2].dictionary.is_none());
    assert!(!control.capture_raw.load(Ordering::Acquire));
    let second_id = &status.configured[1]
        .dictionary
        .as_ref()
        .unwrap()
        .dictionary_id;
    std::fs::write(second, "modified original").unwrap();
    assert!(control.staged.lock().unwrap()[second_id]
        .json
        .contains("image-B"));
    assert!(runtime.current_config(config).unwrap().database.is_some());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn runtime_switch_is_acknowledged_per_uart_and_discards_partial_record() {
    let (dir, manager) = setup();
    std::fs::create_dir_all(&dir).unwrap();
    let initial = dir.join("initial.json");
    std::fs::write(&initial, dictionary("image-A")).unwrap();
    let config = ParserConfig {
        parser_type: "zephyr-dict".into(),
        database: Some(initial.to_string_lossy().into_owned().into()),
    };
    let (control_a, runtime_a) =
        DictionaryControl::new("UART_A", &config, manager.clone()).unwrap();
    let (control_b, runtime_b) =
        DictionaryControl::new("UART_B", &config, manager.clone()).unwrap();
    let info = control_a.stage(&dictionary("image-B")).unwrap();
    // The owner must use its immutable snapshot, not this subsequently edited input.
    std::fs::write(initial, "invalid modified original").unwrap();
    let (mut master_a, mut slave_a) = serialport::TTYPort::pair().unwrap();
    let (mut master_b, mut slave_b) = serialport::TTYPort::pair().unwrap();
    slave_a.set_timeout(Duration::from_millis(100)).unwrap();
    slave_b.set_timeout(Duration::from_millis(100)).unwrap();
    let (tx, mut rx) = mpsc::channel::<LogEntry>(128);
    let mut a =
        UartSource::from_port("UART_A", Box::new(slave_a), "text").with_parser(config.clone());
    let mut b = UartSource::from_port("UART_B", Box::new(slave_b), "text").with_parser(config);
    a.set_dictionary_runtime(runtime_a);
    b.set_dictionary_runtime(runtime_b);
    let task_a = tokio::spawn(Box::new(a).run(tx.clone()));
    let task_b = tokio::spawn(Box::new(b).run(tx));
    tokio::task::block_in_place(|| {
        master_a.write_all(&record()).unwrap();
        master_b.write_all(&record()).unwrap();
    });
    for _ in 0..2 {
        let entry = timeout(Duration::from_secs(3), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert!(entry.message.contains("app: image-A"), "{}", entry.message);
    }
    // Queue a partial old record. Activation must not prepend it to image B.
    tokio::task::block_in_place(|| master_a.write_all(&record()[..9]).unwrap());
    timeout(Duration::from_secs(3), async {
        while control_a.status().raw_offset < (record().len() + 9) as u64 {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    let active = control_a
        .activate(&info.dictionary_id, "zephyr-dict", 0)
        .await
        .unwrap();
    assert_eq!(active.generation, 1);
    assert_eq!(control_b.status().generation, 0);
    assert!(control_a
        .activate(&info.dictionary_id, "text", 1)
        .await
        .is_err());
    assert!(control_a
        .activate("unknown", "zephyr-dict", 1)
        .await
        .is_err());
    assert!(control_a
        .activate(&info.dictionary_id, "zephyr-dict", 0)
        .await
        .unwrap_err()
        .contains("generation conflict"));
    tokio::task::block_in_place(|| {
        master_a.write_all(&record()).unwrap();
        master_b.write_all(&record()).unwrap();
    });
    let mut seen = std::collections::HashSet::new();
    timeout(Duration::from_secs(3), async {
        while seen.len() < 2 {
            let entry = rx.recv().await.unwrap();
            if entry.message.contains("app:") {
                let expected = if entry.source == "UART_A" {
                    "image-B"
                } else {
                    "image-A"
                };
                assert!(entry.message.ends_with(expected), "{}", entry.message);
                seen.insert(entry.source);
            }
        }
    })
    .await
    .unwrap();
    // Simulate a new rotated session. Current B and its bytes must be self-contained.
    let rotated = dir.join("rotated");
    *manager.lock().unwrap() = session(rotated.clone());
    tokio::task::block_in_place(|| master_a.write_all(&record()).unwrap());
    timeout(Duration::from_secs(3), async {
        while control_a
            .status()
            .raw_file
            .as_ref()
            .map_or(true, |p| !p.contains("rotated"))
        {
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    assert!(rotated
        .join("dictionaries")
        .join(format!("{}.json", info.dictionary_id))
        .exists());
    assert_eq!(
        std::fs::read(control_a.status().raw_file.unwrap()).unwrap(),
        record()
    );
    let history = std::fs::read_dir(dir.join("dictionaries"))
        .unwrap()
        .filter_map(Result::ok)
        .find(|p| p.file_name().to_string_lossy().ends_with("-changes.jsonl"))
        .unwrap();
    let event: serde_json::Value =
        serde_json::from_str(std::fs::read_to_string(history.path()).unwrap().trim()).unwrap();
    assert_eq!(event["active"]["raw_offset"], record().len() + 9);
    assert_eq!(
        event["active"]["dictionary"]["dictionary_id"],
        info.dictionary_id
    );
    task_a.abort();
    task_b.abort();
    let _ = task_a.await;
    let _ = task_b.await;
    drop((master_a, master_b));
    std::fs::remove_dir_all(dir).unwrap();
}
