//! Windows shell integration: the binary targets the windows subsystem (no
//! console), so a running server surfaces through a notification-area icon with
//! an Open/Exit menu.

use std::path::PathBuf;
use std::sync::mpsc;
use std::thread::JoinHandle;
use std::time::Duration;

use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
#[cfg(not(debug_assertions))]
use windows_sys::Win32::System::Console::FreeConsole;
use windows_sys::Win32::System::Console::{AttachConsole, GetConsoleWindow, ATTACH_PARENT_PROCESS};
use windows_sys::Win32::System::StationsAndDesktops::{
    CloseDesktop, OpenInputDesktop, DESKTOP_READOBJECTS,
};
use windows_sys::Win32::System::Threading::GetCurrentThreadId;
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageW, MessageBoxW, PeekMessageW, PostThreadMessageW, TranslateMessage,
    MB_ICONERROR, MB_OK, MSG, PM_NOREMOVE, SW_SHOWNORMAL, WM_QUIT, WM_USER,
};

const TRAY_ICON_PNG: &[u8] = include_bytes!("../resources/windows/tray-icon.png");

pub struct Tray {
    thread_id: u32,
    handle: JoinHandle<()>,
}

/// Folders the tray menu opens in Explorer. Grouped because both are PathBuf —
/// positional parameters of the same type would invite a swapped call site.
pub struct TrayPaths {
    pub config_dir: PathBuf,
    pub logs_dir: PathBuf,
}

/// Reattaches to the parent terminal when launched from one, so --help and
/// startup errors have somewhere to print. The shell never waits on a
/// windows-subsystem binary and closing the terminal would kill an attached
/// process, so detach_console() cuts over before serving starts.
pub fn attach_parent_console() {
    unsafe {
        AttachConsole(ATTACH_PARENT_PROCESS);
    }
}

pub fn detach_console() {
    // debug builds stay on the console subsystem (crate attribute in main.rs):
    // there the console is the log output and detaching would strand it
    #[cfg(not(debug_assertions))]
    unsafe {
        FreeConsole();
    }
}

/// Last-resort error surface for a fatal startup error: without a console the
/// process would otherwise vanish without a word. No-op for terminal launches,
/// which already see the error printed.
pub fn show_startup_error(error: &anyhow::Error) {
    if unsafe { GetConsoleWindow() }.is_null() {
        let text = wide(&format!(
            "{error:#}\n\nLog files are in the \"logs\" folder of the data directory."
        ));
        let title = wide("KMReader failed to start");
        unsafe {
            MessageBoxW(
                std::ptr::null_mut(),
                text.as_ptr(),
                title.as_ptr(),
                MB_ICONERROR | MB_OK,
            );
        }
    }
}

/// Returns None in non-interactive sessions (Windows service, headless task):
/// there is no shell to show the icon in, and the server runs unaffected.
pub fn start(
    port: u16,
    paths: TrayPaths,
    shutdown: tokio::sync::watch::Sender<bool>,
) -> Option<Tray> {
    let interactive = unsafe {
        let desktop = OpenInputDesktop(0, 0, DESKTOP_READOBJECTS);
        if desktop.is_null() {
            false
        } else {
            CloseDesktop(desktop);
            true
        }
    };
    if !interactive {
        return None;
    }
    let (ready_tx, ready_rx) = mpsc::channel();
    let handle = std::thread::Builder::new()
        .name("tray".into())
        .spawn(move || run(port, paths, shutdown, ready_tx))
        .ok()?;
    match ready_rx.recv_timeout(Duration::from_secs(10)) {
        Ok(Ok(thread_id)) => Some(Tray { thread_id, handle }),
        Ok(Err(e)) => {
            tracing::warn!("system tray disabled: {e}");
            let _ = handle.join();
            None
        }
        Err(_) => {
            tracing::warn!("system tray disabled: initialization timed out");
            None
        }
    }
}

impl Tray {
    /// Quits the tray thread's message loop and joins it, so the icon is removed
    /// from the notification area instead of lingering as a ghost until hovered.
    pub fn stop(self) {
        unsafe {
            PostThreadMessageW(self.thread_id, WM_QUIT, 0, 0);
        }
        let _ = self.handle.join();
    }
}

