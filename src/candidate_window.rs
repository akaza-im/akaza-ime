use std::cell::RefCell;
use std::sync::Once;

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::WindowsAndMessaging::*;

use crate::globals;

// ---------------------------------------------------------------------------
// 候補ウィンドウのクラス名
// ---------------------------------------------------------------------------

const CLASS_NAME: &str = "AkazaCandidateWindow";
const PAGE_SIZE: usize = 9;

// ---------------------------------------------------------------------------
// 描画用の表示データ (thread-local)
// ---------------------------------------------------------------------------

struct DisplayState {
    /// 現在のページに表示する候補 (最大 PAGE_SIZE 件)
    visible: Vec<String>,
    /// visible 内での選択インデックス
    selected: usize,
    /// ページ情報テキスト (例: "1/3")
    page_info: String,
}

thread_local! {
    static CANDIDATE_HWND: RefCell<HWND> = RefCell::new(HWND::default());
    static DISPLAY_STATE: RefCell<DisplayState> = RefCell::new(DisplayState {
        visible: Vec::new(),
        selected: 0,
        page_info: String::new(),
    });
}

// ---------------------------------------------------------------------------
// ウィンドウクラス登録
// ---------------------------------------------------------------------------

static REGISTER_CLASS: Once = Once::new();

fn ensure_window_class() {
    REGISTER_CLASS.call_once(|| {
        let class_name: Vec<u16> = CLASS_NAME.encode_utf16().chain(std::iter::once(0)).collect();

        let wc = WNDCLASSEXW {
            cbSize: std::mem::size_of::<WNDCLASSEXW>() as u32,
            style: CS_HREDRAW | CS_VREDRAW,
            lpfnWndProc: Some(candidate_wnd_proc),
            hInstance: globals::dll_instance().into(),
            hCursor: unsafe { LoadCursorW(None, IDC_ARROW).unwrap_or_default() },
            hbrBackground: HBRUSH(unsafe { GetStockObject(WHITE_BRUSH) }.0),
            lpszClassName: PCWSTR(class_name.as_ptr()),
            ..Default::default()
        };

        unsafe {
            RegisterClassExW(&wc);
        }
    });
}

// ---------------------------------------------------------------------------
// ウィンドウプロシージャ
// ---------------------------------------------------------------------------

unsafe extern "system" fn candidate_wnd_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    match msg {
        WM_PAINT => {
            paint_candidates(hwnd);
            LRESULT(0)
        }
        WM_ERASEBKGND => LRESULT(1),
        _ => unsafe { DefWindowProcW(hwnd, msg, wparam, lparam) },
    }
}

// ---------------------------------------------------------------------------
// システムメッセージフォント取得
// ---------------------------------------------------------------------------

fn create_candidate_font() -> HFONT {
    unsafe {
        let mut ncm = NONCLIENTMETRICSW {
            cbSize: std::mem::size_of::<NONCLIENTMETRICSW>() as u32,
            ..Default::default()
        };
        if SystemParametersInfoW(
            SPI_GETNONCLIENTMETRICS,
            ncm.cbSize,
            Some(&mut ncm as *mut _ as *mut _),
            SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0),
        )
        .is_ok()
        {
            let font = CreateFontIndirectW(&ncm.lfMessageFont);
            if !font.is_invalid() {
                return font;
            }
        }

        // フォールバック
        let font_name: Vec<u16> = "Meiryo UI\0".encode_utf16().collect();
        CreateFontW(
            -16,
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            SHIFTJIS_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            (FF_DONTCARE.0 | DEFAULT_PITCH.0) as u32,
            PCWSTR(font_name.as_ptr()),
        )
    }
}

// ---------------------------------------------------------------------------
// 描画
// ---------------------------------------------------------------------------

const PADDING_X: i32 = 6;
const PADDING_Y: i32 = 3;
const HIGHLIGHT_COLOR: COLORREF = COLORREF(0x00CC6633); // BGR: 青系
const HIGHLIGHT_TEXT: COLORREF = COLORREF(0x00FFFFFF);
const NORMAL_TEXT: COLORREF = COLORREF(0x00000000);
const BORDER_COLOR: COLORREF = COLORREF(0x00999999);
const PAGE_INFO_COLOR: COLORREF = COLORREF(0x00888888);

