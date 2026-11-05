use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::WM_MOUSELEAVE;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    TME_LEAVE, TRACKMOUSEEVENT, TrackMouseEvent,
};
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::app::{self, app, apply_region, lower, scale, window_dpi};
use crate::store::NoteData;
use crate::theme;

pub const NOTE_CLASS: PCWSTR = w!("PostitNote");

const TITLE_HEIGHT: i32 = 34;
const EDGE: i32 = 6;

const EM_SETLIMITTEXT: u32 = 0x00C5;
const EM_SETMARGINS: u32 = 0x00D3;
const EC_LEFTMARGIN: usize = 0x0001;
const EC_RIGHTMARGIN: usize = 0x0002;

pub struct NoteInst {
    pub hwnd: HWND,
    pub edit: HWND,
    pub id: u64,
    pub color: u8,
    pub close_hover: bool,
    pub dot_hover: bool,
    pub tracking: bool,
}

pub fn create(data: &NoteData) -> Option<(HWND, HWND)> {
    unsafe {
        let module = GetModuleHandleW(None).ok()?;
        let hinst = HINSTANCE(module.0);

        let hwnd = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            NOTE_CLASS,
            w!("Post-it"),
            WS_POPUP | WS_THICKFRAME | WS_CLIPCHILDREN,
            data.x,
            data.y,
            data.w.max(120),
            data.h.max(100),
            None,
            None,
            Some(hinst),
            None,
        )
        .ok()?;

        SetWindowLongPtrW(hwnd, GWLP_USERDATA, data.id as isize);

        let style = WS_CHILD
            | WS_VISIBLE
            | WS_VSCROLL
            | WINDOW_STYLE(
                (ES_MULTILINE | ES_AUTOVSCROLL | ES_WANTRETURN | ES_NOHIDESEL) as u32,
            );
        let edit = CreateWindowExW(
            WINDOW_EX_STYLE(0),
            w!("EDIT"),
            w!(""),
            style,
            0,
            0,
            10,
            10,
            Some(hwnd),
            None,
            None,
            None,
        )
        .ok()?;

        let font = app().font(window_dpi(hwnd), 11);
        SendMessageW(
            edit,
            WM_SETFONT,
            Some(WPARAM(font.0 as usize)),
            Some(LPARAM(1)),
        );
        SendMessageW(
            edit,
            EM_SETLIMITTEXT,
            Some(WPARAM(0)),
            Some(LPARAM(0)),
        );
        SendMessageW(
            edit,
            EM_SETMARGINS,
            Some(WPARAM(EC_LEFTMARGIN | EC_RIGHTMARGIN)),
            Some(LPARAM(0)),
        );
        let text = HSTRING::from(data.text.as_str());
        let _ = SetWindowTextW(edit, &text);

        layout(hwnd, edit);
        Some((hwnd, edit))
    }
}

fn layout(hwnd: HWND, edit: HWND) {
    unsafe {
        let mut rect = RECT::default();
        if GetClientRect(hwnd, &mut rect).is_err() {
            return;
        }
        let dpi = window_dpi(hwnd);
        let padding = scale(10, dpi);
        let top = scale(TITLE_HEIGHT, dpi);
        let width = (rect.right - padding * 2).max(10);
        let height = (rect.bottom - top - padding).max(10);
        let _ = MoveWindow(edit, padding, top, width, height, true);
    }
}

fn note_index(hwnd: HWND) -> Option<usize> {
    app().notes.iter().position(|n| n.hwnd == hwnd)
}

fn buttons(hwnd: HWND) -> (RECT, RECT) {
    unsafe {
        let mut client = RECT::default();
        let _ = GetClientRect(hwnd, &mut client);
        let dpi = window_dpi(hwnd);
        let title = scale(TITLE_HEIGHT, dpi);
        let size = scale(22, dpi);
        let margin = scale(8, dpi);

        let close = RECT {
            left: client.right - margin - size,
            top: (title - size) / 2,
            right: client.right - margin,
            bottom: (title + size) / 2,
        };

        let dot_size = scale(16, dpi);
        let dot = RECT {
            left: close.left - scale(10, dpi) - dot_size,
            top: (title - dot_size) / 2,
            right: close.left - scale(10, dpi),
            bottom: (title + dot_size) / 2,
        };
        (close, dot)
    }
}

