use std::cell::UnsafeCell;
use std::ffi::c_void;
use std::sync::OnceLock;

use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Dwm::{
    DWMWA_BORDER_COLOR, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    CreateFontW, CreatePen, CreateRoundRectRgn, CreateSolidBrush, GetStockObject, HBRUSH, HFONT,
    HGDIOBJ, HPEN, InvalidateRect, MONITOR_DEFAULTTONULL, MonitorFromRect, NULL_PEN, PS_SOLID,
    SetWindowRgn,
};
use windows::Win32::System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleW};
use windows::Win32::System::Registry::{
    HKEY, HKEY_CURRENT_USER, KEY_QUERY_VALUE, KEY_SET_VALUE, REG_SZ, REG_VALUE_TYPE, RegCloseKey,
    RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW,
};
use windows::Win32::System::Threading::CreateMutexW;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForSystem, GetDpiForWindow,
    SetProcessDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, RegisterHotKey, SetFocus, UnregisterHotKey,
};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_TIP, NIM_ADD, NIM_DELETE, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::note::{self, NoteInst};
use crate::store::{NoteData, Store};
use crate::theme;

pub const APP_CLASS: PCWSTR = w!("PostitApp");
pub const WM_TRAY: u32 = WM_APP + 1;
pub const WM_SHOW_ALL: u32 = WM_APP + 2;

const HOTKEY_ID: i32 = 1;
const TIMER_SAVE: usize = 1;
const SAVE_DELAY_MS: u32 = 700;

const CMD_NEW: usize = 100;
const CMD_TOGGLE: usize = 101;
const CMD_AUTOSTART: usize = 102;
const CMD_QUIT: usize = 103;
const CMD_COLOR_BASE: usize = 200;
const CMD_DELETE: usize = 300;

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const RUN_VALUE: PCWSTR = w!("Postit");
const COLOR_NAMES: [PCWSTR; 6] = [
    w!("Jaune"),
    w!("Rose"),
    w!("Vert"),
    w!("Bleu"),
    w!("Violet"),
    w!("Orange"),
];
const WELCOME: &str = "Bienvenue !

Clique sur la note pour écrire.
Glisse la barre du haut pour déplacer.
Tire les bords pour redimensionner.
La pastille change la couleur.
Ctrl+Alt+N : nouvelle note.

Tout est sauvegardé automatiquement.";

pub struct App {
    pub hwnd: HWND,
    pub notes: Vec<NoteInst>,
    pub store: Store,
    pub icon: HICON,
    pub brushes: Vec<(u32, HBRUSH)>,
    pub fonts: Vec<(u32, i32, HFONT)>,
    pub pen: Option<HPEN>,
    pub null_pen: HGDIOBJ,
    pub taskbar_msg: u32,
    pub autostart: bool,
    pub hidden: bool,
    pub dwm_supported: Option<bool>,
}

struct AppCell(UnsafeCell<App>);
unsafe impl Send for AppCell {}
unsafe impl Sync for AppCell {}
static APP: OnceLock<AppCell> = OnceLock::new();

pub fn app() -> &'static mut App {
    unsafe { &mut *APP.get().expect("etat applicatif absent").0.get() }
}

fn app_ready() -> bool {
    APP.get().is_some()
}

fn set_app(state: App) {
    let _ = APP.set(AppCell(UnsafeCell::new(state)));
}

pub fn window_dpi(hwnd: HWND) -> u32 {
    let dpi = unsafe { GetDpiForWindow(hwnd) };
    if dpi == 0 { 96 } else { dpi }
}

pub fn scale(value: i32, dpi: u32) -> i32 {
    ((value as i64 * dpi.max(96) as i64) / 96) as i32
}

impl App {
    fn new(hwnd: HWND, icon: HICON) -> Self {
        Self {
            hwnd,
            notes: Vec::new(),
            store: Store::load(),
            icon,
            brushes: Vec::new(),
            fonts: Vec::new(),
            pen: None,
            null_pen: unsafe { GetStockObject(NULL_PEN) },
            taskbar_msg: 0,
            autostart: false,
            hidden: false,
            dwm_supported: None,
        }
    }

    pub fn brush(&mut self, color: COLORREF) -> HBRUSH {
        if let Some((_, brush)) = self.brushes.iter().find(|(c, _)| *c == color.0) {
            return *brush;
        }
        let brush = unsafe { CreateSolidBrush(color) };
        self.brushes.push((color.0, brush));
        brush
    }