fn paint_candidates(hwnd: HWND) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let hdc = BeginPaint(hwnd, &mut ps);
        if hdc.is_invalid() {
            return;
        }

        // 背景を白で塗りつぶし
        let mut client_rc = RECT::default();
        let _ = GetClientRect(hwnd, &mut client_rc);
        let bg_brush = CreateSolidBrush(COLORREF(0x00FFFFFF));
        FillRect(hdc, &client_rc, bg_brush);
        let _ = DeleteObject(bg_brush.into());

        let font = create_candidate_font();
        let old_font = SelectObject(hdc, font.into());

        let mut tm = TEXTMETRICW::default();
        let _ = GetTextMetricsW(hdc, &mut tm);
        let line_height = tm.tmHeight + tm.tmExternalLeading + PADDING_Y;

        let _ = SetBkMode(hdc, TRANSPARENT);

        DISPLAY_STATE.with(|ds| {
            let ds = ds.borrow();
            let mut y = PADDING_Y;

            for (i, cand) in ds.visible.iter().enumerate() {
                let label = format!("{}. {}", i + 1, cand);
                let label_w: Vec<u16> = label.encode_utf16().collect();

                if i == ds.selected {
                    let _ = GetClientRect(hwnd, &mut client_rc);
                    let rc = RECT {
                        left: 0,
                        top: y - 1,
                        right: client_rc.right,
                        bottom: y + line_height - PADDING_Y + 1,
                    };

                    let brush = CreateSolidBrush(HIGHLIGHT_COLOR);
                    FillRect(hdc, &rc, brush);
                    let _ = DeleteObject(brush.into());
                    SetTextColor(hdc, HIGHLIGHT_TEXT);
                } else {
                    SetTextColor(hdc, NORMAL_TEXT);
                }

                let _ = TextOutW(hdc, PADDING_X, y, &label_w);
                y += line_height;
            }

            // ページ情報
            if !ds.page_info.is_empty() {
                SetTextColor(hdc, PAGE_INFO_COLOR);
                let info_w: Vec<u16> = ds.page_info.encode_utf16().collect();
                let _ = TextOutW(hdc, PADDING_X, y, &info_w);
            }
        });

        SelectObject(hdc, old_font);
        let _ = DeleteObject(font.into());

        // 枠線
        let _ = GetClientRect(hwnd, &mut client_rc);
        let border_brush = CreateSolidBrush(BORDER_COLOR);
        FrameRect(hdc, &client_rc, border_brush);
        let _ = DeleteObject(border_brush.into());

        let _ = EndPaint(hwnd, &ps);
    }
}

// ---------------------------------------------------------------------------
// ウィンドウサイズ計算
// ---------------------------------------------------------------------------

fn compute_window_size(visible: &[String], has_page_info: bool) -> (i32, i32) {
    if visible.is_empty() {
        return (0, 0);
    }

    unsafe {
        let hdc = GetDC(None);
        let font = create_candidate_font();
        let old_font = SelectObject(hdc, font.into());

        let mut tm = TEXTMETRICW::default();
        let _ = GetTextMetricsW(hdc, &mut tm);
        let line_height = tm.tmHeight + tm.tmExternalLeading + PADDING_Y;

        let mut max_width: i32 = 0;
        for (i, cand) in visible.iter().enumerate() {
            let label = format!("{}. {}", i + 1, cand);
            let label_w: Vec<u16> = label.encode_utf16().collect();
            let mut size = SIZE::default();
            let _ = GetTextExtentPoint32W(hdc, &label_w, &mut size);
            if size.cx > max_width {
                max_width = size.cx;
            }
        }

        SelectObject(hdc, old_font);
        let _ = DeleteObject(font.into());
        ReleaseDC(None, hdc);

        let lines = visible.len() + if has_page_info { 1 } else { 0 };
        let width = max_width + PADDING_X * 2 + 4;
        let height = line_height * lines as i32 + PADDING_Y * 2;
        (width, height)
    }
}

