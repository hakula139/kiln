//! Dev server with file watching, auto-rebuild, and live reload.

use std::convert::Infallible;
use std::fs;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use axum::Router;
use axum::body::Body;
use axum::extract::State;
use axum::http::header;
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use http_body_util::BodyExt;
use notify::RecursiveMode;
use tokio::sync::{broadcast, mpsc};
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;
use tower::ServiceExt;
use tower_http::services::ServeDir;

use crate::config::Config;

/// SSE endpoint path — prefixed to avoid conflicts with site content.
const LIVE_RELOAD_PATH: &str = "/__kiln_live_reload";

/// Debounce duration for file watcher events.
const DEBOUNCE: Duration = Duration::from_millis(100);

/// JavaScript snippet injected before `</body>` in HTML responses.
///
/// Reconnects after 1 second on error rather than relying on
/// `EventSource`'s default backoff, ensuring fast recovery when the
/// server restarts.
const LIVE_RELOAD_SCRIPT: &str = r#"
<script>
(function () {
  const connect = () => {
    const source = new EventSource("/__kiln_live_reload");
    source.addEventListener("reload", () => window.location.reload());
    source.onerror = () => {
      source.close();
      setTimeout(connect, 1000);
    };
  };
  connect();
})();
</script>
"#;

/// Starts the dev server with file watching and live reload.
///
/// Performs an initial build, then serves the output directory while
/// watching source files for changes. Blocks until Ctrl+C.
///
/// # Errors
///
/// Returns an error if the initial build fails, the server cannot bind,
/// or file watching cannot be initialized.
///
/// # Panics
///
/// Panics if the Ctrl+C signal handler cannot be installed.
#[tokio::main]
pub async fn serve(root: &Path, port: u16, open: bool) -> Result<()> {
    eprintln!("Building site...");
    crate::build(root).context("initial build failed")?;

    let config = Config::load(root).context("failed to load config")?;
    // output_dir is captured once; if config.toml changes output_dir at runtime,
    // the server must be restarted (same limitation as theme directory watching).
    let output_dir = root.join(&config.output_dir);

    let (reload_tx, _) = broadcast::channel::<()>(16);

    let (watch_tx, watch_rx) = mpsc::unbounded_channel();
    // Watcher must stay alive for the duration of the server; dropping it stops watching.
    let _watcher = setup_watcher(root, &config, watch_tx)?;

    let rebuild_root = root.to_owned();
    let rebuild_tx = reload_tx.clone();
    tokio::spawn(watch_loop(rebuild_root, watch_rx, rebuild_tx));

    let app = build_router(&output_dir, reload_tx);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind to port {port} (is it already in use?)"))?;

    let url = format!("http://localhost:{port}");
    eprintln!("\nServing at {url} (Press Ctrl+C to stop)");
    eprint!("Watching: config.toml, content/, templates/, static/");
    if let Some(ref theme) = config.theme {
        eprint!(", themes/{theme}/");
    }
    eprintln!();

    if open && let Err(e) = open::that(&url) {
        eprintln!("Failed to open browser: {e}");
    }

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("server error")?;

    eprintln!("\nShutting down.");
    Ok(())
}

/// A path to watch and whether to recurse into subdirectories.
struct WatchEntry {
    path: PathBuf,
    recursive: bool,
}

/// Initializes the file watcher on source directories and config.
fn setup_watcher(
    root: &Path,
    config: &Config,
    event_tx: mpsc::UnboundedSender<()>,
) -> Result<notify::RecommendedWatcher> {
    use notify::Watcher;

    let mut watcher =
        notify::recommended_watcher(move |res: notify::Result<notify::Event>| match res {
            Ok(event)
                if matches!(
                    event.kind,
                    notify::EventKind::Create(_)
                        | notify::EventKind::Modify(_)
                        | notify::EventKind::Remove(_)
                ) =>
            {
                _ = event_tx.send(());
            }
            Ok(_) => {}
            Err(e) => tracing::warn!("file watcher error: {e}"),
        })
        .context("failed to initialize file watcher")?;

    for entry in watch_paths(root, config) {
        let mode = if entry.recursive {
            RecursiveMode::Recursive
        } else {
            RecursiveMode::NonRecursive
        };
        watcher
            .watch(&entry.path, mode)
            .with_context(|| format!("failed to watch {}", entry.path.display()))?;
    }

    Ok(watcher)
}