fn contains(rect: &RECT, x: i32, y: i32) -> bool {
    x >= rect.left && x < rect.right && y >= rect.top && y < rect.bottom
}

fn signed_low(value: isize) -> i32 {
    (value as u32 & 0xFFFF) as u16 as i16 as i32
}

fn signed_high(value: isize) -> i32 {
    ((value as u32 >> 16) & 0xFFFF) as u16 as i16 as i32
}

fn set_hover(hwnd: HWND, close: bool, dot: bool) -> bool {
    if let Some(index) = note_index(hwnd) {
        let state = app();
        let note = &mut state.notes[index];
        if note.close_hover != close || note.dot_hover != dot {
            note.close_hover = close;
            note.dot_hover = dot;
            unsafe {
                let _ = InvalidateRect(Some(hwnd), None, false);
            }
            return true;
        }
    }
    false
}

pub unsafe extern "system" fn note_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    unsafe {
        match msg {
            WM_NCCALCSIZE if wp.0 != 0 => LRESULT(0),

            WM_NCHITTEST => {
                let mut window = RECT::default();
                if GetWindowRect(hwnd, &mut window).is_err() {
                    return LRESULT(HTCLIENT as isize);
                }
                let x = signed_low(lp.0) - window.left;
                let y = signed_high(lp.0) - window.top;
                let width = window.right - window.left;
                let height = window.bottom - window.top;
                let dpi = window_dpi(hwnd);
                let edge = scale(EDGE, dpi);

                let left = x < edge;
                let right = x >= width - edge;
                let top = y < edge;
                let bottom = y >= height - edge;

                if top && left {
                    return LRESULT(HTTOPLEFT as isize);
                }
                if top && right {
                    return LRESULT(HTTOPRIGHT as isize);
                }
                if bottom && left {
                    return LRESULT(HTBOTTOMLEFT as isize);
                }
                if bottom && right {
                    return LRESULT(HTBOTTOMRIGHT as isize);
                }
                if left {
                    return LRESULT(HTLEFT as isize);
                }
                if right {
                    return LRESULT(HTRIGHT as isize);
                }
                if top {
                    return LRESULT(HTTOP as isize);
                }
                if bottom {
                    return LRESULT(HTBOTTOM as isize);
                }

                if y < scale(TITLE_HEIGHT, dpi) {
                    let (close, dot) = buttons(hwnd);
                    if contains(&close, x, y) || contains(&dot, x, y) {
                        return LRESULT(HTCLIENT as isize);
                    }
                    return LRESULT(HTCAPTION as isize);
                }
                LRESULT(HTCLIENT as isize)
            }

            WM_GETMINMAXINFO => {
                let info = &mut *(lp.0 as *mut MINMAXINFO);
                let dpi = window_dpi(hwnd);
                info.ptMinTrackSize = POINT {
                    x: scale(180, dpi),
                    y: scale(140, dpi),
                };
                LRESULT(0)
            }

            WM_PAINT => {
                paint(hwnd);
                LRESULT(0)
            }

            WM_ERASEBKGND => LRESULT(1),

            WM_CTLCOLOREDIT => {
                let hdc = HDC(wp.0 as *mut std::ffi::c_void);
                let color = note_index(hwnd)
                    .map(|index| app().notes[index].color)
                    .unwrap_or(0);
                let paper = theme::paper(color);
                SetTextColor(hdc, theme::TEXT_COLOR);
                SetBkColor(hdc, paper);
                let brush = app().brush(paper);
                LRESULT(brush.0 as isize)
            }

            WM_DPICHANGED => {
                let suggested = &*(lp.0 as *const RECT);
                SetWindowPos(
                    hwnd,
                    None,
                    suggested.left,
                    suggested.top,
                    suggested.right - suggested.left,
                    suggested.bottom - suggested.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                )
                .ok();
                if let Some(index) = note_index(hwnd) {
                    let edit = app().notes[index].edit;
                    let font = app().font(window_dpi(hwnd), 11);
                    SendMessageW(
                        edit,
                        WM_SETFONT,
                        Some(WPARAM(font.0 as usize)),
                        Some(LPARAM(1)),
                    );
                    layout(hwnd, edit);
                }
                if app().dwm_supported == Some(false) {
                    apply_region(hwnd);
                }
                app().schedule_save();
                LRESULT(0)
            }

            WM_MOVE | WM_SIZE => {
                if let Some(index) = note_index(hwnd) {
                    if msg == WM_SIZE {
                        let edit = app().notes[index].edit;
                        layout(hwnd, edit);
                    }
                }
                if msg == WM_SIZE && app().dwm_supported == Some(false) {
                    apply_region(hwnd);
                }
                if msg == WM_MOVE {
                    app().schedule_save();
                }
                LRESULT(0)
            }

            WM_EXITSIZEMOVE => {
                app::save_now();
                LRESULT(0)
            }

            WM_ACTIVATE => {
                if (wp.0 as u32 & 0xFFFF) == WA_INACTIVE {
                    lower(hwnd);
                }
                LRESULT(0)
            }

            WM_MOUSEMOVE => {
                let x = signed_low(lp.0);
                let y = signed_high(lp.0);
                let (close, dot) = buttons(hwnd);
                set_hover(hwnd, contains(&close, x, y), contains(&dot, x, y));

                if let Some(index) = note_index(hwnd) {
                    if !app().notes[index].tracking {
                        let mut track = TRACKMOUSEEVENT {
                            cbSize: std::mem::size_of::<TRACKMOUSEEVENT>() as u32,
                            dwFlags: TME_LEAVE,
                            hwndTrack: hwnd,
                            dwHoverTime: 0,
                        };
                        let _ = TrackMouseEvent(&mut track);
                        app().notes[index].tracking = true;
                    }
                }
                LRESULT(0)
            }

            WM_MOUSELEAVE => {
                if let Some(index) = note_index(hwnd) {
                    let state = app();
                    state.notes[index].tracking = false;
                }
                set_hover(hwnd, false, false);
                LRESULT(0)
            }

            WM_SETCURSOR => {
                let hit = (lp.0 as u32) & 0xFFFF;
                if hit == HTCLIENT {
                    let mut point = POINT::default();
                    let _ = GetCursorPos(&mut point);
                    let _ = ScreenToClient(hwnd, &mut point);
                    let (close, dot) = buttons(hwnd);
                    if contains(&close, point.x, point.y) || contains(&dot, point.x, point.y) {
                        if let Ok(cursor) = LoadCursorW(None, IDC_HAND) {
                            SetCursor(Some(cursor));
                        }
                        return LRESULT(1);
                    }
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }

            WM_LBUTTONUP => {
                let x = signed_low(lp.0);
                let y = signed_high(lp.0);
                let (close, dot) = buttons(hwnd);
                if contains(&close, x, y) {
                    let _ = PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
                } else if contains(&dot, x, y) {
                    app::cycle_color(hwnd);
                }
                LRESULT(0)
            }

            WM_RBUTTONUP => {
                app::note_menu(hwnd);
                LRESULT(0)
            }

            WM_NCRBUTTONUP => {
                if (wp.0 as u32) == HTCAPTION {
                    app::note_menu(hwnd);
                    return LRESULT(0);
                }
                DefWindowProcW(hwnd, msg, wp, lp)
            }

            WM_COMMAND => {
                let code = ((wp.0 >> 16) & 0xFFFF) as u32;
                if code == EN_CHANGE && note_index(hwnd).is_some() {
                    app().schedule_save();
                }
                LRESULT(0)
            }

            WM_CLOSE => {
                app::delete_note(hwnd);
                LRESULT(0)
            }

            WM_DESTROY => LRESULT(0),

            _ => DefWindowProcW(hwnd, msg, wp, lp),
        }
    }
}

