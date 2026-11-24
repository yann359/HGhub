use futures_util::future::BoxFuture;
use hghub_hpp::{
    network_policy,
    web_login::{LoginCompletion, LoginRequest, WebLoginProvider},
    PluginError, Result,
};
use serde_json::{json, Value};
use std::{
    cell::Cell,
    num::NonZeroIsize,
    rc::Rc,
    sync::{Arc, OnceLock},
    time::Duration,
};
use tokio_util::sync::CancellationToken;
use webview2_com::{
    take_pwstr, GetCookiesCompletedHandler,
    Microsoft::Web::WebView2::Win32::{ICoreWebView2Cookie, ICoreWebView2_2},
};
use windows::{
    core::{self as win, w, Interface, BOOL, HSTRING, PWSTR},
    Win32::{
        Foundation::{HINSTANCE, HWND, LPARAM, LRESULT, RECT, WPARAM},
        Graphics::Gdi::{GetStockObject, GetSysColorBrush, COLOR_BTNFACE, DEFAULT_GUI_FONT},
        System::{
            LibraryLoader::GetModuleHandleW,
            SystemServices::{SS_CENTERIMAGE, SS_ENDELLIPSIS, SS_NOPREFIX},
        },
        UI::{
            HiDpi::{GetDpiForSystem, GetDpiForWindow},
            WindowsAndMessaging::*,
        },
    },
};
use wry::{
    dpi::{PhysicalPosition, PhysicalSize},
    raw_window_handle::{Win32WindowHandle, WindowHandle},
    NewWindowResponse, PageLoadEvent, WebContext, WebView, WebViewBuilder, WebViewExtWindows,
};

pub struct WindowsLogin;
impl WindowsLogin {
    pub fn detect() -> Option<Self> {
        // ICoreWebView2_2 (CookieManager) shipped with Edge 88.
        let major = wry::webview_version()
            .ok()?
            .split('.')
            .next()?
            .parse::<u32>()
            .ok()?;
        (major >= 88).then_some(Self)
    }
}
impl WebLoginProvider for WindowsLogin {
    fn open(
        &self,
        _: String,
        request: Arc<LoginRequest>,
        cancel: CancellationToken,
    ) -> BoxFuture<'static, Result<LoginCompletion>> {
        Box::pin(async move {
            let (send, receive) = tokio::sync::oneshot::channel();
            std::thread::Builder::new()
                .name("hpp-web-login".into())
                .spawn(move || {
                    let _ = send.send(show(request, cancel));
                })
                .map_err(native_error)?;
            receive.await.map_err(native_error)?
        })
    }
}
fn native_error(e: impl std::fmt::Display) -> PluginError {
    PluginError::internal(format!("网页登录窗口出错：{e}"))
}

// Keep Chromium's raw domain. The public cookie::Cookie::domain() strips a domain cookie's leading dot.
fn cookie_fields(cookie: ICoreWebView2Cookie) -> win::Result<Value> {
    unsafe {
        let mut text = PWSTR::null();
        cookie.Name(&mut text)?;
        let name = take_pwstr(text);
        cookie.Value(&mut text)?;
        let value = take_pwstr(text);
        cookie.Domain(&mut text)?;
        let domain = take_pwstr(text);
        cookie.Path(&mut text)?;
        let path = take_pwstr(text);
        let (mut secure, mut http_only, mut session) =
            (BOOL::default(), BOOL::default(), BOOL::default());
        cookie.IsSecure(&mut secure)?;
        cookie.IsHttpOnly(&mut http_only)?;
        cookie.IsSession(&mut session)?;
        let mut expires = 0.0;
        cookie.Expires(&mut expires)?;
        Ok(
            json!({"name":name, "value":value, "domain":domain.trim_start_matches('.'),
            "hostOnly":!domain.starts_with('.'), "path":path, "secure":secure.as_bool(),
            "httpOnly":http_only.as_bool(), "expiresAt":(!session.as_bool()).then_some((expires * 1000.) as u64)}),
        )
    }
}

struct CookieRead {
    url: String,
    result: Rc<Cell<Option<Result<Vec<Value>>>>>,
}

