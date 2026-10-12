//! Dev server with file watching, auto-rebuild, and live reload.

use std::future::Future;
use std::net::SocketAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result};
use axum::Router;
use axum::body::Body;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{Request, State};
use axum::http::{Method, StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::get;
use indoc::indoc;
use itertools::Itertools;
use notify::{EventKind, RecursiveMode, Watcher};
use tokio::sync::{mpsc, watch};
use tower::ServiceExt;
use tower::util::MapRequest;
use tower_http::services::{ServeDir, ServeFile};
use tower_http::set_status::SetStatus;

use crate::build::{BuildOptions, build};
use crate::config::{self, Config};
use crate::output::guard::resolve_output_dir;

/// The custom 404 page, served without the validators and ranges meant for the missing path.
type NotFoundPage = MapRequest<ServeFile, fn(Request) -> Request>;

/// WebSocket endpoint path, prefixed to avoid conflicts with site content.
const LIVE_RELOAD_PATH: &str = "/__kiln_live_reload";

const DEBOUNCE: Duration = Duration::from_millis(100);

/// JavaScript snippet injected before `</body>` in HTML responses.
///
/// Uses a WebSocket because SSE connections count against Chrome's 6-connection-per-origin
/// HTTP/1.1 limit and leave zombie sockets on navigation that stall later requests.
const LIVE_RELOAD_SCRIPT: &str = indoc! {r#"

    <script>
    (function () {
      let ws = null;
      let retry = null;
      let active = true;
      const connect = () => {
        if (!active || ws) {
          return;
        }
        const url = (location.protocol === "https:" ? "wss://" : "ws://")
          + location.host + "/__kiln_live_reload";
        const socket = new WebSocket(url);
        ws = socket;
        socket.onmessage = (e) => {
          if (e.data === "reload") {
            window.location.reload();
          }
        };
        socket.onclose = () => {
          if (ws !== socket) {
            return;
          }
          ws = null;
          if (active) {
            retry = setTimeout(() => {
              retry = null;
              connect();
            }, 1000);
          }
        };
      };
      window.addEventListener("pagehide", () => {
        active = false;
        clearTimeout(retry);
        retry = null;
        const socket = ws;
        ws = null;
        if (socket) {
          socket.close();
        }
      });
      window.addEventListener("pageshow", () => {
        active = true;
        connect();
      });
      connect();
    })();
    </script>
"#};

/// Starts the dev server with file watching and live reload. Blocks until Ctrl+C.
///
/// # Errors
///
/// Returns an error if the initial build fails, the server cannot bind, or file watching cannot
/// be initialized.
///
/// # Panics
///
/// Panics if the Ctrl+C signal handler cannot be installed.
#[tokio::main]
pub async fn serve(root: &Path, port: u16, open: bool) -> Result<()> {
    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = tokio::net::TcpListener::bind(addr)
        .await
        .with_context(|| format!("failed to bind to port {port} (is it already in use?)"))?;

    serve_until(root, listener, open, shutdown_signal()).await
}

/// Builds the site, starts file watching, and serves until `shutdown` completes.
///
/// Separated from [`serve`] so tests can supply a pre-bound listener (port 0) and a controlled
/// shutdown signal.
async fn serve_until(
    root: &Path,
    listener: tokio::net::TcpListener,
    open: bool,
    shutdown: impl Future<Output = ()> + Send + 'static,
) -> Result<()> {
    let port = listener.local_addr()?.port();
    let base_url = config::localhost_url(port);
    let config = Config::load(root).context("failed to load config")?;
    // output_dir is captured once. If config.toml changes it, the server must be restarted.
    let output_dir = resolve_output_dir(root, &config, None)?;

    eprintln!("Building site...");
    rebuild(root, &base_url, &output_dir).context("initial build failed")?;

    let inputs = watch_paths(root, &config);
    let (event_tx, event_rx) = mpsc::unbounded_channel();
    // Watcher must stay alive for the duration of the server, since dropping it stops watching.
    let _watcher: notify::RecommendedWatcher =
        setup_watcher(&inputs, event_tx, notify::Config::default())?;

    let (reload_tx, _) = watch::channel(());
    tokio::spawn(watch_loop(
        root.to_owned(),
        base_url.clone(),
        output_dir.clone(),
        event_rx,
        reload_tx.clone(),
    ));

    let app = build_router(&output_dir, reload_tx);

    eprintln!("\nServing at {base_url} (Press Ctrl+C to stop)");
    eprintln!(
        "Watching: {}",
        inputs
            .iter()
            .map(|path| path.strip_prefix(root).unwrap_or(path).display())
            .join(", ")
    );

    if open && let Err(e) = open::that(&base_url) {
        eprintln!("Failed to open browser: {e}");
    }

    // `with_graceful_shutdown` would wait for WebSocket connections to close, hanging on Ctrl+C.
    // For a dev server, dropping connections immediately is acceptable.
    tokio::select! {
        result = axum::serve(listener, app).into_future() => {
            result.context("server error")?;
        }
        () = shutdown => {}
    }

    eprintln!("\nShutting down.");
    Ok(())
}

/// Initializes a file watcher on `inputs`.
///
/// Input files are watched through their parent directory because Linux file watches track the
/// original file, which atomic saves replace. Generic over the backend so tests can drive a
/// [`notify::PollWatcher`], which needs no platform event daemon and works under a build sandbox.
fn setup_watcher<W: Watcher>(
    inputs: &[PathBuf],
    event_tx: mpsc::UnboundedSender<()>,
    watcher_config: notify::Config,
) -> Result<W> {
    let inputs = inputs
        .iter()
        .map(|path| {
            path.canonicalize()
                .with_context(|| format!("failed to resolve watch path {}", path.display()))
        })
        .collect::<Result<Vec<_>>>()?;

    let filter = inputs.clone();
    let mut watcher = W::new(
        move |res: notify::Result<notify::Event>| match res {
            Ok(event) if is_input_change(&event, &filter) => _ = event_tx.send(()),
            Ok(_) => {}
            Err(e) => tracing::warn!("file watcher error: {e}"),
        },
        watcher_config,
    )
    .context("failed to initialize file watcher")?;

    for input in &inputs {
        let (path, mode) = match input.parent() {
            Some(parent) if input.is_file() => (parent, RecursiveMode::NonRecursive),
            _ => (input.as_path(), RecursiveMode::Recursive),
        };
        watcher
            .watch(path, mode)
            .with_context(|| format!("failed to watch {}", path.display()))?;
    }

    Ok(watcher)
}

/// Returns the existing site and theme build inputs.
fn watch_paths(root: &Path, config: &Config) -> Vec<PathBuf> {
    let site = config::SITE_INPUTS.iter().map(|name| root.join(name));
    // Changing the theme in config.toml requires a restart.
    let theme = config.theme_dir(root).into_iter().flat_map(|theme_dir| {
        config::THEME_INPUTS
            .iter()
            .map(move |name| theme_dir.join(name))
    });
    site.chain(theme).filter(|path| path.exists()).collect()
}

/// Reports whether `event` changes a watched input, ignoring siblings such as the output and
/// staging directories that share an input file's parent.
fn is_input_change(event: &notify::Event, inputs: &[PathBuf]) -> bool {
    matches!(
        event.kind,
        EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
    ) && event
        .paths
        .iter()
        .any(|path| inputs.iter().any(|input| path.starts_with(input)))
}

/// Debounced rebuild loop: waits for watcher events, rebuilds, and notifies WebSocket clients.
async fn watch_loop(
    root: PathBuf,
    base_url: String,
    output_dir: PathBuf,
    mut event_rx: mpsc::UnboundedReceiver<()>,
    reload_tx: watch::Sender<()>,
) {
    while event_rx.recv().await.is_some() {
        tokio::time::sleep(DEBOUNCE).await;
        while event_rx.try_recv().is_ok() {}

        eprintln!("\nRebuilding...");
        let root = root.clone();
        let base_url = base_url.clone();
        let output_dir = output_dir.clone();
        let result =
            tokio::task::spawn_blocking(move || rebuild(&root, &base_url, &output_dir)).await;

        match result {
            Ok(Ok(())) => reload_tx.send_replace(()),
            Ok(Err(e)) => eprintln!("Rebuild failed: {e:?}"),
            Err(e) => eprintln!("Rebuild task panicked: {e}"),
        }
    }
}

/// Builds the site into the served directory with dev-server URLs and no minification.
fn rebuild(root: &Path, base_url: &str, output_dir: &Path) -> Result<()> {
    build(
        root,
        &BuildOptions {
            base_url_override: Some(base_url),
            output_dir_override: Some(output_dir),
            ..Default::default()
        },
    )
}

/// Creates the axum router with WebSocket live reload and static file serving.
fn build_router(output_dir: &Path, reload_tx: watch::Sender<()>) -> Router {
    let serve_dir = ServeDir::new(output_dir)
        .append_index_html_on_directories(true)
        .not_found_service(NotFoundPage::new(
            ServeFile::new(output_dir.join("404.html")),
            without_validators,
        ));

    Router::new()
        .route(LIVE_RELOAD_PATH, get(ws_handler))
        .fallback(move |request| serve_request(serve_dir.clone(), request))
        .with_state(reload_tx)
}

/// Strips conditional and range headers, which would otherwise yield an empty or partial 404.
fn without_validators(mut request: Request) -> Request {
    for name in [
        header::IF_MATCH,
        header::IF_MODIFIED_SINCE,
        header::IF_NONE_MATCH,
        header::IF_RANGE,
        header::IF_UNMODIFIED_SINCE,
        header::RANGE,
    ] {
        request.headers_mut().remove(name);
    }
    request
}

/// WebSocket upgrade handler that sends `"reload"` after each successful rebuild.
async fn ws_handler(
    ws: WebSocketUpgrade,
    State(reload_tx): State<watch::Sender<()>>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| ws_relay(socket, reload_tx.subscribe()))
}

/// Relays reload notifications to a single WebSocket client until either side closes.
async fn ws_relay(mut socket: WebSocket, mut reload_rx: watch::Receiver<()>) {
    loop {
        tokio::select! {
            changed = reload_rx.changed() => {
                if changed.is_err()
                    || socket.send(Message::Text("reload".into())).await.is_err()
                {
                    break;
                }
            }
            msg = socket.recv() => {
                if msg.is_none_or(|r| r.is_err()) {
                    break;
                }
            }
        }
    }
}

/// Injects live reload into complete HTML pages while preserving static HTTP semantics.
async fn serve_request(
    serve_dir: ServeDir<SetStatus<NotFoundPage>>,
    mut request: Request,
) -> Response {
    // axum derives HEAD's Content-Length from the injected GET body, then drops the body.
    if request.method() == Method::HEAD {
        *request.method_mut() = Method::GET;
    }
    let Ok(response) = serve_dir.oneshot(request).await;
    let mut response = response.map(Body::new);

    // Force revalidation so live reload always serves fresh CSS / JS / images.
    response.headers_mut().insert(
        header::CACHE_CONTROL,
        header::HeaderValue::from_static("no-cache"),
    );

    let is_html = response
        .headers()
        .get(header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|ct| ct.starts_with("text/html"));
    if !is_html || !matches!(response.status(), StatusCode::OK | StatusCode::NOT_FOUND) {
        return response;
    }

    let (mut parts, body) = response.into_parts();
    let Ok(bytes) = axum::body::to_bytes(body, usize::MAX).await else {
        tracing::warn!("failed to collect response body for live reload injection");
        return StatusCode::INTERNAL_SERVER_ERROR.into_response();
    };
    let html = inject_script(&String::from_utf8_lossy(&bytes));
    parts.headers.remove(header::CONTENT_LENGTH);
    Response::from_parts(parts, Body::from(html))
}

/// Injects the live reload script before `</body>` in HTML content.
/// If no `</body>` is found, appends the script at the end.
fn inject_script(html: &str) -> String {
    // to_ascii_lowercase preserves byte positions, so slicing the original at `pos` is safe.
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

async fn shutdown_signal() {
    tokio::signal::ctrl_c()
        .await
        .expect("failed to install Ctrl+C handler");
}

#[cfg(test)]
mod tests {
    use std::fs;

    use axum::http::Request;
    use indoc::indoc;
    use notify::event::{CreateKind, ModifyKind, RenameMode};

    use super::*;
    use crate::test_utils::copy_templates;

    // ── serve_until ──

    #[tokio::test]
    async fn serve_until_serves_pages_assets_and_websocket() {
        let root = tempfile::tempdir().unwrap();
        setup_site(root.path());
        fs::create_dir(root.path().join("static")).unwrap();
        fs::write(root.path().join("static/style.css"), "body { color: red; }").unwrap();

        let (addr, shutdown_tx) = spawn_server(root.path()).await;
        wait_for_server(addr).await;

        let resp = reqwest::get(format!("http://{addr}/posts/hello/"))
            .await
            .unwrap();
        assert_eq!(resp.status(), 200);
        let body = resp.text().await.unwrap();
        assert!(body.contains("Hello"), "should contain page content");
        assert!(
            body.contains(LIVE_RELOAD_SCRIPT),
            "should inject live reload script"
        );

        let css = reqwest::get(format!("http://{addr}/style.css"))
            .await
            .unwrap();
        assert_eq!(css.status(), 200);
        assert_eq!(css.text().await.unwrap(), "body { color: red; }");

        let (ws, _) = tokio_tungstenite::connect_async(format!("ws://{addr}{LIVE_RELOAD_PATH}"))
            .await
            .unwrap();
        drop(ws);

        _ = shutdown_tx.send(());
    }

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

    /// Starts `serve_until` in a background task and returns the address and a shutdown sender.
    async fn spawn_server(root: &Path) -> (SocketAddr, tokio::sync::oneshot::Sender<()>) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (shutdown_tx, shutdown_rx) = tokio::sync::oneshot::channel::<()>();
        let root = root.to_owned();
        tokio::spawn(async move {
            _ = serve_until(&root, listener, false, async {
                _ = shutdown_rx.await;
            })
            .await;
        });
        (addr, shutdown_tx)
    }

    /// Polls until the server responds to an HTTP request.
    async fn wait_for_server(addr: SocketAddr) {
        let client = reqwest::Client::builder()
            .timeout(Duration::from_millis(200))
            .build()
            .unwrap();
        for _ in 0..50 {
            if client.get(format!("http://{addr}/")).send().await.is_ok() {
                return;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        panic!("server did not start within 5 seconds");
    }

    // ── setup_watcher ──

    #[tokio::test]
    async fn setup_watcher_sends_event_on_atomic_input_save() {
        for (input, original) in [
            ("content/test.md", None),
            ("i18n/en.toml", Some(r#"greeting = "Hello""#)),
            ("config.toml", Some(r#"theme = "test-theme""#)),
            ("themes/test-theme/theme.toml", Some("")),
        ] {
            let root = tempfile::tempdir().unwrap();
            for directory in ["content", "i18n", "themes/test-theme"] {
                fs::create_dir_all(root.path().join(directory)).unwrap();
            }
            fs::write(root.path().join("config.toml"), r#"theme = "test-theme""#).unwrap();
            fs::write(root.path().join("themes/test-theme/theme.toml"), "").unwrap();
            let file = root.path().join(input);
            if let Some(content) = original {
                fs::write(&file, content).unwrap();
            }

            let config: Config = toml::from_str(r#"theme = "test-theme""#).unwrap();
            let (tx, mut rx) = mpsc::unbounded_channel();
            let _watcher: notify::PollWatcher = setup_watcher(
                &watch_paths(root.path(), &config),
                tx,
                notify::Config::default()
                    .with_poll_interval(Duration::from_millis(50))
                    .with_compare_contents(true),
            )
            .unwrap();

            for output in ["public", ".kiln-build-abc123"] {
                fs::create_dir(root.path().join(output)).unwrap();
            }
            assert!(
                tokio::time::timeout(Duration::from_millis(300), rx.recv())
                    .await
                    .is_err(),
                "output siblings of config.toml should not trigger a rebuild"
            );

            let staged = root
                .path()
                .join(format!(".{}.tmp", file.file_name().unwrap().display()));
            fs::write(&staged, "# updated").unwrap();
            fs::rename(&staged, &file).unwrap();

            let event = tokio::time::timeout(Duration::from_secs(5), rx.recv())
                .await
                .unwrap_or_else(|_| panic!("should receive event after replacing {input}"));
            assert_eq!(event, Some(()), "{input}");
        }
    }

    // ── watch_paths ──

    #[test]
    fn watch_paths_lists_existing_site_and_theme_inputs() {
        for (theme, created, expected) in [
            (
                None,
                &[
                    "assets/",
                    "config.toml",
                    "content/",
                    "i18n/",
                    "public/",
                    "static/",
                    "templates/",
                ][..],
                &[
                    "assets",
                    "config.toml",
                    "content",
                    "i18n",
                    "static",
                    "templates",
                ][..],
            ),
            (None, &["content/", "themes/my-theme/assets/"], &["content"]),
            (
                Some("my-theme"),
                &[
                    "config.toml",
                    "themes/my-theme/assets/",
                    "themes/my-theme/theme.toml",
                    "themes/other/templates/",
                ],
                &[
                    "config.toml",
                    "themes/my-theme/assets",
                    "themes/my-theme/theme.toml",
                ],
            ),
        ] {
            let root = tempfile::tempdir().unwrap();
            for entry in created {
                let path = root.path().join(entry);
                if entry.ends_with('/') {
                    fs::create_dir_all(path).unwrap();
                } else {
                    fs::create_dir_all(path.parent().unwrap()).unwrap();
                    fs::write(path, "").unwrap();
                }
            }
            let config = Config {
                theme: theme.map(str::to_owned),
                ..Config::default()
            };

            assert_eq!(
                watch_paths(root.path(), &config),
                expected
                    .iter()
                    .map(|path| root.path().join(path))
                    .collect::<Vec<_>>(),
                "{created:?}"
            );
        }
    }

    // ── is_input_change ──

    #[test]
    fn is_input_change_matches_inputs_and_their_descendants() {
        let root = Path::new("/site");
        let inputs = [root.join("config.toml"), root.join("content")];
        let modify = EventKind::Modify(ModifyKind::Any);
        for (kind, paths, expected) in [
            (modify, &["config.toml"][..], true),
            (modify, &["content/posts/hello/index.md"], true),
            (
                EventKind::Create(CreateKind::File),
                &["content/new.md"],
                true,
            ),
            (
                EventKind::Modify(ModifyKind::Name(RenameMode::Both)),
                &["config.toml.tmp", "config.toml"],
                true,
            ),
            (modify, &["config.toml.swp"], false),
            (modify, &["public/index.html"], false),
            (
                EventKind::Create(CreateKind::Folder),
                &[".kiln-build-abc123/output"],
                false,
            ),
            (
                EventKind::Access(notify::event::AccessKind::Any),
                &["config.toml"],
                false,
            ),
        ] {
            let event = paths.iter().fold(notify::Event::new(kind), |event, path| {
                event.add_path(root.join(path))
            });
            assert_eq!(is_input_change(&event, &inputs), expected, "{event:?}");
        }
    }

    // ── watch_loop ──

    #[tokio::test]
    async fn watch_loop_triggers_reload_on_success() {
        let root = tempfile::tempdir().unwrap();
        setup_site(root.path());
        let (event_tx, mut reload_rx, _) = spawn_watch_loop(root.path());

        event_tx.send(()).unwrap();

        tokio::time::timeout(Duration::from_secs(5), reload_rx.changed())
            .await
            .expect("should receive reload within timeout")
            .expect("should receive reload after successful rebuild");
    }

    #[tokio::test]
    async fn watch_loop_stops_when_sender_dropped() {
        let root = tempfile::tempdir().unwrap();
        setup_site(root.path());
        let (event_tx, _, handle) = spawn_watch_loop(root.path());

        drop(event_tx);

        tokio::time::timeout(Duration::from_secs(2), handle)
            .await
            .expect("watch_loop should exit when event sender is dropped")
            .expect("watch_loop should exit without panicking");
    }

    #[tokio::test]
    async fn watch_loop_no_reload_on_failure() {
        let root = tempfile::tempdir().unwrap();
        setup_site(root.path());
        fs::write(
            root.path().join("templates").join("post.html"),
            "{% invalid %}",
        )
        .unwrap();
        let (event_tx, reload_rx, handle) = spawn_watch_loop(root.path());

        event_tx.send(()).unwrap();
        drop(event_tx);
        tokio::time::timeout(Duration::from_secs(5), handle)
            .await
            .expect("watch_loop should finish the failed rebuild within timeout")
            .expect("watch_loop should exit without panicking");

        assert!(
            !reload_rx.borrow().has_changed(),
            "should not send reload on failed rebuild"
        );
    }

    /// Spawns `watch_loop` over `root`, returning its event sender, a reload receiver and handle.
    fn spawn_watch_loop(
        root: &Path,
    ) -> (
        mpsc::UnboundedSender<()>,
        watch::Receiver<()>,
        tokio::task::JoinHandle<()>,
    ) {
        let (event_tx, event_rx) = mpsc::unbounded_channel();
        let (reload_tx, reload_rx) = watch::channel(());
        let handle = tokio::spawn(watch_loop(
            root.to_owned(),
            "http://localhost:0".to_owned(),
            root.join("public"),
            event_rx,
            reload_tx,
        ));
        (event_tx, reload_rx, handle)
    }

    // ── rebuild ──

    #[test]
    fn rebuild_recovers_after_css_import_failure() {
        let root = tempfile::tempdir().unwrap();
        setup_site(root.path());
        let css_dir = root.path().join("assets/css/_src");
        fs::create_dir_all(&css_dir).unwrap();
        fs::write(css_dir.join("style.css"), ".before { color: red; }").unwrap();
        let output_dir = root.path().join("public");
        let site_css = || {
            let [path]: [_; 1] = fs::read_dir(output_dir.join("assets/css"))
                .unwrap()
                .map(|entry| entry.unwrap().path())
                .collect::<Vec<_>>()
                .try_into()
                .unwrap();
            fs::read_to_string(path).unwrap()
        };
        rebuild(root.path(), "http://localhost:0", &output_dir).unwrap();
        let original = site_css();

        fs::write(css_dir.join("style.css"), r#"@import "missing.css";"#).unwrap();
        assert!(rebuild(root.path(), "http://localhost:0", &output_dir).is_err());
        assert_eq!(site_css(), original);

        fs::write(css_dir.join("missing.css"), ".recovered { color: blue; }").unwrap();
        rebuild(root.path(), "http://localhost:0", &output_dir).unwrap();
        assert!(site_css().contains(".recovered"));
    }

    #[test]
    fn rebuild_resolves_subpath_references_under_localhost() {
        let root = tempfile::tempdir().unwrap();
        setup_site(root.path());
        fs::write(
            root.path().join("config.toml"),
            r#"base_url = "https://example.com/blog/""#,
        )
        .unwrap();
        fs::write(
            root.path().join("content/posts/hello/index.md"),
            indoc! {r#"
                +++
                title = "Hello"
                +++
                ![Photo](/blog/assets/images/photo.png)
            "#},
        )
        .unwrap();
        for (path, content) in [
            ("assets/images/photo.png", "image"),
            (
                "assets/css/_src/style.css",
                ".photo { background: url(/blog/assets/images/photo.png); }",
            ),
        ] {
            let path = root.path().join(path);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, content).unwrap();
        }
        let output_dir = root.path().join("public");

        rebuild(root.path(), "http://localhost:0", &output_dir).unwrap();

        let names = |dir: &str| -> Vec<String> {
            fs::read_dir(output_dir.join(dir))
                .unwrap()
                .map(|entry| entry.unwrap().file_name().into_string().unwrap())
                .collect()
        };
        let [photo] = names("assets/images").try_into().unwrap();
        let [css] = names("assets/css").try_into().unwrap();
        assert!(
            photo.starts_with("photo.") && photo != "photo.png",
            "{photo}"
        );
        let html = fs::read_to_string(output_dir.join("posts/hello/index.html")).unwrap();
        assert!(
            html.contains(&format!(r#"src="/assets/images/{photo}""#)),
            "{html}"
        );
        let css = fs::read_to_string(output_dir.join("assets/css").join(css)).unwrap();
        assert!(css.contains(&format!("../images/{photo}")), "{css}");
    }

    // ── build_router ──

    #[tokio::test]
    async fn build_router_serves_html_with_live_reload() {
        let dir = tempfile::tempdir().unwrap();
        fs::create_dir(dir.path().join("about")).unwrap();
        for (path, content) in [
            ("index.html", "<html><body>Home</body></html>"),
            ("about/index.html", "<html><body>About</body></html>"),
            ("page.html", "<html><body><p>Hello</p></body></html>"),
        ] {
            fs::write(dir.path().join(path), content).unwrap();
        }

        let app = setup_router(dir.path());
        for (uri, content) in [
            ("/", "<html><body>Home"),
            ("/about/", "<html><body>About"),
            ("/page.html", "<html><body><p>Hello</p>"),
        ] {
            let response = app
                .clone()
                .oneshot(Request::get(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::OK, "{uri}");
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-cache");
            assert_eq!(
                collect_body(response).await,
                format!("{content}{LIVE_RELOAD_SCRIPT}</body></html>"),
                "{uri}"
            );
        }
    }

    #[tokio::test]
    async fn build_router_redirects_directory_without_trailing_slash() {
        let dir = tempfile::tempdir().unwrap();
        for name in ["about", "about us"] {
            let sub = dir.path().join(name);
            fs::create_dir(&sub).unwrap();
            fs::write(sub.join("index.html"), "<html><body>About</body></html>").unwrap();
        }

        let app = setup_router(dir.path());
        for (uri, location) in [
            ("/about", "/about/"),
            ("/about?x=1&x=2", "/about/?x=1&x=2"),
            (
                "/about?next=%2F%3F%3D&text=a+b",
                "/about/?next=%2F%3F%3D&text=a+b",
            ),
            ("/about?", "/about/?"),
            ("/about%20us?next=%2F", "/about%20us/?next=%2F"),
        ] {
            let response = app
                .clone()
                .oneshot(Request::get(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::TEMPORARY_REDIRECT, "{uri}");
            assert_eq!(
                response.headers().get(header::LOCATION).unwrap(),
                location,
                "{uri}"
            );
        }
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
        assert_eq!(response.headers()[header::CACHE_CONTROL], "no-cache");
        let body = collect_body(response).await;
        assert_eq!(body, "body { color: red; }");
    }

    #[tokio::test]
    async fn build_router_preserves_head_and_range_responses() {
        let dir = tempfile::tempdir().unwrap();
        let html = "<html><body>Hello</body></html>";
        fs::write(dir.path().join("page.html"), html).unwrap();
        fs::write(dir.path().join("404.html"), html).unwrap();
        let app = setup_router(dir.path());

        for (path, status) in [
            ("/page.html", StatusCode::OK),
            ("/missing", StatusCode::NOT_FOUND),
        ] {
            let response = app
                .clone()
                .oneshot(Request::head(path).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), status);
            assert_eq!(
                response.headers()[header::CONTENT_LENGTH],
                (html.len() + LIVE_RELOAD_SCRIPT.len()).to_string()
            );
            assert_eq!(collect_body(response).await, "");
        }
        let response = app
            .oneshot(
                Request::get("/page.html")
                    .header(header::RANGE, "bytes=0-5")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::PARTIAL_CONTENT);
        assert_eq!(
            response.headers()[header::CONTENT_RANGE],
            format!("bytes 0-5/{}", html.len())
        );
        assert_eq!(response.headers()[header::CONTENT_LENGTH], "6");
        assert_eq!(collect_body(response).await, "<html>");
    }

    #[tokio::test]
    async fn build_router_ws_relays_reload() {
        use futures_util::StreamExt;

        let dir = tempfile::tempdir().unwrap();
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let (reload_tx, _) = watch::channel(());
        let app = build_router(dir.path(), reload_tx.clone());
        tokio::spawn(async move { _ = axum::serve(listener, app).await });

        assert!(
            LIVE_RELOAD_SCRIPT.contains(&format!(r#""{LIVE_RELOAD_PATH}""#)),
            "the injected client should connect to the live reload route"
        );
        let url = format!("ws://{addr}{LIVE_RELOAD_PATH}");
        let (mut ws, _) = tokio_tungstenite::connect_async(&url).await.unwrap();

        // `ws_relay` subscribes after the upgrade completes, so a signal published before
        // that lands nowhere.
        for _ in 0..100 {
            if reload_tx.receiver_count() > 0 {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        reload_tx.send(()).expect("client should be subscribed");

        let msg = tokio::time::timeout(Duration::from_secs(5), ws.next())
            .await
            .expect("should receive message within timeout")
            .expect("stream should produce a message")
            .expect("message should not error");

        assert_eq!(
            msg.into_text().unwrap(),
            "reload",
            "should relay the reload signal to the client"
        );

        drop(ws);
        tokio::time::timeout(Duration::from_secs(5), async {
            while reload_tx.receiver_count() != 0 {
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .expect("client disconnect should release its reload subscription");
    }

    #[tokio::test]
    async fn build_router_ws_rejects_plain_http() {
        let dir = tempfile::tempdir().unwrap();

        let response = setup_router(dir.path())
            .oneshot(Request::get(LIVE_RELOAD_PATH).body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(
            response.status(),
            StatusCode::BAD_REQUEST,
            "plain HTTP to WS endpoint should be rejected"
        );
    }

    #[tokio::test]
    async fn build_router_not_found_serves_empty_or_full_custom_page() {
        let dir = tempfile::tempdir().unwrap();
        let app = setup_router(dir.path());
        let request = || Request::get("/nonexistent");

        let response = app
            .clone()
            .oneshot(request().body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        assert_eq!(collect_body(response).await, "");

        fs::write(
            dir.path().join("404.html"),
            "<html><body><h1>Not Found</h1></body></html>",
        )
        .unwrap();
        let response = app
            .clone()
            .oneshot(request().body(Body::empty()).unwrap())
            .await
            .unwrap();
        let etag = response.headers()[header::ETAG].clone();
        let last_modified = response.headers()[header::LAST_MODIFIED].clone();
        for (name, value) in [
            (None, None),
            (Some(header::IF_NONE_MATCH), Some(etag.clone())),
            (Some(header::IF_MODIFIED_SINCE), Some(last_modified.clone())),
            (Some(header::IF_MATCH), Some(r#""other""#.parse().unwrap())),
            (
                Some(header::IF_UNMODIFIED_SINCE),
                Some("Thu, 01 Jan 1970 00:00:00 GMT".parse().unwrap()),
            ),
            (Some(header::RANGE), Some("bytes=0-5".parse().unwrap())),
        ] {
            let mut request = request();
            if let (Some(name), Some(value)) = (&name, value) {
                request = request.header(name, value);
            }
            let response = app
                .clone()
                .oneshot(request.body(Body::empty()).unwrap())
                .await
                .unwrap();

            assert_eq!(response.status(), StatusCode::NOT_FOUND, "{name:?}");
            assert_eq!(response.headers()[header::CACHE_CONTROL], "no-cache");
            assert_eq!(
                collect_body(response).await,
                format!("<html><body><h1>Not Found</h1>{LIVE_RELOAD_SCRIPT}</body></html>"),
                "{name:?}"
            );
        }
    }

    /// Creates a router backed by a directory of static files.
    fn setup_router(dir: &Path) -> Router {
        let (tx, _) = watch::channel(());
        build_router(dir, tx)
    }

    async fn collect_body(response: Response) -> String {
        let bytes = axum::body::to_bytes(response.into_body(), usize::MAX)
            .await
            .unwrap();
        String::from_utf8(bytes.to_vec()).unwrap()
    }

    // ── inject_script ──

    #[test]
    fn inject_script_places_script_before_body_close_or_appends() {
        for (html, expected) in [
            (
                "<html><body>Hello</body></html>",
                format!("<html><body>Hello{LIVE_RELOAD_SCRIPT}</body></html>"),
            ),
            (
                "<html><body>Hello</BODY></html>",
                format!("<html><body>Hello{LIVE_RELOAD_SCRIPT}</BODY></html>"),
            ),
            (
                "<html>Hello</html>",
                format!("<html>Hello</html>{LIVE_RELOAD_SCRIPT}"),
            ),
        ] {
            assert_eq!(inject_script(html), expected);
        }
    }
}