fn paint(hwnd: HWND) {
    unsafe {
        let mut paint = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut paint);

        let (color, close_hover, dot_hover) = note_index(hwnd)
            .map(|index| {
                let note = &app().notes[index];
                (note.color, note.close_hover, note.dot_hover)
            })
            .unwrap_or((0, false, false));

        let paper = theme::paper(color);
        let title_bg = theme::darken(paper, 6);
        let line = theme::darken(paper, 16);
        let accent = theme::darken(paper, 55);

        let mut client = RECT::default();
        let _ = GetClientRect(hwnd, &mut client);
        let dpi = window_dpi(hwnd);
        let title_height = scale(TITLE_HEIGHT, dpi);

        let state = app();
        FillRect(hdc, &client, state.brush(paper));

        let title = RECT {
            left: 0,
            top: 0,
            right: client.right,
            bottom: title_height,
        };
        FillRect(hdc, &title, state.brush(title_bg));

        let separator = RECT {
            left: 0,
            top: title_height - 1,
            right: client.right,
            bottom: title_height,
        };
        FillRect(hdc, &separator, state.brush(line));

        // Pastille de couleur a gauche.
        let old_brush = SelectObject(hdc, HGDIOBJ(state.brush(accent).0));
        let old_pen = SelectObject(hdc, state.null_pen);
        let radius = scale(5, dpi);
        let center_x = scale(16, dpi);
        let center_y = title_height / 2;
        let _ = Ellipse(
            hdc,
            center_x - radius,
            center_y - radius,
            center_x + radius,
            center_y + radius,
        );
        SelectObject(hdc, old_pen);
        SelectObject(hdc, old_brush);

        // Libelle discret.
        let font = state.font(dpi, 9);
        let old_font = SelectObject(hdc, HGDIOBJ(font.0));
        SetTextColor(hdc, theme::MUTED_COLOR);
        SetBkMode(hdc, TRANSPARENT);
        let mut label: Vec<u16> = "Post-it".encode_utf16().collect();
        let mut label_rect = RECT {
            left: scale(28, dpi),
            top: 0,
            right: (client.right - scale(64, dpi)).max(scale(32, dpi)),
            bottom: title_height,
        };
        DrawTextW(
            hdc,
            &mut label,
            &mut label_rect,
            DT_SINGLELINE | DT_VCENTER | DT_LEFT | DT_NOPREFIX,
        );

        let (close, dot) = buttons(hwnd);

        if close_hover {
            FillRect(hdc, &close, state.brush(line));
        }
        let pen = state.pen();
        let old_pen = SelectObject(hdc, HGDIOBJ(pen.0));
        let inset = scale(6, dpi);
        let _ = MoveToEx(hdc, close.left + inset, close.top + inset, None);
        let _ = LineTo(hdc, close.right - inset, close.bottom - inset);
        let _ = MoveToEx(hdc, close.right - inset, close.top + inset, None);
        let _ = LineTo(hdc, close.left + inset, close.bottom - inset);
        SelectObject(hdc, old_pen);

        if dot_hover {
            FillRect(hdc, &dot, state.brush(line));
        }
        let dot_center_x = (dot.left + dot.right) / 2;
        let dot_center_y = (dot.top + dot.bottom) / 2;
        let dot_radius = scale(5, dpi);
        let old_brush = SelectObject(hdc, HGDIOBJ(state.brush(accent).0));
        let old_pen = SelectObject(hdc, state.null_pen);
        let _ = Ellipse(
            hdc,
            dot_center_x - dot_radius,
            dot_center_y - dot_radius,
            dot_center_x + dot_radius,
            dot_center_y + dot_radius,
        );
        SelectObject(hdc, old_pen);
        SelectObject(hdc, old_brush);

        SelectObject(hdc, old_font);
        let _ = EndPaint(hwnd, &paint);
    }
}