impl CookieRead {
    fn start(view: &WebView, url: String) -> Result<Self> {
        let result = Rc::new(Cell::new(None));
        let ready = Rc::clone(&result);
        unsafe {
            let core: ICoreWebView2_2 = view
                .controller()
                .CoreWebView2()
                .and_then(|v| v.cast())
                .map_err(native_error)?;
            core.CookieManager()
                .and_then(|manager| {
                    manager.GetCookies(
                        &HSTRING::from(url.as_str()),
                        &GetCookiesCompletedHandler::create(Box::new(move |status, list| {
                            let cookies = status
                                .and_then(|()| {
                                    let list = list.ok_or_else(win::Error::empty)?;
                                    let mut count = 0;
                                    list.Count(&mut count)?;
                                    (0..count)
                                        .map(|i| cookie_fields(list.GetValueAtIndex(i)?))
                                        .collect()
                                })
                                .map_err(native_error);
                            ready.set(Some(cookies));
                            Ok(())
                        })),
                    )
                })
                .map_err(native_error)?;
        }
        Ok(Self { url, result })
    }
}

const CLOSE_ID: usize = 1;
const FINISH_LOGIN: u32 = WM_APP + 1;
const LAYOUT_LOGIN: u32 = WM_APP + 2;

// Only the outer pump touches the WebView. A synchronous Win32 callback can arrive
// during WebView2 initialization or cookie collection.
unsafe extern "system" fn login_window_proc(
    hwnd: HWND,
    message: u32,
    wp: WPARAM,
    lp: LPARAM,
) -> LRESULT {
    match message {
        WM_COMMAND if wp.0 == CLOSE_ID && lp.0 != 0 => {
            let _ = PostMessageW(Some(hwnd), FINISH_LOGIN, WPARAM(0), LPARAM(0));
        }
        WM_CLOSE => {
            let _ = PostMessageW(Some(hwnd), FINISH_LOGIN, WPARAM(0), LPARAM(0));
        }
        WM_SIZE => {
            let _ = PostMessageW(Some(hwnd), LAYOUT_LOGIN, WPARAM(0), LPARAM(0));
        }
        WM_DPICHANGED => {
            let rect = &*(lp.0 as *const RECT);
            let _ = SetWindowPos(
                hwnd,
                None,
                rect.left,
                rect.top,
                rect.right - rect.left,
                rect.bottom - rect.top,
                SWP_NOZORDER | SWP_NOACTIVATE,
            );
        }
        _ => return DefWindowProcW(hwnd, message, wp, lp),
    }
    LRESULT(0)
}

struct LoginWindow(HWND);
impl LoginWindow {
    fn new(caption: &str) -> Result<Self> {
        static CLASS: OnceLock<win::Result<()>> = OnceLock::new();
        let name = w!("HGhub.HppLogin");
        unsafe {
            let instance: HINSTANCE = GetModuleHandleW(None).map_err(native_error)?.into();
            CLASS
                .get_or_init(|| {
                    let class = WNDCLASSW {
                        lpfnWndProc: Some(login_window_proc),
                        hInstance: instance,
                        hCursor: LoadCursorW(None, IDC_ARROW)?,
                        hbrBackground: GetSysColorBrush(COLOR_BTNFACE),
                        lpszClassName: name,
                        ..Default::default()
                    };
                    if RegisterClassW(&class) == 0 {
                        return Err(win::Error::from_thread());
                    }
                    Ok(())
                })
                .as_ref()
                .map_err(native_error)?;
            let dpi = GetDpiForSystem() as i32;
            CreateWindowExW(
                Default::default(),
                name,
                &HSTRING::from(caption),
                WS_OVERLAPPEDWINDOW | WS_CLIPCHILDREN,
                CW_USEDEFAULT,
                CW_USEDEFAULT,
                720 * dpi / 96,
                760 * dpi / 96,
                None,
                None,
                Some(instance),
                None,
            )
            .map(Self)
            .map_err(native_error)
        }
    }
    fn handle(&self) -> WindowHandle<'_> {
        let raw = Win32WindowHandle::new(NonZeroIsize::new(self.0 .0 as isize).unwrap());
        // This RAII window outlives the child WebView and both are destroyed on this same STA.
        unsafe { WindowHandle::borrow_raw(raw.into()) }
    }
}
impl Drop for LoginWindow {
    fn drop(&mut self) {
        unsafe {
            let _ = DestroyWindow(self.0);
        }
    }
}