/// Computes which paths should be watched for changes.
///
/// Returns only paths that exist on disk. Directories that don't exist
/// (e.g., no `static/` folder) are silently skipped.
fn watch_paths(root: &Path, config: &Config) -> Vec<WatchEntry> {
    let mut paths = Vec::new();

    let config_path = root.join("config.toml");
    if config_path.is_file() {
        paths.push(WatchEntry {
            path: config_path,
            recursive: false,
        });
    }

    for dir in ["content", "templates", "static"] {
        let path = root.join(dir);
        if path.is_dir() {
            paths.push(WatchEntry {
                path,
                recursive: true,
            });
        }
    }

    // Watch the active theme directory. If the theme changes in config.toml,
    // the server must be restarted to pick up the new theme directory.
    if let Some(theme_dir) = config.theme_dir(root)
        && theme_dir.is_dir()
    {
        paths.push(WatchEntry {
            path: theme_dir,
            recursive: true,
        });
    }

    paths
}

/// Debounced rebuild loop: waits for watcher events, rebuilds, and notifies SSE clients.
async fn watch_loop(
    root: PathBuf,
    mut event_rx: mpsc::UnboundedReceiver<()>,
    reload_tx: broadcast::Sender<()>,
) {
    loop {
        if event_rx.recv().await.is_none() {
            break;
        }
        tokio::time::sleep(DEBOUNCE).await;
        while event_rx.try_recv().is_ok() {}

        eprintln!("\nRebuilding...");
        let root = root.clone();
        let result = tokio::task::spawn_blocking(move || safe_rebuild(&root)).await;

        match result {
            Ok(Ok(())) => {
                _ = reload_tx.send(());
            }
            Ok(Err(e)) => {
                eprintln!("Rebuild failed: {e:?}");
            }
            Err(e) => {
                eprintln!("Rebuild task panicked: {e}");
            }
        }
    }
}

/// Builds the site, preserving the previous output on failure.
///
/// `build()` calls `clean_output_dir()` which wipes the output directory before
/// writing. If the build then fails (e.g., template error), the server would be
/// left serving an empty directory. This wrapper backs up the previous output and
/// restores it if the build fails, so the last successful build remains available.
fn safe_rebuild(root: &Path) -> Result<()> {
    let config = Config::load(root).context("failed to load config")?;
    let output_dir = root.join(&config.output_dir);
    let backup_dir = root.join(format!("{}.prev", config.output_dir));

    if output_dir.exists() {
        if backup_dir.exists() {
            _ = fs::remove_dir_all(&backup_dir);
        }
        fs::rename(&output_dir, &backup_dir).context("failed to back up output directory")?;
    }

    match crate::build(root) {
        Ok(()) => {
            if backup_dir.exists() {
                _ = fs::remove_dir_all(&backup_dir);
            }
            Ok(())
        }
        Err(e) => {
            // Restore the backup so the server keeps serving the last good build.
            if backup_dir.exists() {
                _ = fs::remove_dir_all(&output_dir);
                _ = fs::rename(&backup_dir, &output_dir);
            }
            Err(e)
        }
    }
}

/// Creates the axum router with SSE live reload and static file serving.
fn build_router(output_dir: &Path, reload_tx: broadcast::Sender<()>) -> Router {
    let serve_dir = ServeDir::new(output_dir).append_index_html_on_directories(true);

    Router::new()
        .route(LIVE_RELOAD_PATH, get(sse_handler))
        .fallback(move |request: axum::extract::Request| {
            let sd = serve_dir.clone();
            async move { serve_with_inject(sd, request).await }
        })
        .with_state(reload_tx)
}

