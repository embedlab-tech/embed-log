use super::*;
use crate::models::LogEntry;
use crate::parsers::create_parser;
use crate::sources::traits::LogSource;
use crate::sources::uart::UartSource;
use serialport::SerialPort;
use tokio::time::timeout;

#[path = "dictionary_anchor_tests.rs"]
mod anchors;

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
        ..Default::default()
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
                arm: None,
                deadline: Instant::now() - Duration::from_secs(1),
                ack,
            })
            .await
            .unwrap();
        let (result, _, _) = runtime
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
        ..Default::default()
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
        ..Default::default()
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
        ..Default::default()
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
    assert_eq!(
        control_a
            .activate(&info.dictionary_id, "zephyr-dict", 0)
            .await
            .unwrap()
            .generation,
        1
    );
    let old_id = control_a.status().configured[0]
        .dictionary
        .as_ref()
        .unwrap()
        .dictionary_id
        .clone();
    assert!(control_a
        .activate(&old_id, "zephyr-dict", 0)
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
        // Opening the rotated file publishes its path before the bytes are written.
        while {
            let status = control_a.status();
            status
                .raw_file
                .as_ref()
                .map_or(true, |p| !p.contains("rotated"))
                || status.raw_offset < record().len() as u64
        } {
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

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reload_rereads_the_configured_file_and_keeps_the_old_one_on_failure() {
    let (dir, manager) = setup();
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("log_dictionary.json");
    std::fs::write(&file, dictionary("image-A")).unwrap();
    let config = ParserConfig {
        parser_type: "zephyr-dict".into(),
        database: Some(file.to_string_lossy().into_owned().into()),
        ..Default::default()
    };
    let (control, runtime) = DictionaryControl::new("UART_A", &config, manager).unwrap();
    let (mut master, mut slave) = serialport::TTYPort::pair().unwrap();
    slave.set_timeout(Duration::from_millis(100)).unwrap();
    let (tx, mut rx) = mpsc::channel::<LogEntry>(128);
    let mut source =
        UartSource::from_port("UART_A", Box::new(slave), "text").with_parser(config.clone());
    source.set_dictionary_runtime(runtime);
    let task = tokio::spawn(Box::new(source).run(tx));
    let next_message = |master: &mut serialport::TTYPort| {
        tokio::task::block_in_place(|| master.write_all(&record()).unwrap());
    };

    // The file did not change: nothing to apply, generation stays.
    let outcome = control.reload(0).await.unwrap();
    assert!(!outcome.reloaded);
    assert_eq!(outcome.status.generation, 0);

    // A rebuild overwrites the file in place; the startup snapshot keeps decoding.
    assert!(!control.status().configured[0].changed_on_disk);
    std::fs::write(&file, dictionary("image-B")).unwrap();
    assert!(control.status().configured[0].changed_on_disk);
    next_message(&mut master);
    let entry = timeout(Duration::from_secs(3), rx.recv())
        .await
        .unwrap()
        .unwrap();
    assert!(entry.message.ends_with("image-A"), "{}", entry.message);

    // Invalid, missing and stale-generation reloads leave the active one alone.
    std::fs::write(&file, "{ not a dictionary").unwrap();
    assert!(control.reload(0).await.is_err());
    std::fs::remove_file(&file).unwrap();
    assert!(control.reload(0).await.unwrap_err().contains("cannot read"));
    assert_eq!(control.status().generation, 0);
    std::fs::write(&file, dictionary("image-B")).unwrap();
    assert!(control
        .reload(5)
        .await
        .unwrap_err()
        .contains("generation conflict"));

    let outcome = control.reload(0).await.unwrap();
    assert!(outcome.reloaded);
    assert_eq!(outcome.status.generation, 1);
    assert!(!outcome.status.configured[0].changed_on_disk);
    assert_eq!(outcome.path, file.to_string_lossy());
    // The selector entry follows the file, so the active id is still "configured".
    let status = control.status();
    assert_eq!(status.dictionary, status.configured[0].dictionary);
    assert_eq!(status.dictionary.as_ref(), Some(&outcome.dictionary));
    // The superseded snapshot is released but stays on disk for the session.
    assert_eq!(control.staged.lock().unwrap().len(), 1);
    next_message(&mut master);
    let entry = timeout(Duration::from_secs(3), async {
        loop {
            let entry = rx.recv().await.unwrap();
            if entry.message.contains("app:") && entry.message.ends_with("image-B") {
                return entry;
            }
        }
    })
    .await
    .unwrap();
    assert!(entry.message.ends_with("image-B"));

    // Touching the file changes its stamp but not its content: the hint clears
    // without a new generation.
    std::fs::write(&file, dictionary("image-B")).unwrap();
    assert!(control.status().configured[0].changed_on_disk);
    let outcome = control.reload(1).await.unwrap();
    assert!(!outcome.status.configured[0].changed_on_disk);
    assert!(!outcome.reloaded);
    assert_eq!(outcome.status.generation, 1);

    // An uploaded dictionary is not backed by a file, so there is nothing to reload.
    let uploaded = control.stage(&dictionary("image-C")).unwrap();
    control
        .activate(&uploaded.dictionary_id, "zephyr-dict", 1)
        .await
        .unwrap();
    assert!(control.reload(2).await.unwrap_err().contains("uploaded"));
    task.abort();
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn watch_reloads_a_rebuilt_file_and_reports_a_bad_one_once() {
    let (dir, manager) = setup();
    std::fs::create_dir_all(&dir).unwrap();
    let file = dir.join("log_dictionary.json");
    std::fs::write(&file, dictionary("image-A")).unwrap();
    let config = ParserConfig {
        parser_type: "zephyr-dict".into(),
        database: Some(file.to_string_lossy().into_owned().into()),
        watch: true,
    };
    let (control, runtime) = DictionaryControl::new("UART_A", &config, manager).unwrap();
    assert!(control.status().watch);
    let (mut master, mut slave) = serialport::TTYPort::pair().unwrap();
    slave.set_timeout(Duration::from_millis(100)).unwrap();
    let (tx, mut rx) = mpsc::channel::<LogEntry>(128);
    let mut source =
        UartSource::from_port("UART_A", Box::new(slave), "text").with_parser(config.clone());
    source.set_dictionary_runtime(runtime);
    let task = tokio::spawn(Box::new(source).run(tx));
    let watcher = tokio::spawn(control.clone().watch_active_file());
    let wait_for = |what: &'static str, check: Box<dyn Fn(&DictionaryStatus) -> bool>| {
        let control = control.clone();
        async move {
            timeout(Duration::from_secs(10), async {
                while !check(&control.status()) {
                    tokio::time::sleep(Duration::from_millis(50)).await;
                }
            })
            .await
            .unwrap_or_else(|_| panic!("timed out waiting for {what}"));
        }
    };

    // A rebuild: the watcher applies it without any request.
    std::fs::write(&file, dictionary("image-B")).unwrap();
    wait_for("automatic reload", Box::new(|s| s.generation == 1)).await;
    assert!(!control.status().configured[0].changed_on_disk);
    tokio::task::block_in_place(|| master.write_all(&record()).unwrap());
    let entry = timeout(Duration::from_secs(3), async {
        loop {
            let entry = rx.recv().await.unwrap();
            if entry.message.contains("app:") {
                return entry;
            }
        }
    })
    .await
    .unwrap();
    assert!(entry.message.ends_with("image-B"), "{}", entry.message);

    // A broken file is reported, the active dictionary stays, and it is tried once.
    std::fs::write(&file, "{ half written").unwrap();
    wait_for("watch error", Box::new(|s| s.watch_error.is_some())).await;
    assert_eq!(control.status().generation, 1);
    tokio::time::sleep(Duration::from_millis(2500)).await;
    assert_eq!(control.status().generation, 1);

    // The next good build recovers and clears the error.
    std::fs::write(&file, dictionary("image-C")).unwrap();
    wait_for(
        "recovery",
        Box::new(|s| s.generation == 2 && s.watch_error.is_none()),
    )
    .await;
    watcher.abort();
    task.abort();
    std::fs::remove_dir_all(dir).unwrap();
}

/// A UART source on a pty that applies this control's updates.
fn spawn_uart(
    runtime: DictionaryRuntime,
    config: &ParserConfig,
) -> (
    serialport::TTYPort,
    tokio::task::JoinHandle<anyhow::Result<()>>,
) {
    let (master, mut slave) = serialport::TTYPort::pair().unwrap();
    slave.set_timeout(Duration::from_millis(100)).unwrap();
    let (tx, mut rx) = mpsc::channel::<LogEntry>(128);
    tokio::spawn(async move { while rx.recv().await.is_some() {} });
    let mut source =
        UartSource::from_port("UART_A", Box::new(slave), "text").with_parser(config.clone());
    source.set_dictionary_runtime(runtime);
    (master, tokio::spawn(Box::new(source).run(tx)))
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn files_with_identical_bytes_are_told_apart_by_entry_not_by_id() {
    let (dir, manager) = setup();
    std::fs::create_dir_all(&dir).unwrap();
    let debug = dir.join("debug.json");
    let release = dir.join("release.json");
    std::fs::write(&debug, dictionary("image-A")).unwrap();
    std::fs::write(&release, dictionary("image-A")).unwrap();
    let config = ParserConfig {
        parser_type: "zephyr-dict".into(),
        database: Some(crate::config::models::DictionaryPaths::Multiple(vec![
            debug.to_string_lossy().into_owned(),
            release.to_string_lossy().into_owned(),
        ])),
        ..Default::default()
    };
    let (control, runtime) = DictionaryControl::new("UART_A", &config, manager).unwrap();
    let (_master, task) = spawn_uart(runtime, &config);
    let status = control.status();
    let id = status.configured[0]
        .dictionary
        .clone()
        .unwrap()
        .dictionary_id;
    assert_eq!(
        status.configured[1]
            .dictionary
            .as_ref()
            .unwrap()
            .dictionary_id,
        id
    );
    assert_eq!(status.active_index, Some(0));

    // Choosing the second file changes nothing about decoding, but Reload must follow it.
    let chosen = control
        .activate_configured(&id, "zephyr-dict", 0, Some(1))
        .await
        .unwrap();
    assert_eq!((chosen.generation, chosen.active_index), (0, Some(1)));
    assert!(control
        .activate_configured(&id, "zephyr-dict", 0, Some(5))
        .await
        .is_err());

    // Only release.json is rebuilt: that is the file that must be re-read.
    std::fs::write(&release, dictionary("image-B")).unwrap();
    let status = control.status();
    assert!(!status.configured[0].changed_on_disk);
    assert!(status.configured[1].changed_on_disk);
    let outcome = control.reload(0).await.unwrap();
    assert!(
        outcome.reloaded,
        "the rebuilt file is read, not the first match"
    );
    assert_eq!(outcome.path, release.to_string_lossy());
    assert_eq!(outcome.status.generation, 1);
    assert_eq!(outcome.status.active_index, Some(1));
    assert_ne!(outcome.dictionary.dictionary_id, id);
    // debug.json keeps its own snapshot.
    assert_eq!(
        outcome.status.configured[0]
            .dictionary
            .as_ref()
            .unwrap()
            .dictionary_id,
        id
    );
    task.abort();
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn reload_of_an_unchanged_file_still_rejects_a_stale_generation() {
    let (dir, manager) = setup();
    std::fs::create_dir_all(&dir).unwrap();
    let first = dir.join("first.json");
    let second = dir.join("second.json");
    std::fs::write(&first, dictionary("image-A")).unwrap();
    std::fs::write(&second, dictionary("image-B")).unwrap();
    let config = ParserConfig {
        parser_type: "zephyr-dict".into(),
        database: Some(crate::config::models::DictionaryPaths::Multiple(vec![
            first.to_string_lossy().into_owned(),
            second.to_string_lossy().into_owned(),
        ])),
        ..Default::default()
    };
    let (control, runtime) = DictionaryControl::new("UART_A", &config, manager).unwrap();
    let (_master, task) = spawn_uart(runtime, &config);
    let second_id = control.status().configured[1]
        .dictionary
        .clone()
        .unwrap()
        .dictionary_id;
    // Another client switches the dictionary: generation 0 -> 1.
    control
        .activate_configured(&second_id, "zephyr-dict", 0, Some(1))
        .await
        .unwrap();
    assert_eq!(control.status().generation, 1);
    // The file did not change, but the caller's generation is stale.
    let error = control.reload(0).await.unwrap_err();
    assert!(error.contains("generation conflict"), "{error}");
    assert!(!control.reload(1).await.unwrap().reloaded);
    task.abort();
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_stale_client_cannot_move_the_followed_file() {
    let (dir, manager) = setup();
    std::fs::create_dir_all(&dir).unwrap();
    let debug = dir.join("debug.json");
    let release = dir.join("release.json");
    let other = dir.join("other.json");
    std::fs::write(&debug, dictionary("image-A")).unwrap();
    std::fs::write(&release, dictionary("image-A")).unwrap();
    std::fs::write(&other, dictionary("image-B")).unwrap();
    let config = ParserConfig {
        parser_type: "zephyr-dict".into(),
        database: Some(crate::config::models::DictionaryPaths::Multiple(
            [&debug, &release, &other]
                .iter()
                .map(|path| path.to_string_lossy().into_owned())
                .collect(),
        )),
        ..Default::default()
    };
    let (control, runtime) = DictionaryControl::new("UART_A", &config, manager).unwrap();
    let (_master, task) = spawn_uart(runtime, &config);
    let status = control.status();
    let id_a = status.configured[0]
        .dictionary
        .clone()
        .unwrap()
        .dictionary_id;
    let id_b = status.configured[2]
        .dictionary
        .clone()
        .unwrap()
        .dictionary_id;

    // Another client goes A -> B -> A: generation 0 -> 2, followed file debug.json.
    control
        .activate_configured(&id_b, "zephyr-dict", 0, Some(2))
        .await
        .unwrap();
    control
        .activate_configured(&id_a, "zephyr-dict", 1, Some(0))
        .await
        .unwrap();
    assert_eq!(control.status().generation, 2);

    // A request made back at generation 0 arrives late and names release.json,
    // which holds the active bytes. It must be refused, not redirect Reload.
    let error = control
        .activate_configured(&id_a, "zephyr-dict", 0, Some(1))
        .await
        .unwrap_err();
    assert!(error.contains("generation conflict"), "{error}");
    assert_eq!(control.status().active_index, Some(0));

    // With the current generation the same choice is accepted.
    let moved = control
        .activate_configured(&id_a, "zephyr-dict", 2, Some(1))
        .await
        .unwrap();
    assert_eq!((moved.generation, moved.active_index), (2, Some(1)));
    task.abort();
    std::fs::remove_dir_all(dir).unwrap();
}