struct LoginBar {
    address: HWND,
    close: HWND,
}
impl LoginBar {
    fn new(parent: HWND, origin: &str) -> Result<Self> {
        // Both controls are children of the host window, outside the site's WebView.
        unsafe {
            let base = WS_CHILD | WS_VISIBLE;
            let address = CreateWindowExW(
                Default::default(),
                w!("STATIC"),
                &HSTRING::from(origin),
                base | WINDOW_STYLE(SS_CENTERIMAGE.0 | SS_ENDELLIPSIS.0 | SS_NOPREFIX.0),
                0,
                0,
                0,
                0,
                Some(parent),
                None,
                None,
                None,
            )
            .map_err(native_error)?;
            let close = CreateWindowExW(
                Default::default(),
                w!("BUTTON"),
                w!("关闭"),
                base | WS_TABSTOP,
                0,
                0,
                0,
                0,
                Some(parent),
                Some(HMENU(CLOSE_ID as _)),
                None,
                None,
            )
            .map_err(native_error)?;
            let font = WPARAM(GetStockObject(DEFAULT_GUI_FONT).0 as usize);
            for control in [address, close] {
                SendMessageW(control, WM_SETFONT, Some(font), None);
            }
            Ok(Self { address, close })
        }
    }

    fn layout(&self, hwnd: HWND, view: &WebView) -> Result<()> {
        let mut rect = RECT::default();
        unsafe { GetClientRect(hwnd, &mut rect) }.map_err(native_error)?;
        let (width, height) = (rect.right as u32, rect.bottom as u32);
        let dpi = unsafe { GetDpiForWindow(hwnd) } as f64;
        let px = |dip: f64| (dip * dpi / 96.0).round() as i32;
        let bar = px(44.0);
        let right = width as i32 - px(10.0);
        let button_width = px(76.0);
        unsafe {
            MoveWindow(
                self.address,
                px(12.0),
                0,
                (right - button_width - px(20.0)).max(0),
                bar,
                true,
            )
            .map_err(native_error)?;
            MoveWindow(
                self.close,
                (right - button_width).max(0),
                px(6.0),
                button_width,
                px(32.0),
                true,
            )
            .map_err(native_error)?;
        }
        view.set_bounds(wry::Rect {
            position: PhysicalPosition::new(0, bar).into(),
            size: PhysicalSize::new(width, height.saturating_sub(bar as u32)).into(),
        })
        .map_err(native_error)
    }
}