// ---------------------------------------------------------------------------
// CandidateWindow
// ---------------------------------------------------------------------------

pub struct CandidateWindow {
    hwnd: HWND,
}

impl CandidateWindow {
    pub fn new() -> Self {
        Self {
            hwnd: HWND::default(),
        }
    }

    fn ensure_window(&mut self) {
        if !self.hwnd.is_invalid() && self.hwnd.0 != std::ptr::null_mut() {
            return;
        }

        ensure_window_class();

        let class_name: Vec<u16> = CLASS_NAME.encode_utf16().chain(std::iter::once(0)).collect();

        let hwnd = unsafe {
            CreateWindowExW(
                WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
                PCWSTR(class_name.as_ptr()),
                PCWSTR::null(),
                WS_POPUP,
                0,
                0,
                1,
                1,
                None,
                None,
                Some(globals::dll_instance().into()),
                None,
            )
        };

        self.hwnd = hwnd.unwrap_or_default();
        CANDIDATE_HWND.with(|h| *h.borrow_mut() = self.hwnd);
    }

    pub fn show(&mut self, candidates: &[String], selected: usize, x: i32, y: i32) {
        if candidates.is_empty() {
            self.hide();
            return;
        }

        self.ensure_window();
        if self.hwnd.is_invalid() || self.hwnd.0 == std::ptr::null_mut() {
            return;
        }

        // ページング: 選択中の候補が含まれるページを計算
        let total_pages = (candidates.len() + PAGE_SIZE - 1) / PAGE_SIZE;
        let current_page = selected / PAGE_SIZE;
        let page_start = current_page * PAGE_SIZE;
        let page_end = (page_start + PAGE_SIZE).min(candidates.len());
        let visible: Vec<String> = candidates[page_start..page_end].to_vec();
        let selected_in_page = selected - page_start;
        let has_page_info = total_pages > 1;
        let page_info = if has_page_info {
            format!("{}/{}", current_page + 1, total_pages)
        } else {
            String::new()
        };

        // 表示データを更新
        DISPLAY_STATE.with(|ds| {
            let mut ds = ds.borrow_mut();
            ds.visible = visible.clone();
            ds.selected = selected_in_page;
            ds.page_info = page_info;
        });

        let (width, height) = compute_window_size(&visible, has_page_info);

        // 画面外にはみ出さないよう調整
        let (mut pos_x, mut pos_y) = (x, y);
        unsafe {
            let screen_w = GetSystemMetrics(SM_CXSCREEN);
            let screen_h = GetSystemMetrics(SM_CYSCREEN);
            if pos_x + width > screen_w {
                pos_x = screen_w - width;
            }
            if pos_y + height > screen_h {
                pos_y = y - height - 20;
            }

            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                pos_x,
                pos_y,
                width,
                height,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
            let _ = InvalidateRect(Some(self.hwnd), None, true);
        }
    }

    pub fn hide(&mut self) {
        if !self.hwnd.is_invalid() && self.hwnd.0 != std::ptr::null_mut() {
            unsafe {
                let _ = ShowWindow(self.hwnd, SW_HIDE);
            }
        }
    }

    pub fn destroy(&mut self) {
        if !self.hwnd.is_invalid() && self.hwnd.0 != std::ptr::null_mut() {
            unsafe {
                let _ = DestroyWindow(self.hwnd);
            }
            self.hwnd = HWND::default();
            CANDIDATE_HWND.with(|h| *h.borrow_mut() = HWND::default());
        }
    }
}

/// CompositionSink や OnSetFocus など、AkazaTextService の外から候補ウィンドウを非表示にする
pub fn hide_candidate_window() {
    CANDIDATE_HWND.with(|h| {
        let hwnd = *h.borrow();
        if !hwnd.is_invalid() && hwnd.0 != std::ptr::null_mut() {
            unsafe {
                let _ = ShowWindow(hwnd, SW_HIDE);
            }
        }
    });
}