/// SSE endpoint that streams reload events to connected browsers.
async fn sse_handler(State(tx): State<broadcast::Sender<()>>) -> impl IntoResponse {
    let rx = tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result| match result {
        Ok(()) => Some(Ok::<_, Infallible>(
            Event::default().event("reload").data("reload"),
        )),
        Err(_) => None,
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// Serves a static file via `ServeDir` and injects the live reload script
/// into HTML responses. Non-HTML responses pass through untouched.
async fn serve_with_inject(serve_dir: ServeDir, request: axum::extract::Request) -> Response {
    let response = serve_dir
        .oneshot(request)
        .await
        .expect("ServeDir is infallible")
        .map(Body::new);

    let is_html = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.starts_with("text/html"));

    if !is_html {
        return response;
    }

    let (mut parts, body) = response.into_parts();
    let Ok(collected) = body.collect().await else {
        tracing::warn!("failed to collect response body for live reload injection");
        return Response::from_parts(parts, Body::empty());
    };
    let bytes = collected.to_bytes();
    let html = String::from_utf8_lossy(&bytes);

    let modified = inject_script(&html);
    parts.headers.remove(header::CONTENT_LENGTH);
    Response::from_parts(parts, Body::from(modified))
}

/// Injects the live reload script before `</body>` in HTML content.
/// If no `</body>` is found, appends the script at the end.
fn inject_script(html: &str) -> String {
    // to_ascii_lowercase only changes single-byte ASCII chars, so byte
    // positions in the lowercased string are valid for slicing the original.
    if let Some(pos) = html.to_ascii_lowercase().rfind("</body>") {
        let mut result = String::with_capacity(html.len() + LIVE_RELOAD_SCRIPT.len());
        result.push_str(&html[..pos]);
        result.push_str(LIVE_RELOAD_SCRIPT);
        result.push_str(&html[pos..]);
        result
    } else {
        format!("{html}{LIVE_RELOAD_SCRIPT}")
    }
}

/// Waits for Ctrl+C to signal graceful shutdown.
async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install Ctrl+C handler");
}

#[cfg(test)]
mod tests {
    use std::fs;

    use indoc::indoc;

    use axum::http::{Request, StatusCode};

    use super::*;
    use crate::test_utils::copy_templates;

    // -- watch_paths --