fn show(request: Arc<LoginRequest>, cancel: CancellationToken) -> Result<LoginCompletion> {
    let window = LoginWindow::new(request.title.as_deref().unwrap_or("网页登录"))?;
    let handle = window.handle();
    let dir = tempfile::Builder::new()
        .prefix("hghub-login-")
        .tempdir()
        .map_err(native_error)?;
    let mut context = WebContext::new(Some(dir.path().into()));
    let navigations = Rc::new(Cell::new(0_u32));
    let loaded = navigations.clone();
    let view = WebViewBuilder::new_with_web_context(&mut context)
        .with_incognito(true)
        .with_devtools(false)
        .with_download_started_handler(|_, _| false)
        .with_new_window_req_handler(|_, _| NewWindowResponse::Deny)
        .with_navigation_handler(|raw| {
            network_policy::parse_target(&raw).is_ok_and(|url| url.scheme() == "https")
        })
        .with_on_page_load_handler(move |event, raw| {
            if matches!(event, PageLoadEvent::Finished) && raw.starts_with("https://") {
                loaded.set(loaded.get().saturating_add(1));
            }
        })
        .build_as_child(&handle)
        .map_err(native_error)?;
    // Disable native messaging and password persistence before the first remote navigation.
    unsafe {
        use webview2_com::Microsoft::Web::WebView2::Win32::ICoreWebView2Settings4;
        let settings = view
            .controller()
            .CoreWebView2()
            .and_then(|core| core.Settings())
            .map_err(native_error)?;
        settings
            .SetIsWebMessageEnabled(false)
            .map_err(native_error)?;
        settings
            .SetAreHostObjectsAllowed(false)
            .map_err(native_error)?;
        if let Ok(settings) = settings.cast::<ICoreWebView2Settings4>() {
            settings
                .SetIsPasswordAutosaveEnabled(false)
                .map_err(native_error)?;
            settings
                .SetIsGeneralAutofillEnabled(false)
                .map_err(native_error)?;
        }
    }
    let start = tauri::Url::parse(&request.url).map_err(native_error)?;
    let mut origin = start.origin();
    let bar = LoginBar::new(window.0, &origin.ascii_serialization())?;
    bar.layout(window.0, &view)?;
    if cancel.is_cancelled() {
        return Err(PluginError::cancelled());
    }
    view.load_url(&request.url).map_err(native_error)?;
    unsafe {
        if SetTimer(Some(window.0), 1, 350, None) == 0 {
            return Err(native_error(win::Error::from_thread()));
        }
        let _ = ShowWindow(window.0, SW_SHOW);
        let _ = SetForegroundWindow(window.0);
    }
    let result = (|| -> Result<LoginCompletion> {
        let mut read: Option<CookieRead> = None;
        let mut closing = false;
        loop {
            let mut msg = MSG::default();
            let status = unsafe { GetMessageW(&mut msg, None, 0, 0) }.0;
            if status == -1 {
                return Err(native_error(win::Error::from_thread()));
            }
            if status == 0 || cancel.is_cancelled() {
                return Err(PluginError::cancelled());
            }
            let ours = msg.hwnd == window.0;
            let close = ours && msg.message == FINISH_LOGIN && !closing;
            let poll = ours && msg.message == WM_TIMER && !closing && read.is_none();
            if close || poll {
                if close {
                    closing = true;
                    unsafe {
                        let _ = ShowWindow(window.0, SW_HIDE);
                    }
                    // Take a fresh snapshot at the click even when a previous poll is pending.
                    read = None;
                }
                let raw = view.url().map_err(native_error)?;
                if !raw.is_empty() {
                    let page = tauri::Url::parse(&raw).map_err(native_error)?;
                    if page.scheme() == "https" && page.origin() != origin {
                        origin = page.origin();
                        unsafe {
                            SetWindowTextW(
                                bar.address,
                                &HSTRING::from(origin.ascii_serialization()),
                            )
                        }
                        .map_err(native_error)?;
                    }
                    if request.matches_page(&page) {
                        read = Some(CookieRead::start(&view, raw)?);
                    }
                }
                if closing && read.is_none() {
                    return Err(PluginError::cancelled());
                }
            } else if ours && msg.message == LAYOUT_LOGIN {
                bar.layout(window.0, &view)?;
            } else {
                unsafe {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
            if let Some(cookies) = read.as_ref().and_then(|r| r.result.take()) {
                let raw = read.take().unwrap().url;
                let cookies = cookies?;
                if request.matches_cookies(&cookies)
                    && (!cookies.is_empty() || navigations.get() >= 2 || raw != start.as_str())
                    && view.url().map_err(native_error)? == raw
                {
                    return Ok(LoginCompletion {
                        final_url: raw,
                        cookies,
                    });
                }
                if closing {
                    return Err(PluginError::cancelled());
                }
            }
        }
    })();
    drop(view);
    drop(context);
    drop(window);
    // Chromium releases its file handles shortly after closing the last controller.
    let path = dir.path().to_owned();
    if dir.close().is_err() {
        std::thread::spawn(move || {
            for _ in 0..30 {
                std::thread::sleep(Duration::from_secs(1));
                match std::fs::remove_dir_all(&path) {
                    Ok(()) => return,
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => return,
                    Err(_) => (),
                }
            }
            eprintln!("网页登录临时目录仍被系统占用：{}", path.display());
        });
    }
    result
}
