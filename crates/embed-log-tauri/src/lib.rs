use std::io::{Read, Write};
use std::net::{SocketAddr, TcpStream};
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, AtomicU16, Ordering},
    Arc, OnceLock,
};
use std::time::Duration;

use tauri::Manager;

use embed_log_core::config::load_config;
use embed_log_core::demo::{prepare_demo_file_sources, spawn_demo_traffic};
use embed_log_core::onboarding as ob;
use embed_log_core::onboarding::{QuickConfigDraft, QuickConfigResult, SaveHandler};
use embed_log_core::runtime::LogServer;

static CONFIG_PATH: OnceLock<PathBuf> = OnceLock::new();
static SERVER_STARTED: AtomicBool = AtomicBool::new(false);
static CURRENT_WS_PORT: AtomicU16 = AtomicU16::new(8080);

#[tauri::command]
fn get_server_status() -> ob::ServerStatus {
    let config_path = CONFIG_PATH.get().cloned().unwrap_or_default();
    let mut status = ob::server_status(&config_path);
    status.running = SERVER_STARTED.load(Ordering::SeqCst);
    status
}

#[tauri::command]
fn list_serial_ports() -> Vec<String> {
    ob::list_serial_ports()
}

#[tauri::command]
fn save_quick_config(
    app: tauri::AppHandle,
    draft: QuickConfigDraft,
) -> Result<QuickConfigResult, String> {
    let config_path = CONFIG_PATH
        .get()
        .cloned()
        .unwrap_or_else(|| app_default_config_path(&app));
    let result = ob::save_quick_config(&config_path, &draft)?;
    let config =
        load_config(&config_path).map_err(|e| format!("generated config is invalid: {e}"))?;
    start_log_server(app, config_path, config)?;
    Ok(result)
}

#[tauri::command]
fn open_external_url(url: String) -> Result<(), String> {
    let target = if url.starts_with("/sessions/") || url.starts_with("/api/") {
        let config_path = CONFIG_PATH.get().cloned().unwrap_or_default();
        let ws_port = load_config(&config_path)
            .ok()
            .map(|config| config.server.ws_port)
            .unwrap_or(8080);
        format!("http://127.0.0.1:{ws_port}{url}")
    } else if url.starts_with("http://127.0.0.1:")
        || url.starts_with("http://localhost:")
        || url.starts_with("https://")
    {
        url
    } else {
        return Err("refusing to open non-local URL".to_string());
    };

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg(&target)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        std::process::Command::new("xdg-open")
            .arg(&target)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("cmd")
            .args(["/C", "start", "", &target])
            .spawn()
            .map_err(|e| e.to_string())?;
    }

    Ok(())
}

/// Reveal a file in the system file manager (Finder / Explorer / xdg-open).
#[tauri::command]
fn reveal_in_file_manager(path: String) -> Result<(), String> {
    let p = std::path::Path::new(&path);
    // If the path doesn't exist, try to open the parent directory.
    let target = if p.exists() {
        p.to_path_buf()
    } else if let Some(parent) = p.parent() {
        parent.to_path_buf()
    } else {
        return Err("invalid path".into());
    };

    #[cfg(target_os = "macos")]
    {
        std::process::Command::new("open")
            .arg("-R")
            .arg(&target)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "linux")]
    {
        let dir = if target.is_dir() {
            &target
        } else {
            target.parent().unwrap_or(&target)
        };
        std::process::Command::new("xdg-open")
            .arg(dir)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    #[cfg(target_os = "windows")]
    {
        std::process::Command::new("explorer")
            .arg("/select,")
            .arg(&target)
            .spawn()
            .map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn export_current_session_via_http(port: u16) -> Result<(), String> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let mut stream = TcpStream::connect_timeout(&addr, Duration::from_secs(2))
        .map_err(|e| format!("connect to session API: {e}"))?;
    stream
        .set_read_timeout(Some(Duration::from_secs(10)))
        .map_err(|e| e.to_string())?;
    stream
        .set_write_timeout(Some(Duration::from_secs(2)))
        .map_err(|e| e.to_string())?;

    let request = format!(
        "POST /api/session/export HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nConnection: close\r\nContent-Length: 0\r\n\r\n"
    );
    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("write export request: {e}"))?;

    let mut response = String::new();
    stream
        .read_to_string(&mut response)
        .map_err(|e| format!("read export response: {e}"))?;
    let status_line = response.lines().next().unwrap_or_default();
    if status_line.contains(" 200 ") || status_line.contains(" 204 ") {
        Ok(())
    } else {
        Err(format!("export API returned {status_line}"))
    }
}