    #[test]
    fn watch_paths_all_dirs_present() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("content")).unwrap();
        fs::create_dir(root.path().join("templates")).unwrap();
        fs::create_dir(root.path().join("static")).unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();

        let config: Config = toml::from_str("").unwrap();
        let paths = watch_paths(root.path(), &config);

        assert_eq!(paths.len(), 4);
        assert!(paths[0].path.ends_with("config.toml") && !paths[0].recursive);
        assert!(paths[1].path.ends_with("content") && paths[1].recursive);
        assert!(paths[2].path.ends_with("templates") && paths[2].recursive);
        assert!(paths[3].path.ends_with("static") && paths[3].recursive);
    }

    #[test]
    fn watch_paths_missing_dirs_skipped() {
        let root = tempfile::tempdir().unwrap();
        fs::create_dir(root.path().join("content")).unwrap();
        // No templates/, static/, or config.toml

        let config: Config = toml::from_str("").unwrap();
        let paths = watch_paths(root.path(), &config);

        assert_eq!(paths.len(), 1);
        assert!(paths[0].path.ends_with("content"));
    }

    #[test]
    fn watch_paths_with_theme() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();
        let theme_dir = root.path().join("themes").join("my-theme");
        fs::create_dir_all(&theme_dir).unwrap();

        let config: Config = toml::from_str(r#"theme = "my-theme""#).unwrap();
        let paths = watch_paths(root.path(), &config);

        let theme_entry = paths.iter().find(|e| e.path.ends_with("my-theme"));
        assert!(theme_entry.is_some(), "should include theme directory");
        assert!(theme_entry.unwrap().recursive);
    }

    #[test]
    fn watch_paths_without_theme() {
        let root = tempfile::tempdir().unwrap();
        fs::write(root.path().join("config.toml"), "").unwrap();

        let config: Config = toml::from_str("").unwrap();
        let paths = watch_paths(root.path(), &config);

        let theme_entry = paths
            .iter()
            .find(|e| e.path.to_string_lossy().contains("themes"));
        assert!(
            theme_entry.is_none(),
            "should not include any theme directory"
        );
    }

    // -- safe_rebuild --

    /// Creates a minimal site that builds successfully.
    fn setup_site(root: &Path) {
        fs::write(root.join("config.toml"), "").unwrap();
        copy_templates(&root.join("templates"));
        let page_dir = root.join("content").join("posts").join("hello");
        fs::create_dir_all(&page_dir).unwrap();
        fs::write(
            page_dir.join("index.md"),
            indoc! {r#"
                +++
                title = "Hello"
                +++
                Body
            "#},
        )
        .unwrap();
    }

    #[test]
    fn safe_rebuild_success_cleans_backup() {
        let root = tempfile::tempdir().unwrap();
        setup_site(root.path());

        // First build to create output.
        crate::build(root.path()).unwrap();
        assert!(root.path().join("public").exists());

        // Rebuild should succeed and clean up the backup.
        safe_rebuild(root.path()).unwrap();
        assert!(root.path().join("public").exists());
        assert!(!root.path().join("public.prev").exists());
    }

    #[test]
    fn safe_rebuild_failure_restores_backup() {
        let root = tempfile::tempdir().unwrap();
        setup_site(root.path());

        // First build to create output with known content.
        crate::build(root.path()).unwrap();
        let output = root.path().join("public").join("hello").join("index.html");
        let original = fs::read_to_string(&output).unwrap();

        // Break the template so the next build fails.
        fs::write(
            root.path().join("templates").join("post.html"),
            "{% invalid %}",
        )
        .unwrap();

        assert!(safe_rebuild(root.path()).is_err());

        // Previous output should be restored.
        let restored = fs::read_to_string(&output).unwrap();
        assert_eq!(
            restored, original,
            "should restore previous output on failure"
        );
        assert!(!root.path().join("public.prev").exists());
    }

    #[test]
    fn safe_rebuild_no_existing_output() {
        let root = tempfile::tempdir().unwrap();
        setup_site(root.path());

        // No prior build — output dir doesn't exist yet.
        assert!(!root.path().join("public").exists());

        safe_rebuild(root.path()).unwrap();
        assert!(root.path().join("public").exists());
    }

    // -- build_router --

    /// Creates a router backed by a directory of static files.
    fn setup_router(dir: &Path) -> Router {
        let (tx, _) = broadcast::channel::<()>(16);
        build_router(dir, tx)
    }

    /// Collects a response body into a string.
    async fn collect_body(response: Response) -> String {
        let bytes = response.into_body().collect().await.unwrap().to_bytes();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    #[tokio::test]
    async fn build_router_injects_script_into_html() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("page.html"),
            "<html><body><p>Hello</p></body></html>",
        )
        .unwrap();

        let app = setup_router(dir.path());
        let response = app
            .oneshot(Request::get("/page.html").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = collect_body(response).await;
        assert!(
            body.contains("<p>Hello</p>"),
            "should preserve original content"
        );
        assert!(
            body.contains(LIVE_RELOAD_SCRIPT),
            "should inject live reload script into HTML"
        );
    }

    #[tokio::test]
    async fn build_router_no_inject_for_non_html() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("style.css"), "body { color: red; }").unwrap();

        let app = setup_router(dir.path());
        let response = app
            .oneshot(Request::get("/style.css").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = collect_body(response).await;
        assert_eq!(body, "body { color: red; }");
    }

    #[tokio::test]
    async fn build_router_serves_directory_index() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(
            dir.path().join("index.html"),
            "<html><body>Home</body></html>",
        )
        .unwrap();

        let app = setup_router(dir.path());
        let response = app
            .oneshot(Request::get("/").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let body = collect_body(response).await;
        assert!(body.contains("Home"), "should serve index.html for /");
        assert!(
            body.contains(LIVE_RELOAD_SCRIPT),
            "should inject into directory index"
        );
    }

    #[tokio::test]
    async fn build_router_sse_endpoint() {
        let dir = tempfile::tempdir().unwrap();
        let (tx, _) = broadcast::channel::<()>(16);
        let app = build_router(dir.path(), tx);

        let response = app
            .oneshot(Request::get(LIVE_RELOAD_PATH).body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        let content_type = response
            .headers()
            .get(header::CONTENT_TYPE)
            .unwrap()
            .to_str()
            .unwrap();
        assert!(
            content_type.starts_with("text/event-stream"),
            "should return text/event-stream, got: {content_type}"
        );
    }

    // -- inject_script --

    #[test]
    fn inject_script_before_body_close() {
        let html = "<html><body><p>Hello</p></body></html>";
        let result = inject_script(html);
        assert!(
            result.contains(LIVE_RELOAD_SCRIPT),
            "should contain live reload script"
        );
        assert!(
            result.contains(&format!("{LIVE_RELOAD_SCRIPT}</body>")),
            "script should be injected before </body>, got:\n{result}"
        );
    }

    #[test]
    fn inject_script_case_insensitive() {
        let html = "<html><body><p>Hello</p></BODY></html>";
        let result = inject_script(html);
        assert!(
            result.contains(&format!("{LIVE_RELOAD_SCRIPT}</BODY>")),
            "should handle uppercase </BODY>, got:\n{result}"
        );
    }

    #[test]
    fn inject_script_no_body_tag() {
        let html = "<html><p>Hello</p></html>";
        let result = inject_script(html);
        assert!(
            result.ends_with(LIVE_RELOAD_SCRIPT),
            "should append script when no </body>, got:\n{result}"
        );
    }
}