    pub fn font(&mut self, dpi: u32, points: i32) -> HFONT {
        if let Some((_, _, font)) = self
            .fonts
            .iter()
            .find(|(d, p, _)| *d == dpi && *p == points)
        {
            return *font;
        }
        let height = -((points * dpi as i32) / 72);
        let font = unsafe {
            CreateFontW(
                height,
                0,
                0,
                0,
                400,
                0,
                0,
                0,
                windows::Win32::Graphics::Gdi::FONT_CHARSET(1),
                windows::Win32::Graphics::Gdi::FONT_OUTPUT_PRECISION(0),
                windows::Win32::Graphics::Gdi::FONT_CLIP_PRECISION(0),
                windows::Win32::Graphics::Gdi::FONT_QUALITY(5),
                0,
                w!("Segoe UI"),
            )
        };
        self.fonts.push((dpi, points, font));
        font
    }

    pub fn pen(&mut self) -> HPEN {
        if let Some(pen) = self.pen {
            return pen;
        }
        let pen = unsafe { CreatePen(PS_SOLID, 1, theme::ACCENT_COLOR) };
        self.pen = Some(pen);
        pen
    }

    pub fn schedule_save(&mut self) {
        unsafe {
            SetTimer(Some(self.hwnd), TIMER_SAVE, SAVE_DELAY_MS, None);
        }
    }
}

pub fn run() -> windows::core::Result<()> {
    unsafe {
        if SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2).is_err() {
            let _ = SetProcessDPIAware();
        }

        if !claim_instance() {
            return Ok(());
        }

        let module = GetModuleHandleW(None)?;
        let hinst = HINSTANCE(module.0);
        let icon = load_icon(hinst);

        register_classes(hinst, icon);

        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            APP_CLASS,
            w!("Post-it"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(hinst),
            None,
        )?;

        let mut state = App::new(hwnd, icon);
        state.autostart = autostart_enabled();
        set_app(state);

        app().taskbar_msg = RegisterWindowMessageW(w!("TaskbarCreated"));
        add_tray();
        register_hotkey();
        restore_notes();
        message_loop();
    }
    Ok(())
}

fn claim_instance() -> bool {
    unsafe {
        let _mutex = CreateMutexW(None, true, w!("Local\\Postit-7C1E4E2A"));
        if GetLastError() == ERROR_ALREADY_EXISTS {
            if let Ok(existing) = FindWindowW(APP_CLASS, PCWSTR::null()) {
                let _ = PostMessageW(Some(existing), WM_SHOW_ALL, WPARAM(0), LPARAM(0));
            }
            return false;
        }
    }
    true
}

fn load_icon(hinst: HINSTANCE) -> HICON {
    unsafe {
        // ID 1 = icone embarquee par winresource (build.rs).
        let embedded = LoadImageW(
            Some(hinst),
            PCWSTR(1 as *const u16),
            IMAGE_ICON,
            0,
            0,
            LR_DEFAULTSIZE | LR_SHARED,
        );
        if let Ok(handle) = embedded {
            return HICON(handle.0);
        }
        LoadIconW(None, IDI_APPLICATION).unwrap_or_default()
    }
}

fn register_classes(hinst: HINSTANCE, icon: HICON) {
    unsafe {
        let cursor = LoadCursorW(None, IDC_ARROW).unwrap_or_default();

        let app_class = WNDCLASSW {
            style: CS_DROPSHADOW,
            lpfnWndProc: Some(app_proc),
            hInstance: hinst,
            hIcon: icon,
            hCursor: cursor,
            lpszClassName: APP_CLASS,
            ..Default::default()
        };
        RegisterClassW(&app_class);

        let note_class = WNDCLASSW {
            style: CS_DROPSHADOW,
            lpfnWndProc: Some(note::note_proc),
            hInstance: hinst,
            hIcon: icon,
            hCursor: cursor,
            lpszClassName: note::NOTE_CLASS,
            ..Default::default()
        };
        RegisterClassW(&note_class);
    }
}

fn message_loop() {
    unsafe {
        let mut msg = MSG::default();
        loop {
            let ret = GetMessageW(&mut msg, None, 0, 0);
            if ret.0 <= 0 {
                break;
            }
            let _ = TranslateMessage(&msg);
            DispatchMessageW(&msg);
        }
    }
}