/// JS injected into the webview: shows a clickable toast on every download.
/// Clicking the toast reveals the file in Finder / Explorer.
const DOWNLOAD_TOAST_JS: &str = r#"
(function() {
    if (window.__embedLogDownloadToast) return;
    window.__embedLogDownloadToast = true;

    // Resolve the Tauri invoke function.
    // Tauri v2 exposes it at __TAURI__.core.invoke.
    var invoke = (window.__TAURI__ && window.__TAURI__.core && window.__TAURI__.core.invoke)
        || function() { return Promise.resolve(); };

    // ── Toast container ──
    var container = document.createElement('div');
    container.id = '__embed-log-toast-container';
    container.style.cssText = 'position:fixed;bottom:24px;right:24px;z-index:99999;' +
        'display:flex;flex-direction:column;gap:8px;pointer-events:none;' +
        'max-width:380px;';
    document.body.appendChild(container);

    // Resolve the system Downloads directory.
    // On macOS it's ~/Downloads; we reconstruct from the home dir.
    var downloadsDir = '';
    try {
        // Tauri v2 path API — fallback to common defaults.
        var home = (typeof process !== 'undefined' && process.env && process.env.HOME)
            || '/tmp';
        downloadsDir = home + '/Downloads';
    } catch(_) {
        downloadsDir = '/tmp';
    }

    function showToast(text, filePath) {
        var toast = document.createElement('div');
        toast.style.cssText = 'pointer-events:auto;cursor:pointer;' +
            'background:#2d2245;color:#e8e0f0;' +
            'padding:12px 18px;border-radius:10px;' +
            'font:13px/1.4 -apple-system,BlinkMacSystemFont,system-ui,sans-serif;' +
            'box-shadow:0 4px 20px rgba(0,0,0,0.25);' +
            'opacity:0;transform:translateY(8px);' +
            'transition:opacity 0.25s,transform 0.25s;' +
            'border:1px solid rgba(160,130,200,0.2);';
        toast.title = 'Click to reveal in Finder';

        var icon = document.createElement('span');
        icon.textContent = '✓ ';
        icon.style.color = '#a0e8a0';

        var label = document.createElement('span');
        label.textContent = text;

        var hint = document.createElement('div');
        hint.textContent = 'click to reveal';
        hint.style.cssText = 'font-size:11px;color:#9e8dbd;margin-top:3px;opacity:0.8;';

        toast.appendChild(icon);
        toast.appendChild(label);
        toast.appendChild(hint);
        container.appendChild(toast);

        // Click → reveal in file manager.
        toast.addEventListener('click', function() {
            if (filePath) {
                invoke('reveal_in_file_manager', { path: filePath }).catch(function() {});
            }
            // Dismiss immediately.
            toast.style.opacity = '0';
            toast.style.transform = 'translateY(8px)';
            setTimeout(function() {
                if (toast.parentNode) toast.parentNode.removeChild(toast);
            }, 200);
        });

        // Hover effect.
        toast.addEventListener('mouseenter', function() {
            toast.style.background = '#3d3265';
        });
        toast.addEventListener('mouseleave', function() {
            toast.style.background = '#2d2245';
        });

        // Animate in.
        requestAnimationFrame(function() {
            toast.style.opacity = '1';
            toast.style.transform = 'translateY(0)';
        });

        // Auto-dismiss after 6 seconds (longer so user has time to click).
        setTimeout(function() {
            if (!toast.parentNode) return;
            toast.style.opacity = '0';
            toast.style.transform = 'translateY(8px)';
            setTimeout(function() {
                if (toast.parentNode) toast.parentNode.removeChild(toast);
            }, 300);
        }, 6000);
    }

    // ── Intercept <a download> clicks ──
    document.addEventListener('click', function(e) {
        var a = e.target.closest('a[download]');
        if (!a) return;
        var name = a.download || a.href.split('/').pop() || 'file';
        var filePath = downloadsDir + '/' + name;
        setTimeout(function() { showToast('Saved: ' + name, filePath); }, 100);
    }, true);

    // ── Intercept Blob-based downloads ──
    var _origCreateObjectURL = URL.createObjectURL;
    var pendingDownloads = [];
    URL.createObjectURL = function(blob) {
        var url = _origCreateObjectURL.call(URL, blob);
        if (blob instanceof Blob) {
            pendingDownloads.push({ url: url, size: blob.size });
        }
        return url;
    };

    var _origClick = HTMLAnchorElement.prototype.click;
    HTMLAnchorElement.prototype.click = function() {
        if (this.download && this.href && this.href.startsWith('blob:')) {
            var href = this.href;
            var name = this.download;
            var info = pendingDownloads.find(function(d) { return d.url === href; });
            var sizeStr = info ? ' (' + (info.size / 1024).toFixed(1) + ' kB)' : '';
            pendingDownloads = pendingDownloads.filter(function(d) { return d.url !== href; });
            _origClick.call(this);
            var filePath = downloadsDir + '/' + name;
            setTimeout(function() { showToast('Saved: ' + name + sizeStr, filePath); }, 100);
            try { URL.revokeObjectURL(href); } catch(_) {}
            return;
        }
        _origClick.call(this);
    };
})();
"#;

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_shell::init())
        .invoke_handler(tauri::generate_handler![
            get_server_status,
            list_serial_ports,
            save_quick_config,
            reveal_in_file_manager,
            open_external_url
        ])
        .setup(|app| {
            let config_path = resolve_config_path(app);
            CONFIG_PATH.set(config_path.clone()).ok();

            let window = app.get_webview_window("main").unwrap();
            install_close_handler(&window, app.handle().clone());

            let config = match load_config(&config_path).map_err(|e| anyhow::anyhow!("{e}")) {
                Ok(config) => config,
                Err(error) => {
                    if config_path.exists() {
                        show_config_error(&window, &config_path, &error.to_string());
                    } else {
                        show_onboarding(&window, app.handle().clone(), config_path.clone());
                    }
                    return Ok(());
                }
            };

            let ws_port = config.server.ws_port;
            start_log_server(app.handle().clone(), config_path, config)
                .map_err(|e| anyhow::anyhow!(e))?;

            let url = format!("http://127.0.0.1:{ws_port}/");
            let window_clone = window.clone();
            std::thread::spawn(move || {
                std::thread::sleep(std::time::Duration::from_millis(500));
                let _ = window.eval(format!("window.location.href = '{url}';"));
                std::thread::sleep(std::time::Duration::from_millis(1500));
                let _ = window_clone.eval(DOWNLOAD_TOAST_JS);
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

fn install_close_handler(window: &tauri::WebviewWindow, app: tauri::AppHandle) {
    let close_started = Arc::new(AtomicBool::new(false));
    window.on_window_event(move |event| {
        if let tauri::WindowEvent::CloseRequested { api, .. } = event {
            api.prevent_close();
            if close_started.swap(true, Ordering::SeqCst) {
                return;
            }
            let app = app.clone();
            std::thread::spawn(move || {
                if SERVER_STARTED.load(Ordering::SeqCst) {
                    let ws_port = CURRENT_WS_PORT.load(Ordering::SeqCst);
                    if let Err(error) = export_current_session_via_http(ws_port) {
                        eprintln!("session export during Tauri shutdown failed: {error}");
                    }
                }
                app.exit(0);
            });
        }
    });
}

fn start_log_server(
    app: tauri::AppHandle,
    config_path: PathBuf,
    config: embed_log_core::config::AppConfig,
) -> Result<(), String> {
    if SERVER_STARTED.swap(true, Ordering::SeqCst) {
        return Ok(());
    }

    let ws_port = config.server.ws_port;
    CURRENT_WS_PORT.store(ws_port, Ordering::SeqCst);
    let frontend_dir = resolve_frontend_dir(&app);
    let logs_root = embed_log_core::config::resolve_logs_root(&config_path, &config.logs.dir);
    let demo_traffic = std::env::var_os("EMBED_LOG_DEMO_TRAFFIC").is_some();
    if demo_traffic {
        if let Err(error) = prepare_demo_file_sources(&config) {
            eprintln!("Demo traffic disabled: {error}");
        }
    }

    tauri::async_runtime::spawn(async move {
        if demo_traffic {
            spawn_demo_traffic(&config);
        }
        let server = LogServer::new(config, frontend_dir, logs_root).with_config_path(config_path);
        if let Err(e) = server.run().await {
            eprintln!("LogServer error: {e}");
        }
        app.exit(0);
    });

    Ok(())
}

fn show_config_error(window: &tauri::WebviewWindow, config_path: &std::path::Path, message: &str) {
    let message = message.replace('\\', "\\\\").replace('`', "\\`");
    let path = config_path
        .display()
        .to_string()
        .replace('\\', "\\\\")
        .replace('`', "\\`");
    let _ = window.eval(format!(
        "document.body.innerHTML = `<main style=\"font:14px system-ui;padding:24px;max-width:760px\"><h1>embed-log config error</h1><p><code>{}</code></p><p>Config path: <code>{}</code></p></main>`;",
        message, path
    ));
}

fn show_onboarding(window: &tauri::WebviewWindow, app: tauri::AppHandle, config_path: PathBuf) {
    // The save handler writes the config (via core) and starts the Tauri
    // LogServer, so the browser's post-save redirect lands on a live server.
    let app_for_handler = app.clone();
    let save_handler: SaveHandler = Arc::new(move |path, draft| {
        let result = ob::save_quick_config(&path, &draft)?;
        let config = load_config(&path).map_err(|e| format!("generated config is invalid: {e}"))?;
        start_log_server(app_for_handler.clone(), path, config)?;
        Ok(result)
    });

    match ob::OnboardingServer::start(config_path, save_handler) {
        Ok(server) => {
            if let Ok(url) = tauri::Url::parse(&server.base_url) {
                let _ = window.navigate(url);
            }
            // Onboarding server runs on a random port until the process exits;
            // its save handler starts the LogServer and the JS redirects.
        }
        Err(error) => {
            eprintln!("failed to start onboarding page: {error}");
            let _ = window.eval(ob::onboarding_script());
        }
    }
}

fn resolve_config_path(app: &tauri::App) -> PathBuf {
    resolve_config_path_from(
        std::env::args(),
        std::env::var_os("EMBED_LOG_CONFIG_YML_PATH"),
        Some(app_default_config_path(app.handle())),
    )
}

fn resolve_config_path_from<I, S>(
    args: I,
    env_path: Option<std::ffi::OsString>,
    app_default: Option<PathBuf>,
) -> PathBuf
where
    I: IntoIterator<Item = S>,
    S: Into<String>,
{
    let args: Vec<String> = args.into_iter().map(Into::into).collect();
    for (i, arg) in args.iter().enumerate() {
        if (arg == "--config" || arg == "-c") && i + 1 < args.len() {
            return PathBuf::from(&args[i + 1]);
        }
    }
    if let Some(env_path) = env_path {
        return PathBuf::from(env_path);
    }
    let local = PathBuf::from("embed-log.yml");
    if local.exists() {
        return local;
    }
    app_default.unwrap_or(local)
}

fn app_default_config_path(app: &tauri::AppHandle) -> PathBuf {
    app.path()
        .app_config_dir()
        .unwrap_or_else(|_| std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .join("embed-log.yml")
}

fn resolve_frontend_dir(app: &tauri::AppHandle) -> PathBuf {
    if let Ok(resource_dir) = app.path().resource_dir() {
        let frontend = resource_dir.join("frontend");
        if frontend.join("index.html").exists() {
            return frontend;
        }
    }
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let frontend = cwd.join("frontend");
    if frontend.join("index.html").exists() {
        return frontend;
    }
    let parent_frontend = cwd.join("..").join("frontend");
    if parent_frontend.join("index.html").exists() {
        return parent_frontend;
    }
    frontend
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsString;

    #[test]
    fn config_resolution_uses_cli_then_embed_log_config_yml_path_then_default() {
        assert_eq!(
            resolve_config_path_from(
                ["embed-log-tauri", "--config", "flag.yml"],
                Some(OsString::from("env.yml")),
                Some(PathBuf::from("app-default.yml")),
            ),
            PathBuf::from("flag.yml")
        );
        assert_eq!(
            resolve_config_path_from(
                ["embed-log-tauri"],
                Some(OsString::from("env.yml")),
                Some(PathBuf::from("app-default.yml")),
            ),
            PathBuf::from("env.yml")
        );
        assert_eq!(
            resolve_config_path_from(
                ["embed-log-tauri"],
                None,
                Some(PathBuf::from("app-default.yml")),
            ),
            if PathBuf::from("embed-log.yml").exists() {
                PathBuf::from("embed-log.yml")
            } else {
                PathBuf::from("app-default.yml")
            }
        );
    }
}
