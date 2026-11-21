//! System tray, so CyberCord keeps running (and keeps helping the network) when
//! the window is closed.
//!
//! * Linux: StatusNotifierItem over D-Bus (`ksni`, no GTK). Shown by Xfce's
//!   status tray, KDE, Cinnamon, MATE, and GNOME with the "AppIndicator and
//!   KStatusNotifierItem Support" extension (preinstalled on Ubuntu).
//! * Windows 10/11: the native notification-area icon (`tray-icon`).
//!
//! Menu: Open CyberCord · Scan for new users ✓ · Status ▸ · Notifications ✓ · Quit.

use std::sync::mpsc::Sender;

use eframe::egui;

/// User statuses, in menu order. `id` is what goes over the network.
pub const STATUSES: &[(&str, &str)] = &[
    ("", "st_none"),
    ("online", "st_online"),
    ("offline", "st_offline"),
    ("work", "st_work"),
    ("sleep", "st_sleep"),
    ("busy", "st_busy"),
    ("learning", "st_learning"),
    ("relax", "st_relax"),
    ("play", "st_play"),
    ("read", "st_read"),
];

pub fn status_index(id: &str) -> usize {
    STATUSES.iter().position(|(s, _)| *s == id).unwrap_or(0)
}

#[derive(Debug, Clone, PartialEq)]
pub enum TrayCmd {
    Open,
    SetScan(bool),
    SetStatus(usize),
    SetNotify(bool),
    Quit,
}

/// What the menu shows; labels are already translated.
#[derive(Clone, Debug, Default)]
pub struct TrayState {
    pub scan: bool,
    pub status: usize,
    pub notify: bool,
    pub tooltip: String,
    pub l_open: String,
    pub l_scan: String,
    pub l_status: String,
    pub l_notify: String,
    pub l_quit: String,
    pub l_statuses: Vec<String>,
}

/// Running tray; keep it alive. `update` refreshes labels/checkmarks.
pub struct Tray {
    #[cfg(target_os = "linux")]
    handle: ksni::blocking::Handle<SniTray>,
    #[cfg(windows)]
    win: win::WinTray,
}

impl Tray {
    pub fn update(&self, st: &TrayState) {
        #[cfg(target_os = "linux")]
        {
            let st = st.clone();
            self.handle.update(move |t| t.st = st);
        }
        #[cfg(windows)]
        self.win.update(st);
    }
}

/// Start the tray. None if the desktop has no tray (then the app minimizes
/// instead of hiding, so the window can always be brought back).
pub fn start(st: TrayState, tx: Sender<TrayCmd>, ctx: egui::Context, icon_png: &[u8]) -> Option<Tray> {
    if std::env::var("CYBERCORD_NO_TRAY").is_ok() {
        return None;
    }
    #[cfg(target_os = "linux")]
    {
        use ksni::blocking::TrayMethods;
        let tray = SniTray { st, tx, ctx, icon: argb_icons(icon_png) };
        match tray.spawn() {
            Ok(handle) => Some(Tray { handle }),
            Err(e) => {
                eprintln!("tray: not available ({e}); closing the window will minimize instead");
                None
            }
        }
    }
    #[cfg(windows)]
    {
        win::WinTray::new(st, tx, ctx, icon_png).map(|win| Tray { win })
    }
    #[cfg(not(any(target_os = "linux", windows)))]
    {
        let _ = (st, tx, ctx, icon_png);
        None
    }
}