unsafe extern "system" fn app_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if !app_ready() {
        return unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
    }

    match msg {
        WM_TRAY => {
            tray_callback(lp);
            LRESULT(0)
        }
        WM_TIMER if wp.0 == TIMER_SAVE => {
            unsafe {
                let _ = KillTimer(Some(hwnd), TIMER_SAVE);
            }
            save_now();
            LRESULT(0)
        }
        WM_HOTKEY if wp.0 == HOTKEY_ID as usize => {
            new_note(true);
            LRESULT(0)
        }
        WM_SHOW_ALL => {
            show_all();
            LRESULT(0)
        }
        WM_QUERYENDSESSION => {
            save_now();
            LRESULT(1)
        }
        WM_ENDSESSION => {
            if wp.0 != 0 {
                save_now();
            }
            LRESULT(0)
        }
        WM_DESTROY => {
            shutdown();
            unsafe { PostQuitMessage(0) };
            LRESULT(0)
        }
        _ => {
            let taskbar_msg = app().taskbar_msg;
            if taskbar_msg != 0 && msg == taskbar_msg {
                add_tray();
                return LRESULT(0);
            }
            unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
        }
    }
}

fn tray_callback(lp: LPARAM) {
    let event = lp.0 as u32 & 0xFFFF;
    match event {
        WM_LBUTTONUP | WM_LBUTTONDBLCLK => new_note(true),
        WM_RBUTTONUP => tray_menu(),
        _ => {}
    }
}

pub fn new_note(focus: bool) {
    let (x, y, w, h) = next_rect();
    let data = {
        let state = app();
        let id = state.store.next_id;
        state.store.next_id += 1;
        let mut data = NoteData::new(id, x, y);
        data.w = w;
        data.h = h;
        data
    };

    app().store.notes.push(data.clone());
    if attach(&data, focus).is_some() {
        app().hidden = false;
        app().schedule_save();
    } else {
        let state = app();
        state.store.notes.retain(|n| n.id != data.id);
    }
}

fn restore_notes() {
    if app().store.fresh {
        let (x, y, w, h) = next_rect();
        let data = {
            let state = app();
            let id = state.store.next_id;
            state.store.next_id += 1;
            let mut data = NoteData::new(id, x, y);
            data.w = w;
            data.h = h;
            data.text = WELCOME.to_string();
            data
        };
        app().store.notes.push(data.clone());
        attach(&data, false);
        save_now();
        return;
    }

    let notes = app().store.notes.clone();
    for data in notes {
        let data = clamp_to_screen(data);
        attach(&data, false);
    }
}

fn attach(data: &NoteData, focus: bool) -> Option<HWND> {
    let (hwnd, edit) = note::create(data)?;
    {
        let state = app();
        state.notes.push(NoteInst {
            hwnd,
            edit,
            id: data.id,
            color: data.color,
            close_hover: false,
            dot_hover: false,
            tracking: false,
        });
    }
    apply_rounding(hwnd, data.color);

    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        if focus {
            SetWindowPos(
                hwnd,
                Some(HWND_TOP),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )
            .ok();
            let _ = SetForegroundWindow(hwnd);
            let _ = SetFocus(Some(edit));
        } else {
            lower(hwnd);
        }
    }
    Some(hwnd)
}

pub fn lower(hwnd: HWND) {
    unsafe {
        SetWindowPos(
            hwnd,
            Some(HWND_BOTTOM),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )
        .ok();
    }
}

fn next_rect() -> (i32, i32, i32, i32) {
    unsafe {
        let dpi = GetDpiForSystem().max(96);
        let w = scale(theme::DEFAULT_W, dpi);
        let h = scale(theme::DEFAULT_H, dpi);

        let mut work = RECT::default();
        let _ = SystemParametersInfoW(
            SPI_GETWORKAREA,
            0,
            Some(&mut work as *mut _ as *mut c_void),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        );
        if work.right <= work.left {
            work = RECT {
                left: 0,
                top: 0,
                right: GetSystemMetrics(SM_CXSCREEN),
                bottom: GetSystemMetrics(SM_CYSCREEN),
            };
        }

        let offset = (app().notes.len() as i32 % 8) * scale(26, dpi);
        let mut x = work.left + (work.right - work.left - w) / 2 + offset;
        let mut y = work.top + (work.bottom - work.top - h) / 2 + offset;
        x = x.min(work.right - w - scale(12, dpi)).max(work.left + scale(12, dpi));
        y = y.min(work.bottom - h - scale(12, dpi)).max(work.top + scale(12, dpi));
        (x, y, w, h)
    }
}