fn run(
    port: u16,
    paths: TrayPaths,
    shutdown: tokio::sync::watch::Sender<bool>,
    ready: mpsc::Sender<Result<u32, String>>,
) {
    let open_item = MenuItem::with_id("open", "Open KMReader", true, None);
    let config_item = MenuItem::with_id("config", "Open configuration directory", true, None);
    let logs_item = MenuItem::with_id("logs", "Open logs folder", true, None);
    let exit_item = MenuItem::with_id("exit", "Exit", true, None);
    let menu = Menu::new();
    let _ = menu.append(&open_item);
    let config_target = wide_os(paths.config_dir.as_os_str());
    let _ = menu.append(&config_item);
    // without a console the log files are the only place startup/scan errors show
    // up, so the menu only offers the folder when file logging came up
    let logs_target = paths
        .logs_dir
        .is_dir()
        .then(|| wide_os(paths.logs_dir.as_os_str()));
    if logs_target.is_some() {
        let _ = menu.append(&logs_item);
    }
    let _ = menu.append(&PredefinedMenuItem::separator());
    let _ = menu.append(&exit_item);

    let tray = (|| {
        let icon = load_icon().map_err(|e| format!("tray icon decode: {e}"))?;
        TrayIconBuilder::new()
            .with_menu(Box::new(menu))
            // default true would pop the menu on left-button-up, making a plain
            // left click (our open-UI gesture) unreachable
            .with_menu_on_left_click(false)
            .with_tooltip("KMReader")
            .with_icon(icon)
            .build()
            .map_err(|e| e.to_string())
    })();
    let tray = match tray {
        Ok(tray) => tray,
        Err(e) => {
            let _ = ready.send(Err(e));
            return;
        }
    };

    MenuEvent::set_event_handler(Some(move |event: MenuEvent| match event.id.0.as_str() {
        "open" => shell_open(&wide(&format!("http://localhost:{port}/"))),
        "logs" => {
            if let Some(target) = &logs_target {
                shell_open(target);
            }
        }
        "config" => shell_open(&config_target),
        "exit" => {
            let _ = shutdown.send(true);
        }
        _ => {}
    }));
    TrayIconEvent::set_event_handler(Some(move |event: TrayIconEvent| {
        if let TrayIconEvent::Click {
            button: MouseButton::Left,
            button_state: MouseButtonState::Up,
            ..
        } = event
        {
            shell_open(&wide(&format!("http://localhost:{port}/")));
        }
    }));

    unsafe {
        let mut msg = MSG::default();
        // force the message queue into existence so stop() can post WM_QUIT to it
        PeekMessageW(
            &mut msg,
            std::ptr::null_mut(),
            WM_USER,
            WM_USER,
            PM_NOREMOVE,
        );
        let _ = ready.send(Ok(GetCurrentThreadId()));

        while GetMessageW(&mut msg, std::ptr::null_mut(), 0, 0) > 0 {
            TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
    drop(tray);
}

fn load_icon() -> Result<Icon, String> {
    let image = image::load_from_memory_with_format(TRAY_ICON_PNG, image::ImageFormat::Png)
        .map_err(|e| e.to_string())?
        .into_rgba8();
    let (width, height) = image.dimensions();
    Icon::from_rgba(image.into_raw(), width, height).map_err(|e| e.to_string())
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

fn wide_os(s: &std::ffi::OsStr) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    s.encode_wide().chain(std::iter::once(0)).collect()
}

fn shell_open(target: &[u16]) {
    let verb = wide("open");
    unsafe {
        let result = ShellExecuteW(
            std::ptr::null_mut(),
            verb.as_ptr(),
            target.as_ptr(),
            std::ptr::null(),
            std::ptr::null(),
            SW_SHOWNORMAL,
        );
        // the API's legacy return contract: values ≤ 32 are failure codes
        if result as isize <= 32 {
            tracing::warn!("ShellExecute open failed (code {})", result as isize);
        }
    }
}
