//! Pass a new directory for this offline WebView2 isolation probe. Use a disposable parent.
#[cfg(target_os = "windows")]
fn main() {
    use std::{
        error::Error,
        path::PathBuf,
        sync::mpsc,
        time::{Duration, Instant},
    };
    use tauri::{webview::PageLoadEvent, WebviewUrl, WebviewWindow, WebviewWindowBuilder};

    type Result<T> = std::result::Result<T, Box<dyn Error>>;
    const WAIT: Duration = Duration::from_secs(20);
    fn eval(window: &WebviewWindow, js: &str) -> Result<String> {
        let (send, recv) = mpsc::channel();
        window.eval_with_callback(js, move |value| {
            let _ = send.send(value);
        })?;
        Ok(recv.recv_timeout(WAIT)?)
    }
    fn probe(app: &tauri::AppHandle, root: PathBuf) -> Result<()> {
        std::fs::create_dir(&root)?;
        let mut views = Vec::new();
        let read = "[location.origin, document.cookie, localStorage.getItem('marker')]";
        let origin = "https://hppfixture.localhost";
        for label in ["host", "login-a", "login-b"] {
            let (send, recv) = mpsc::channel();
            let start = Instant::now();
            let window =
                WebviewWindowBuilder::new(app, label, WebviewUrl::External(origin.parse()?))
                    .data_directory(root.join(label))
                    .use_https_scheme(true)
                    .visible(false)
                    .on_page_load(move |_, page| {
                        if matches!(page.event(), PageLoadEvent::Finished) {
                            let _ = send.send(());
                        }
                    })
                    .build()?;
            recv.recv_timeout(WAIT)?;
            println!("{label}: {:.0?} to offline page load", start.elapsed());
            let initial: serde_json::Value = serde_json::from_str(&eval(&window, read)?)?;
            if initial != serde_json::json!([origin, "", null]) {
                return Err(format!("{label}: unexpected initial fixture state {initial}").into());
            }
            eval(&window, &format!("document.cookie='marker={label}; Secure'; localStorage.setItem('marker','{label}')"))?;
            views.push((label, window));
        }
        for (label, window) in &views {
            let actual: serde_json::Value = serde_json::from_str(&eval(window, read)?)?;
            if actual != serde_json::json!([origin, format!("marker={label}"), label]) {
                return Err(format!("{label}: fixture state changed {actual}").into());
            }
        }
        Ok(())
    }
    let root = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .expect("pass a fresh temp directory path");
    let mut context = tauri::generate_context!();
    context.config_mut().app.windows.clear();
    tauri::Builder::default()
        .register_uri_scheme_protocol("hppfixture", |_, _| tauri::http::Response::builder()
            .header("Content-Type", "text/html").body(b"<!doctype html><title>fixture</title>".to_vec()).unwrap())
        .setup(move |app| {
            let handle = app.handle().clone();
            std::thread::spawn(move || match probe(&handle, root) {
                Ok(()) => { println!("PASS: three WebView2 directories retain independent same-origin cookies and localStorage"); handle.exit(0); }
                Err(error) => { eprintln!("FAIL: {error}"); handle.exit(1); }
            });
            Ok(())
        })
        .run(context).expect("run WebView2 isolation fixture");
}

#[cfg(not(target_os = "windows"))]
fn main() {
    eprintln!("This fixture requires Windows WebView2");
    std::process::exit(1);
}