fn clamp_to_screen(mut data: NoteData) -> NoteData {
    data.w = data.w.clamp(160, 2000);
    data.h = data.h.clamp(140, 2000);

    unsafe {
        let rect = RECT {
            left: data.x,
            top: data.y,
            right: data.x + data.w,
            bottom: data.y + data.h,
        };
        let monitor = MonitorFromRect(&rect, MONITOR_DEFAULTTONULL);
        if monitor.is_invalid() {
            let (x, y, w, h) = next_rect();
            data.x = x;
            data.y = y;
            data.w = w.min(data.w);
            data.h = h.min(data.h);
        }

        let vx = GetSystemMetrics(SM_XVIRTUALSCREEN);
        let vy = GetSystemMetrics(SM_YVIRTUALSCREEN);
        let vw = GetSystemMetrics(SM_CXVIRTUALSCREEN);
        let vh = GetSystemMetrics(SM_CYVIRTUALSCREEN);
        if data.x < vx - data.w + 80 || data.x > vx + vw - 80 {
            data.x = vx + (vw - data.w) / 2;
        }
        if data.y < vy - 40 || data.y > vy + vh - 80 {
            data.y = vy + (vh - data.h) / 2;
        }
    }
    data
}

pub fn show_all() {
    if app().notes.is_empty() {
        new_note(true);
        return;
    }
    let hwnds: Vec<(HWND, HWND)> = app().notes.iter().map(|n| (n.hwnd, n.edit)).collect();
    for (hwnd, _) in &hwnds {
        unsafe {
            let _ = ShowWindow(*hwnd, SW_SHOWNOACTIVATE);
        }
        lower(*hwnd);
    }
    app().hidden = false;
    if let Some((hwnd, edit)) = hwnds.first() {
        unsafe {
            let _ = SetForegroundWindow(*hwnd);
            let _ = SetFocus(Some(*edit));
        }
    }
}

fn tray_menu() {
    let (hidden, autostart, owner) = {
        let state = app();
        (state.hidden, state.autostart, state.hwnd)
    };

    let cmd = show_menu(owner, |menu| {
        append(menu, MF_STRING, CMD_NEW, w!("Nouvelle note\tCtrl+Alt+N"));
        append(menu, MF_SEPARATOR, 0, PCWSTR::null());
        let toggle = if hidden {
            w!("Tout afficher")
        } else {
            w!("Tout masquer")
        };
        append(menu, MF_STRING, CMD_TOGGLE, toggle);
        let flags = if autostart {
            MF_STRING | MF_CHECKED
        } else {
            MF_STRING
        };
        append(menu, flags, CMD_AUTOSTART, w!("Lancer au démarrage"));
        append(menu, MF_SEPARATOR, 0, PCWSTR::null());
        append(menu, MF_STRING, CMD_QUIT, w!("Quitter"));
    });

    match cmd {
        CMD_NEW => new_note(true),
        CMD_TOGGLE => toggle_visibility(),
        CMD_AUTOSTART => toggle_autostart(),
        CMD_QUIT => quit(),
        _ => {}
    }
}