fn send(tx: &Sender<TrayCmd>, ctx: &egui::Context, cmd: TrayCmd) {
    let _ = tx.send(cmd.clone());
    // Act right away even while the window is hidden (no frames are drawn then).
    match cmd {
        TrayCmd::Open => {
            ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
            ctx.send_viewport_cmd(egui::ViewportCommand::Minimized(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        TrayCmd::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        _ => {}
    }
    ctx.request_repaint();
}

/// PNG → ARGB32 (network byte order) at a few sizes, for SNI.
#[cfg(target_os = "linux")]
fn argb_icons(png: &[u8]) -> Vec<ksni::Icon> {
    let Ok(img) = image::load_from_memory(png) else { return Vec::new() };
    [22u32, 32, 48, 64]
        .iter()
        .map(|&s| {
            let rgba = img.resize_exact(s, s, image::imageops::FilterType::Triangle).to_rgba8();
            let mut data = Vec::with_capacity((s * s * 4) as usize);
            for p in rgba.pixels() {
                data.extend_from_slice(&[p[3], p[0], p[1], p[2]]);
            }
            ksni::Icon { width: s as i32, height: s as i32, data }
        })
        .collect()
}

#[cfg(target_os = "linux")]
struct SniTray {
    st: TrayState,
    tx: Sender<TrayCmd>,
    ctx: egui::Context,
    icon: Vec<ksni::Icon>,
}

#[cfg(target_os = "linux")]
impl ksni::Tray for SniTray {
    fn id(&self) -> String {
        "cybercord".into()
    }
    fn title(&self) -> String {
        "CyberCord".into()
    }
    fn icon_name(&self) -> String {
        String::new()
    }
    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        self.icon.clone()
    }
    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip { title: "CyberCord".into(), description: self.st.tooltip.clone(), icon_name: String::new(), icon_pixmap: Vec::new() }
    }
    fn category(&self) -> ksni::Category {
        ksni::Category::Communications
    }
    /// Left click opens the window.
    fn activate(&mut self, _x: i32, _y: i32) {
        send(&self.tx, &self.ctx, TrayCmd::Open);
    }
    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        use ksni::menu::*;
        vec![
            StandardItem { label: self.st.l_open.clone(), activate: Box::new(|t: &mut Self| send(&t.tx, &t.ctx, TrayCmd::Open)), ..Default::default() }.into(),
            MenuItem::Separator,
            CheckmarkItem {
                label: self.st.l_scan.clone(),
                checked: self.st.scan,
                activate: Box::new(|t: &mut Self| {
                    t.st.scan = !t.st.scan;
                    send(&t.tx, &t.ctx, TrayCmd::SetScan(t.st.scan));
                }),
                ..Default::default()
            }
            .into(),
            SubMenu {
                label: format!("{}: {}", self.st.l_status, self.st.l_statuses.get(self.st.status).cloned().unwrap_or_default()),
                submenu: vec![RadioGroup {
                    selected: self.st.status,
                    select: Box::new(|t: &mut Self, i| {
                        t.st.status = i;
                        send(&t.tx, &t.ctx, TrayCmd::SetStatus(i));
                    }),
                    options: self.st.l_statuses.iter().map(|l| RadioItem { label: l.clone(), ..Default::default() }).collect(),
                }
                .into()],
                ..Default::default()
            }
            .into(),
            CheckmarkItem {
                label: self.st.l_notify.clone(),
                checked: self.st.notify,
                activate: Box::new(|t: &mut Self| {
                    t.st.notify = !t.st.notify;
                    send(&t.tx, &t.ctx, TrayCmd::SetNotify(t.st.notify));
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem { label: self.st.l_quit.clone(), icon_name: "application-exit".into(), activate: Box::new(|t: &mut Self| send(&t.tx, &t.ctx, TrayCmd::Quit)), ..Default::default() }.into(),
        ]
    }
}

#[cfg(windows)]
mod win {
    use super::*;
    use tray_icon::menu::{CheckMenuItem, Menu, MenuEvent, MenuId, MenuItem, PredefinedMenuItem, Submenu};
    use tray_icon::{TrayIcon, TrayIconBuilder, TrayIconEvent};

    pub struct WinTray {
        _icon: TrayIcon,
        open: MenuItem,
        scan: CheckMenuItem,
        status_menu: Submenu,
        statuses: Vec<CheckMenuItem>,
        notify: CheckMenuItem,
        quit: MenuItem,
        scan_on: std::sync::Arc<std::sync::atomic::AtomicBool>,
        notify_on: std::sync::Arc<std::sync::atomic::AtomicBool>,
    }

    impl WinTray {
        /// Must be called on the UI thread (the window's message loop drives the tray).
        pub fn new(st: TrayState, tx: Sender<TrayCmd>, ctx: egui::Context, png: &[u8]) -> Option<Self> {
            let img = image::load_from_memory(png).ok()?.resize_exact(32, 32, image::imageops::FilterType::Triangle).to_rgba8();
            let icon = tray_icon::Icon::from_rgba(img.into_raw(), 32, 32).ok()?;
            let open = MenuItem::new(&st.l_open, true, None);
            let scan = CheckMenuItem::new(&st.l_scan, true, st.scan, None);
            let statuses: Vec<CheckMenuItem> = st.l_statuses.iter().enumerate().map(|(i, l)| CheckMenuItem::new(l, true, i == st.status, None)).collect();
            let status_menu = Submenu::new(&st.l_status, true);
            for s in &statuses {
                status_menu.append(s).ok()?;
            }
            let notify = CheckMenuItem::new(&st.l_notify, true, st.notify, None);
            let quit = MenuItem::new(&st.l_quit, true, None);
            let menu = Menu::new();
            menu.append_items(&[&open, &PredefinedMenuItem::separator(), &scan, &status_menu, &notify, &PredefinedMenuItem::separator(), &quit]).ok()?;
            let ids: (MenuId, MenuId, Vec<MenuId>, MenuId, MenuId) =
                (open.id().clone(), scan.id().clone(), statuses.iter().map(|s| s.id().clone()).collect(), notify.id().clone(), quit.id().clone());
            // Menu items are not thread-safe; mirror the toggles in atomics for the handler.
            let scan_on = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(st.scan));
            let notify_on = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(st.notify));
            let (scan_h, notify_h) = (scan_on.clone(), notify_on.clone());
            let (tx2, ctx2) = (tx.clone(), ctx.clone());
            MenuEvent::set_event_handler(Some(move |e: MenuEvent| {
                use std::sync::atomic::Ordering::SeqCst;
                if e.id == ids.0 {
                    send(&tx2, &ctx2, TrayCmd::Open);
                } else if e.id == ids.1 {
                    let on = !scan_h.load(SeqCst);
                    scan_h.store(on, SeqCst);
                    send(&tx2, &ctx2, TrayCmd::SetScan(on));
                } else if e.id == ids.3 {
                    let on = !notify_h.load(SeqCst);
                    notify_h.store(on, SeqCst);
                    send(&tx2, &ctx2, TrayCmd::SetNotify(on));
                } else if e.id == ids.4 {
                    send(&tx2, &ctx2, TrayCmd::Quit);
                } else if let Some(i) = ids.2.iter().position(|x| *x == e.id) {
                    send(&tx2, &ctx2, TrayCmd::SetStatus(i));
                }
            }));
            TrayIconEvent::set_event_handler(Some(move |e: TrayIconEvent| {
                if let TrayIconEvent::DoubleClick { .. } = e {
                    send(&tx, &ctx, TrayCmd::Open);
                }
            }));
            let tray = TrayIconBuilder::new().with_icon(icon).with_tooltip(&st.tooltip).with_menu(Box::new(menu)).build().ok()?;
            Some(Self { _icon: tray, open, scan, status_menu, statuses, notify, quit, scan_on, notify_on })
        }

        pub fn update(&self, st: &TrayState) {
            self.scan_on.store(st.scan, std::sync::atomic::Ordering::SeqCst);
            self.notify_on.store(st.notify, std::sync::atomic::Ordering::SeqCst);
            self.open.set_text(&st.l_open);
            self.scan.set_text(&st.l_scan);
            self.scan.set_checked(st.scan);
            self.status_menu.set_text(&st.l_status);
            for (i, s) in self.statuses.iter().enumerate() {
                if let Some(l) = st.l_statuses.get(i) {
                    s.set_text(l);
                }
                s.set_checked(i == st.status);
            }
            self.notify.set_text(&st.l_notify);
            self.notify.set_checked(st.notify);
            self.quit.set_text(&st.l_quit);
            let _ = self._icon.set_tooltip(Some(&st.tooltip));
        }
    }
}
