use anyhow::Result;
use chrono::Local;
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use tokio::sync::mpsc;
use tracing::{info, warn};

use super::traits::LogSource;
use crate::models::LogEntry;
use crate::parsers::create_parser;

/// Watches a file for new content and emits lines as [`LogEntry`].
///
/// Uses `notify` for filesystem events and reads new bytes on each change.
pub struct FileSource {
    name: String,
    file_path: String,
    parser_type: String,
}

impl FileSource {
    pub fn new(name: impl Into<String>, file_path: impl Into<String>) -> Self {
        Self::new_with_parser(name, file_path, "text")
    }

    pub fn new_with_parser(
        name: impl Into<String>,
        file_path: impl Into<String>,
        parser_type: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            file_path: file_path.into(),
            parser_type: parser_type.into(),
        }
    }
}

#[async_trait::async_trait]
impl LogSource for FileSource {
    async fn run(self: Box<Self>, tx: mpsc::Sender<LogEntry>) -> Result<()> {
        let path = std::path::PathBuf::from(&self.file_path);
        if !path.exists() {
            // Create the file if it doesn't exist so we can watch it.
            std::fs::File::create(&path)?;
        }

        info!("[{}] watching file: {}", self.name, path.display());

        let (notify_tx, mut notify_rx) = mpsc::channel::<()>(64);
        let _watch_path = path.clone();

        // Set up filesystem watcher in a blocking thread.
        let mut watcher: RecommendedWatcher = Watcher::new(
            move |result: Result<Event, notify::Error>| match result {
                Ok(event) => {
                    if matches!(event.kind, EventKind::Modify(_)) {
                        let _ = notify_tx.blocking_send(());
                    }
                }
                Err(e) => {
                    warn!("fs watch error: {e}");
                }
            },
            notify::Config::default(),
        )?;

        watcher.watch(path.parent().unwrap_or(&path), RecursiveMode::NonRecursive)?;

        // Read from current end of file.
        let mut offset = std::fs::metadata(&path)?.len();
        let mut parser = create_parser(&self.parser_type);
        let mut poll = tokio::time::interval(std::time::Duration::from_millis(250));

        loop {
            tokio::select! {
                event = notify_rx.recv() => {
                    if event.is_none() {
                        break;
                    }
                }
                _ = poll.tick() => {}
            }

            // Read new bytes.
            let metadata = match std::fs::metadata(&path) {
                Ok(m) => m,
                Err(_) => continue,
            };
            let new_len = metadata.len();
            if new_len <= offset {
                // File was truncated — reset.
                if new_len < offset {
                    offset = 0;
                    parser = create_parser(&self.parser_type);
                }
                continue;
            }

            let mut file = std::fs::File::open(&path)?;
            use std::io::{Read, Seek, SeekFrom};
            file.seek(SeekFrom::Start(offset))?;

            let mut buf = vec![0u8; (new_len - offset) as usize];
            let n = file.read(&mut buf)?;
            offset += n as u64;

            for line in parser.feed(&buf[..n]) {
                let trimmed = line.trim_end();
                if trimmed.is_empty() {
                    continue;
                }
                let entry = LogEntry::new(Local::now(), self.name.clone(), trimmed.to_string());
                if tx.send(entry).await.is_err() {
                    return Ok(());
                }
            }
        }

        Ok(())
    }

    fn source_name(&self) -> &str {
        &self.name
    }

    fn source_type(&self) -> &str {
        "file"
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sources::traits::LogSource;
    use std::io::Write;
    use tokio::time::{timeout, Duration};

    #[tokio::test]
    async fn file_source_emits_appended_lines() {
        let root =
            std::env::temp_dir().join(format!("embed-log-file-source-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let path = root.join("watched.log");
        std::fs::write(&path, "existing\n").unwrap();

        let (tx, mut rx) = mpsc::channel(2);
        let source = FileSource::new("file", path.display().to_string());
        let handle = tokio::spawn(async move {
            let _ = Box::new(source).run(tx).await;
        });

        tokio::time::sleep(Duration::from_millis(200)).await;
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .unwrap();
        writeln!(file, "appended").unwrap();
        file.flush().unwrap();

        let entry = timeout(Duration::from_secs(3), rx.recv())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(entry.source, "file");
        assert_eq!(entry.message, "appended");

        handle.abort();
        std::fs::remove_dir_all(root).unwrap();
    }
}