fn toggle_visibility() {
    let (hidden, hwnds) = {
        let state = app();
        (
            state.hidden,
            state.notes.iter().map(|n| n.hwnd).collect::<Vec<_>>(),
        )
    };

    if hidden {
        for hwnd in hwnds {
            unsafe {
                let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
            }
            lower(hwnd);
        }
        app().hidden = false;
    } else {
        for hwnd in hwnds {
            unsafe {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
        }
        app().hidden = true;
    }
}

pub fn note_menu(hwnd: HWND) {
    let Some((color, id)) = app()
        .notes
        .iter()
        .find(|n| n.hwnd == hwnd)
        .map(|n| (n.color, n.id))
    else {
        return;
    };

    let cmd = show_menu(hwnd, |menu| unsafe {
        append(menu, MF_STRING, CMD_NEW, w!("Nouvelle note"));
        let submenu = CreatePopupMenu().unwrap_or_default();
        for (index, name) in COLOR_NAMES.iter().enumerate() {
            let flags = if index == color as usize {
                MF_STRING | MF_CHECKED
            } else {
                MF_STRING
            };
            append(submenu, flags, CMD_COLOR_BASE + index, *name);
        }
        let _ = AppendMenuW(
            menu,
            MF_POPUP,
            submenu.0 as usize,
            w!("Couleur"),
        );
        append(menu, MF_SEPARATOR, 0, PCWSTR::null());
        append(menu, MF_STRING, CMD_DELETE, w!("Supprimer la note"));
    });

    match cmd {
        CMD_NEW => new_note(true),
        CMD_DELETE => delete_note(hwnd),
        value if (CMD_COLOR_BASE..CMD_COLOR_BASE + COLOR_NAMES.len()).contains(&value) => {
            set_color(id, (value - CMD_COLOR_BASE) as u8);
        }
        _ => {}
    }
}

pub fn set_color(id: u64, color: u8) {
    let (hwnd, edit) = {
        let state = app();
        let Some(note) = state.notes.iter_mut().find(|n| n.id == id) else {
            return;
        };
        note.color = color;
        if let Some(saved) = state.store.notes.iter_mut().find(|n| n.id == id) {
            saved.color = color;
        }
        (note.hwnd, note.edit)
    };

    apply_rounding(hwnd, color);
    unsafe {
        let _ = InvalidateRect(Some(hwnd), None, true);
        let _ = InvalidateRect(Some(edit), None, true);
    }
    app().schedule_save();
}

pub fn cycle_color(hwnd: HWND) {
    let next = app()
        .notes
        .iter()
        .find(|n| n.hwnd == hwnd)
        .map(|n| (n.color + 1) % 6);
    if let Some(color) = next {
        let id = app().notes.iter().find(|n| n.hwnd == hwnd).map(|n| n.id);
        if let Some(id) = id {
            set_color(id, color);
        }
    }
}

pub fn delete_note(hwnd: HWND) {
    {
        let state = app();
        let Some(index) = state.notes.iter().position(|n| n.hwnd == hwnd) else {
            return;
        };
        let id = state.notes[index].id;
        let text = read_edit(state.notes[index].edit);
        if let Some(saved) = state.store.notes.iter_mut().find(|n| n.id == id) {
            saved.text = text;
        }
        state.notes.remove(index);
        state.store.notes.retain(|n| n.id != id);
    }

    unsafe {
        let _ = DestroyWindow(hwnd);
    }
    save_now();
}

fn read_edit(edit: HWND) -> String {
    unsafe {
        let len = GetWindowTextLengthW(edit).max(0) as usize;
        if len == 0 {
            return String::new();
        }
        let mut buffer = vec![0u16; len + 1];
        let copied = GetWindowTextW(edit, &mut buffer).max(0) as usize;
        String::from_utf16_lossy(&buffer[..copied])
    }
}

pub fn save_now() {
    let notes: Vec<(u64, HWND, HWND)> = app()
        .notes
        .iter()
        .map(|n| (n.id, n.hwnd, n.edit))
        .collect();

    for (id, hwnd, edit) in notes {
        let text = read_edit(edit);
        let mut rect = RECT::default();
        let geometry = unsafe { GetWindowRect(hwnd, &mut rect).is_ok() };
        let state = app();
        if let Some(saved) = state.store.notes.iter_mut().find(|n| n.id == id) {
            saved.text = text;
            if geometry {
                saved.x = rect.left;
                saved.y = rect.top;
                saved.w = rect.right - rect.left;
                saved.h = rect.bottom - rect.top;
            }
        }
    }

    if let Err(err) = app().store.save() {
        eprintln!("Sauvegarde impossible: {err}");
    }
}

fn quit() {
    save_now();
    let hwnds: Vec<HWND> = app().notes.iter().map(|n| n.hwnd).collect();
    for hwnd in hwnds {
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
    }
    let hwnd = app().hwnd;
    unsafe {
        let _ = DestroyWindow(hwnd);
    }
}

fn shutdown() {
    remove_tray();
    save_now();
    unsafe {
        let _ = KillTimer(Some(app().hwnd), TIMER_SAVE);
        let _ = UnregisterHotKey(Some(app().hwnd), HOTKEY_ID);
    }
}

pub fn apply_rounding(hwnd: HWND, color: u8) {
    unsafe {
        let paper = theme::paper(color);
        let preference = DWMWCP_ROUND;
        let rounded = DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &preference as *const _ as *const c_void,
            std::mem::size_of_val(&preference) as u32,
        )
        .is_ok();

        let border = theme::darken(paper, 22).0;
        let _ = DwmSetWindowAttribute(
            hwnd,
            DWMWA_BORDER_COLOR,
            &border as *const _ as *const c_void,
            std::mem::size_of_val(&border) as u32,
        );

        let state = app();
        if state.dwm_supported.is_none() {
            state.dwm_supported = Some(rounded);
        }
        if !rounded || state.dwm_supported == Some(false) {
            apply_region(hwnd);
        }
    }
}

