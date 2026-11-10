use super::*;

type Fixture = (
    PathBuf,
    DictionaryControl,
    DictionaryRuntime,
    Box<dyn StreamParser>,
    ParserConfig,
);

fn fixture() -> Fixture {
    let (dir, manager) = setup();
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("initial.json");
    std::fs::write(&path, dictionary("image-A")).unwrap();
    let config = ParserConfig {
        parser_type: "zephyr-dict".into(),
        database: Some(path.to_string_lossy().into_owned().into()),
    };
    let (control, runtime) = DictionaryControl::new("UART_A", &config, manager).unwrap();
    let config = runtime.current_config(config).unwrap();
    let parser = create_parser(&config);
    (dir, control, runtime, parser, config)
}

async fn apply<T>(
    runtime: &mut DictionaryRuntime,
    parser: &mut Box<dyn StreamParser>,
    config: &mut ParserConfig,
    task: tokio::task::JoinHandle<T>,
) -> T {
    timeout(Duration::from_secs(3), async {
        while runtime.updates.is_empty() {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    let (result, ack, _) = runtime.apply_next(parser, config).unwrap();
    let _ = ack.send(result);
    task.await.unwrap()
}

async fn arm(
    control: &DictionaryControl,
    runtime: &mut DictionaryRuntime,
    parser: &mut Box<dyn StreamParser>,
    config: &mut ParserConfig,
    target: &str,
) -> DictionaryInfo {
    let control = control.clone();
    let json = dictionary(target);
    let task =
        tokio::spawn(async move { control.stage_with_regex(&json, Some("^RAW-READY:B$")).await });
    apply(runtime, parser, config, task).await.unwrap()
}

fn feed(
    runtime: &mut DictionaryRuntime,
    parser: &mut Box<dyn StreamParser>,
    config: &mut ParserConfig,
    bytes: &[u8],
) -> Vec<String> {
    runtime.record_raw(bytes).unwrap();
    runtime.feed(parser, config, bytes)
}

#[tokio::test]
async fn raw_anchor_handles_every_split_and_switches_inside_the_same_chunk() {
    let marker = b"\r\nRAW-READY:B\r\n";
    for split in 0..=marker.len() {
        let (dir, control, mut runtime, mut parser, mut config) = fixture();
        let target = arm(&control, &mut runtime, &mut parser, &mut config, "image-B").await;
        assert_eq!(control.status().generation, 0);
        assert!(control.status().armed.is_some());
        let mut first = record();
        first.extend_from_slice(&marker[..split]);
        let mut lines = feed(&mut runtime, &mut parser, &mut config, &first);
        let mut second = marker[split..].to_vec();
        second.extend(record());
        lines.extend(feed(&mut runtime, &mut parser, &mut config, &second));
        assert_eq!(
            lines.iter().filter(|l| l.ends_with("app: image-A")).count(),
            1,
            "{lines:?}"
        );
        assert_eq!(
            lines.iter().filter(|l| l.ends_with("app: image-B")).count(),
            1,
            "{lines:?}"
        );
        assert_eq!(control.status().generation, 1);
        assert_eq!(control.status().dictionary, Some(target));
        assert!(control.status().armed.is_none());
        assert_eq!(
            std::fs::read(control.status().raw_file.unwrap()).unwrap(),
            [first, second].concat()
        );
        let history = dir
            .join("dictionaries")
            .join(format!("{}-changes.jsonl", control.artifact_name));
        let history = std::fs::read_to_string(history).unwrap();
        assert_eq!(history.lines().count(), 1);
        let event: serde_json::Value = serde_json::from_str(history.trim()).unwrap();
        assert_eq!(event["reason"], "anchor");
        assert_eq!(event["anchor_raw_offset"], record().len() + marker.len());
        assert_eq!(event["partial_record_discarded"], true);
        std::fs::remove_dir_all(dir).unwrap();
    }
}

#[tokio::test]
async fn decoded_strings_and_wrong_raw_identifiers_cannot_supply_the_anchor() {
    let (dir, control, mut runtime, mut parser, mut config) = fixture();
    arm(
        &control,
        &mut runtime,
        &mut parser,
        &mut config,
        "RAW-READY:B",
    )
    .await;
    let mut bytes = record();
    bytes.extend_from_slice(b"\r\nRAW-READY:wrong\r\n");
    bytes.extend(record());
    let lines = feed(&mut runtime, &mut parser, &mut config, &bytes);
    assert_eq!(control.status().generation, 0);
    assert!(lines.iter().any(|l| l.ends_with("image-A")));
    assert!(control.status().armed.is_some());
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn invalid_patterns_do_not_change_an_existing_arm_or_generation() {
    let (dir, control, mut runtime, mut parser, mut config) = fixture();
    arm(&control, &mut runtime, &mut parser, &mut config, "image-B").await;
    for pattern in ["[".to_string(), ".*".into(), "x".repeat(1025)] {
        assert!(control
            .stage_with_regex(&dictionary("image-C"), Some(&pattern))
            .await
            .is_err());
        assert_eq!(
            control.status().armed.unwrap().activate_on_regex,
            "^RAW-READY:B$"
        );
        assert_eq!(control.status().generation, 0);
    }
    control
        .stage_with_regex(&dictionary("image-C"), None)
        .await
        .unwrap();
    assert!(control.status().armed.is_some());
    assert!(control
        .stage_with_regex(&dictionary("image-A"), Some("marker"))
        .await
        .is_err());
    let (text, _) = DictionaryControl::new(
        "text",
        &ParserConfig::default(),
        Arc::new(Mutex::new(session(dir.join("text")))),
    )
    .unwrap();
    assert!(text
        .stage_with_regex(&dictionary("B"), Some("marker"))
        .await
        .unwrap_err()
        .contains("requires zephyr-dict"));
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn timeout_reconnect_and_line_limit_preserve_bytes_without_joining_marker_fragments() {
    let (dir, control, mut runtime, mut parser, mut config) = fixture();
    arm(&control, &mut runtime, &mut parser, &mut config, "image-B").await;
    feed(&mut runtime, &mut parser, &mut config, b"RAW-");
    assert_eq!(runtime.armed.as_ref().unwrap().pending, b"RAW-");
    runtime.armed.as_mut().unwrap().since = Some(Instant::now() - ANCHOR_LINE_TIMEOUT);
    let lines = runtime.on_idle(&mut parser, Duration::from_secs(60));
    assert!(lines.iter().any(|l| l.contains("timed out")));
    feed(&mut runtime, &mut parser, &mut config, b"READY:B\n");
    assert_eq!(control.status().generation, 0);
    feed(&mut runtime, &mut parser, &mut config, b"RAW-");
    runtime.reset_anchor(&mut parser);
    feed(&mut runtime, &mut parser, &mut config, b"READY:B\n");
    assert_eq!(control.status().generation, 0);
    let mut oversized = vec![b'x'; MAX_ANCHOR_LINE + 1];
    oversized.extend_from_slice(b"RAW-READY:B\n");
    feed(&mut runtime, &mut parser, &mut config, &oversized);
    assert!(runtime.armed.as_ref().unwrap().pending.is_empty());
    assert_eq!(control.status().generation, 0);
    feed(&mut runtime, &mut parser, &mut config, b"RAW-READY:B\n");
    assert_eq!(control.status().generation, 1);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn explicit_activation_disarms_and_confirmation_is_idempotent() {
    let (dir, control, mut runtime, mut parser, mut config) = fixture();
    let target = arm(&control, &mut runtime, &mut parser, &mut config, "image-B").await;
    feed(&mut runtime, &mut parser, &mut config, b"RAW-");
    let clone = control.clone();
    let id = target.dictionary_id;
    let task = tokio::spawn(async move { clone.activate(&id, "zephyr-dict", 0).await });
    assert_eq!(
        apply(&mut runtime, &mut parser, &mut config, task)
            .await
            .unwrap()
            .generation,
        1
    );
    assert!(control.status().armed.is_none());
    let bytes = record();
    assert!(feed(&mut runtime, &mut parser, &mut config, &bytes[..9]).is_empty());
    let clone = control.clone();
    let id = control.status().dictionary.unwrap().dictionary_id;
    let task = tokio::spawn(async move { clone.activate(&id, "zephyr-dict", 0).await });
    assert_eq!(
        apply(&mut runtime, &mut parser, &mut config, task)
            .await
            .unwrap()
            .generation,
        1
    );
    let lines = feed(&mut runtime, &mut parser, &mut config, &bytes[9..]);
    assert!(lines.iter().any(|l| l.ends_with("image-B")));
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn explicit_activation_survives_failure_to_open_the_raw_file() {
    let (dir, control, mut runtime, mut parser, mut config) = fixture();
    let target = control.stage(&dictionary("image-B")).unwrap();
    let raw = dir
        .join("dictionaries")
        .join(format!("{}-uart.bin", control.artifact_name));
    std::fs::create_dir(raw).unwrap();
    let clone = control.clone();
    let task = tokio::spawn(async move {
        clone
            .activate(&target.dictionary_id, "zephyr-dict", 0)
            .await
    });
    let status = apply(&mut runtime, &mut parser, &mut config, task)
        .await
        .unwrap();
    assert_eq!(status.generation, 1);
    assert!(status.raw_error.is_some());
    assert!(runtime
        .feed(&mut parser, &mut config, &record())
        .iter()
        .any(|l| l.ends_with("image-B")));
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn replacing_an_arm_flushes_the_old_fragment_and_uses_the_new_snapshot() {
    let (dir, control, mut runtime, mut parser, mut config) = fixture();
    arm(&control, &mut runtime, &mut parser, &mut config, "image-B").await;
    feed(&mut runtime, &mut parser, &mut config, b"RAW-");
    let replacement = arm(&control, &mut runtime, &mut parser, &mut config, "image-C").await;
    assert_eq!(control.status().generation, 0);
    assert_eq!(
        control.status().armed.unwrap().dictionary_id,
        replacement.dictionary_id
    );
    feed(&mut runtime, &mut parser, &mut config, b"READY:B\n");
    assert_eq!(control.status().generation, 0);
    let bytes = [b"RAW-READY:B\n".to_vec(), record()].concat();
    let lines = feed(&mut runtime, &mut parser, &mut config, &bytes);
    assert!(lines.iter().any(|l| l.ends_with("image-C")));
    assert_eq!(control.status().generation, 1);
    std::fs::remove_dir_all(dir).unwrap();
}

#[tokio::test]
async fn history_failure_keeps_the_old_parser_and_disarms_visibly() {
    let (dir, control, mut runtime, mut parser, mut config) = fixture();
    arm(&control, &mut runtime, &mut parser, &mut config, "image-B").await;
    let history = dir
        .join("dictionaries")
        .join(format!("{}-changes.jsonl", control.artifact_name));
    std::fs::create_dir(history).unwrap();
    let bytes = [b"RAW-READY:B\n".to_vec(), record()].concat();
    let lines = feed(&mut runtime, &mut parser, &mut config, &bytes);
    assert!(lines
        .iter()
        .any(|l| l.contains("activation failed; disarmed")));
    assert!(lines.iter().any(|l| l.ends_with("image-A")));
    assert_eq!(control.status().generation, 0);
    assert!(control.status().armed.is_none());
    assert!(runtime.armed.is_none());
    std::fs::remove_dir_all(dir).unwrap();
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn raw_write_failure_does_not_prevent_anchor_activation() {
    let (dir, control, mut runtime, mut parser, mut config) = fixture();
    arm(&control, &mut runtime, &mut parser, &mut config, "image-B").await;
    let raw = dir
        .join("dictionaries")
        .join(format!("{}-uart.bin", control.artifact_name));
    std::os::unix::fs::symlink("/dev/full", raw).unwrap();
    let bytes = [b"RAW-READY:B\n".to_vec(), record()].concat();
    let error = runtime.record_raw(&bytes).unwrap_err();
    runtime.disable_raw(error);
    let lines = runtime.feed(&mut parser, &mut config, &bytes);
    assert!(lines.iter().any(|l| l.ends_with("image-B")));
    assert_eq!(control.status().generation, 1);
    assert!(control.status().raw_error.is_some());
    std::fs::remove_dir_all(dir).unwrap();
}