pub fn apply_region(hwnd: HWND) {
    unsafe {
        let mut rect = RECT::default();
        if GetClientRect(hwnd, &mut rect).is_err() {
            return;
        }
        let dpi = window_dpi(hwnd);
        let radius = scale(14, dpi);
        let region = CreateRoundRectRgn(0, 0, rect.right + 1, rect.bottom + 1, radius, radius);
        SetWindowRgn(hwnd, Some(region), true);
    }
}

fn add_tray() {
    let data = tray_data();
    unsafe {
        let _ = Shell_NotifyIconW(NIM_ADD, &data);
    }
}

fn remove_tray() {
    let data = tray_data();
    unsafe {
        let _ = Shell_NotifyIconW(NIM_DELETE, &data);
    }
}

fn tray_data() -> NOTIFYICONDATAW {
    let mut data: NOTIFYICONDATAW = unsafe { std::mem::zeroed() };
    let state = app();
    data.cbSize = std::mem::size_of::<NOTIFYICONDATAW>() as u32;
    data.hWnd = state.hwnd;
    data.uID = 1;
    data.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    data.uCallbackMessage = WM_TRAY;
    data.hIcon = state.icon;
    let tip: Vec<u16> = "Post-it".encode_utf16().collect();
    for (slot, value) in data.szTip.iter_mut().zip(tip) {
        *slot = value;
    }
    data
}

fn register_hotkey() {
    unsafe {
        let _ = RegisterHotKey(
            Some(app().hwnd),
            HOTKEY_ID,
            MOD_CONTROL | MOD_ALT | MOD_NOREPEAT,
            'N' as u32,
        );
    }
}

fn show_menu(owner: HWND, build: impl FnOnce(HMENU)) -> usize {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else {
            return 0;
        };
        build(menu);
        let _ = SetForegroundWindow(owner);

        let mut point = POINT::default();
        let _ = GetCursorPos(&mut point);
        let result = TrackPopupMenu(
            menu,
            TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY,
            point.x,
            point.y,
            None,
            owner,
            None,
        );
        let _ = DestroyMenu(menu);
        let _ = PostMessageW(Some(owner), WM_NULL, WPARAM(0), LPARAM(0));
        result.0 as usize
    }
}

fn append(menu: HMENU, flags: MENU_ITEM_FLAGS, id: usize, text: PCWSTR) {
    unsafe {
        let _ = AppendMenuW(menu, flags, id, text);
    }
}

fn toggle_autostart() {
    let enabled = app().autostart;
    if set_autostart(!enabled) {
        app().autostart = !enabled;
    }
}

fn autostart_enabled() -> bool {
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, None, KEY_QUERY_VALUE, &mut key).0 != 0 {
            return false;
        }
        let mut buffer = [0u16; 1024];
        let mut size = (buffer.len() * 2) as u32;
        let mut kind = REG_VALUE_TYPE(0);
        let result = RegQueryValueExW(
            key,
            RUN_VALUE,
            None,
            Some(&mut kind),
            Some(buffer.as_mut_ptr() as *mut u8),
            Some(&mut size),
        );
        let _ = RegCloseKey(key);
        if result.0 != 0 || kind != REG_SZ {
            return false;
        }
        let len = (size as usize / 2).saturating_sub(1);
        !String::from_utf16_lossy(&buffer[..len]).trim().is_empty()
    }
}

fn set_autostart(enabled: bool) -> bool {
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, None, KEY_SET_VALUE, &mut key).0 != 0 {
            return false;
        }

        let result = if enabled {
            let mut path = vec![0u16; 32768];
            let len = GetModuleFileNameW(None, &mut path) as usize;
            let exe = String::from_utf16_lossy(&path[..len]);
            let value = format!("\"{exe}\"");
            let mut bytes: Vec<u8> = value
                .encode_utf16()
                .flat_map(|unit| unit.to_le_bytes())
                .collect();
            bytes.extend_from_slice(&[0, 0]);
            RegSetValueExW(key, RUN_VALUE, None, REG_SZ, Some(&bytes))
        } else {
            RegDeleteValueW(key, RUN_VALUE)
        };

        let _ = RegCloseKey(key);
        result.0 == 0
    }
}


