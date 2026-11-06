use bilipick::bilibili::{ApiError, QrLogin, QrState};
use bilipick::download::{
    DownloadError, DownloadEvent, DownloadItem, discover_tools, download_item,
};
use bilipick::service::{BiliClient, ResolvedContent};
use bilipick::settings::Settings;
use qrcodegen::{QrCode, QrCodeEcc};
use std::path::PathBuf;
use std::ptr::{null, null_mut};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows_sys::Win32::Graphics::Gdi::{
    BeginPaint, CreateFontW, CreatePen, CreateSolidBrush, DT_CENTER, DT_END_ELLIPSIS,
    DT_SINGLELINE, DT_VCENTER, DeleteObject, DrawTextW, EndPaint, FillRect, HBRUSH, HDC, HFONT,
    InvalidateRect, PAINTSTRUCT, PS_SOLID, RoundRect, SelectObject, SetBkColor, SetBkMode,
    SetTextColor, TRANSPARENT, UpdateWindow,
};
use windows_sys::Win32::System::Com::CoTaskMemFree;
use windows_sys::Win32::System::LibraryLoader::GetModuleHandleW;
use windows_sys::Win32::UI::Controls::{
    DRAWITEMSTRUCT, EM_SETCUEBANNER, EM_SETSEL, ICC_PROGRESS_CLASS, INITCOMMONCONTROLSEX,
    InitCommonControlsEx, MEASUREITEMSTRUCT, ODS_DISABLED, ODS_FOCUS, ODS_SELECTED, PBM_SETPOS,
    PBM_SETRANGE32, PBM_SETSTATE, PBS_SMOOTH, PBST_ERROR, PBST_NORMAL,
};
use windows_sys::Win32::UI::HiDpi::GetDpiForSystem;
use windows_sys::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, GetKeyState, SetFocus, VK_CONTROL,
};
use windows_sys::Win32::UI::Shell::{
    BIF_RETURNONLYFSDIRS, BROWSEINFOW, DefSubclassProc, RemoveWindowSubclass, SHBrowseForFolderW,
    SHGetPathFromIDListW, SetWindowSubclass, ShellExecuteW,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    AdjustWindowRectEx, BM_GETCHECK, BM_SETCHECK, BS_AUTOCHECKBOX, BS_OWNERDRAW, CB_ADDSTRING,
    CB_GETCURSEL, CB_GETITEMDATA, CB_RESETCONTENT, CB_SETCURSEL, CB_SETITEMDATA, CBS_DROPDOWNLIST,
    CBS_HASSTRINGS, CreateWindowExW, DI_NORMAL, DLGC_WANTALLKEYS, DefWindowProcW, DestroyIcon,
    DestroyWindow, DispatchMessageW, DrawIconEx, EN_KILLFOCUS, ES_AUTOHSCROLL, EnumChildWindows,
    GWLP_USERDATA, GetMessageW, GetWindowLongPtrW, GetWindowTextLengthW, GetWindowTextW, HICON,
    ICON_BIG, ICON_SMALL, IDC_ARROW, IDYES, IMAGE_ICON, IsChild, IsDialogMessageW, IsWindow,
    LB_ADDSTRING, LB_GETSEL, LB_GETTEXT, LB_GETTEXTLEN, LB_RESETCONTENT, LB_SETITEMHEIGHT,
    LB_SETSEL, LB_SETTOPINDEX, LBS_EXTENDEDSEL, LBS_HASSTRINGS, LBS_NOINTEGRALHEIGHT, LBS_NOTIFY,
    LBS_OWNERDRAWFIXED, LR_DEFAULTCOLOR, LoadCursorW, LoadImageW, MB_YESNO, MessageBoxW,
    PostMessageW, PostQuitMessage, RegisterClassW, SW_HIDE, SW_SHOW, SW_SHOWNORMAL, SendMessageW,
    SetForegroundWindow, SetProcessDPIAware, SetWindowLongPtrW, SetWindowTextW, ShowWindow,
    TranslateMessage, WM_APP, WM_CHAR, WM_CLOSE, WM_COMMAND, WM_CREATE, WM_CTLCOLOREDIT,
    WM_CTLCOLORLISTBOX, WM_CTLCOLORSTATIC, WM_DESTROY, WM_DRAWITEM, WM_GETDLGCODE, WM_KEYDOWN,
    WM_MEASUREITEM, WM_NCCREATE, WM_NCDESTROY, WM_PAINT, WM_SETFONT, WM_SETICON, WNDCLASSW,
    WS_CAPTION, WS_CHILD, WS_CLIPCHILDREN, WS_MINIMIZEBOX, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE,
    WS_VSCROLL,
};

const MAIN_CLASS: &str = "BiliPickMainWindow";
const LOGIN_CLASS: &str = "BiliPickLoginWindow";
const MAIN_EVENT: u32 = WM_APP + 1;
const LOGIN_EVENT: u32 = WM_APP + 2;

const ID_INPUT: usize = 101;
const ID_PARSE: usize = 102;
const ID_LIST: usize = 103;
const ID_QUALITY: usize = 104;
const ID_OUTPUT: usize = 105;
const ID_BROWSE: usize = 106;
const ID_FFMPEG: usize = 107;
const ID_START: usize = 108;
const ID_CANCEL: usize = 109;
const ID_RETRY: usize = 110;
const ID_OPEN_FOLDER: usize = 111;
const ID_STATUS: usize = 112;
const ID_PROGRESS: usize = 113;
const ID_TITLE: usize = 114;
const ID_MEDIA_LABEL: usize = 116;
const ID_QUALITY_LABEL: usize = 117;
const ID_OUTPUT_LABEL: usize = 118;
const ID_FFMPEG_LABEL: usize = 119;
const ID_REMEMBER: usize = 120;
const ID_LOGOUT: usize = 121;
const ID_LOGIN: usize = 122;
const ID_LOGIN_STATUS_TEXT: usize = 201;
const ID_REFRESH: usize = 202;
const ID_LOGIN_MODE_TEXT: usize = 203;

const BACKGROUND: u32 = 0x00fa_f8f5;
const SURFACE: u32 = 0x00ff_ffff;
const INK: u32 = 0x003e_302b;
const MUTED: u32 = 0x0080_736c;
const BORDER: u32 = 0x00e5_ded8;
const ACCENT: u32 = 0x00a0_7550;

fn scaled(value: i32) -> i32 {
    unsafe { value * GetDpiForSystem() as i32 / 96 }
}

#[derive(Default)]
struct Controls {
    title: HWND,
    input: HWND,
    parse: HWND,
    list: HWND,
    quality: HWND,
    output: HWND,
    browse: HWND,
    ffmpeg: HWND,
    start: HWND,
    cancel: HWND,
    retry: HWND,
    open_folder: HWND,
    progress: HWND,
    status: HWND,
    login_status: HWND,
    remember: HWND,
    login: HWND,
    logout: HWND,
}

struct App {
    client: Arc<Mutex<BiliClient>>,
    running: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    controls: Controls,
    resolved: Option<ResolvedContent>,
    failed_indices: Vec<usize>,
    last_failure: Option<String>,
    pending_job: Option<DownloadJob>,
    login_window: HWND,
    font: HFONT,
    brand_font: HFONT,
    small_font: HFONT,
    background: HBRUSH,
    surface: HBRUSH,
    icon: HICON,
    small_icon: HICON,
    settings: Settings,
}

impl Drop for App {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        unsafe {
            for object in [
                self.font,
                self.brand_font,
                self.small_font,
                self.background,
                self.surface,
            ] {
                DeleteObject(object);
            }
            DestroyIcon(self.icon);
            DestroyIcon(self.small_icon);
        }
    }
}

#[derive(Clone)]
struct DownloadJob {
    entries: Vec<(usize, DownloadItem)>,
    quality: u32,
    output: PathBuf,
    ffmpeg: String,
}

enum UiEvent {
    Parsed(ResolvedContent),
    ParseFailed(String),
    LoginRequired(DownloadJob),
    LoginSucceeded {
        remember: bool,
        saved: Result<(), String>,
    },
    SessionRestored(Result<bool, String>),
    ItemStarted {
        position: usize,
        total: usize,
        label: String,
    },
    Status(String),
    Progress {
        current: u64,
        total: Option<u64>,
        speed: u64,
    },
    ItemFinished {
        index: usize,
        path: PathBuf,
    },
    ItemFailed {
        index: usize,
        message: String,
    },
    DownloadFinished {
        cancelled: bool,
    },
}

pub fn run() {
    unsafe {
        SetProcessDPIAware();
        let controls = INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_PROGRESS_CLASS,
        };
        InitCommonControlsEx(&controls);

        let instance = GetModuleHandleW(null());
        let background = CreateSolidBrush(BACKGROUND);
        let surface = CreateSolidBrush(SURFACE);
        let face = wide("Microsoft YaHei UI");
        let font = CreateFontW(
            -scaled(16),
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            1,
            0,
            0,
            5,
            0,
            face.as_ptr(),
        );
        let brand_font = CreateFontW(
            -scaled(28),
            0,
            0,
            0,
            700,
            0,
            0,
            0,
            1,
            0,
            0,
            5,
            0,
            face.as_ptr(),
        );
        let small_font = CreateFontW(
            -scaled(13),
            0,
            0,
            0,
            400,
            0,
            0,
            0,
            1,
            0,
            0,
            5,
            0,
            face.as_ptr(),
        );
        let icon = LoadImageW(
            instance,
            1_usize as _,
            IMAGE_ICON,
            scaled(72),
            scaled(72),
            LR_DEFAULTCOLOR,
        );
        let small_icon = LoadImageW(
            instance,
            1_usize as _,
            IMAGE_ICON,
            scaled(16),
            scaled(16),
            LR_DEFAULTCOLOR,
        );
        let class_name = wide(MAIN_CLASS);
        let window_class = WNDCLASSW {
            lpfnWndProc: Some(main_wnd_proc),
            hInstance: instance,
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hIcon: small_icon,
            hbrBackground: background,
            lpszClassName: class_name.as_ptr(),
            ..Default::default()
        };
        RegisterClassW(&window_class);

        let app = Box::new(App {
            client: Arc::new(Mutex::new(BiliClient::default())),
            running: Arc::new(AtomicBool::new(false)),
            cancel: Arc::new(AtomicBool::new(false)),
            controls: Controls::default(),
            resolved: None,
            failed_indices: Vec::new(),
            last_failure: None,
            pending_job: None,
            login_window: null_mut(),
            font,
            brand_font,
            small_font,
            background,
            surface,
            icon,
            small_icon,
            settings: Settings::default(),
        });
        let style = WS_CAPTION | WS_SYSMENU | WS_MINIMIZEBOX | WS_CLIPCHILDREN;
        let mut bounds = RECT {
            left: 0,
            top: 0,
            right: scaled(944),
            bottom: scaled(772),
        };
        AdjustWindowRectEx(&mut bounds, style, 0, 0);
        let title = wide("哔取 BiliPick");
        let window = CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            style,
            100,
            70,
            bounds.right - bounds.left,
            bounds.bottom - bounds.top,
            null_mut(),
            null_mut(),
            instance,
            Box::into_raw(app).cast(),
        );
        if window.is_null() {
            return;
        }
        SendMessageW(window, WM_SETICON, ICON_SMALL as usize, small_icon as isize);
        SendMessageW(window, WM_SETICON, ICON_BIG as usize, icon as isize);
        ShowWindow(window, SW_SHOW);
        UpdateWindow(window);
        SetFocus(app_controls(window).input);

        let mut message = std::mem::zeroed();
        while GetMessageW(&mut message, null_mut(), 0, 0) > 0 {
            let login = (*app_ptr(window)).login_window;
            let dialog = if IsWindow(login) != 0
                && (message.hwnd == login || IsChild(login, message.hwnd) != 0)
            {
                login
            } else {
                window
            };
            if IsDialogMessageW(dialog, &message) == 0 {
                TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }
}

fn app_controls(window: HWND) -> &'static mut Controls {
    unsafe { &mut (*app_ptr(window)).controls }
}

unsafe fn app_ptr(window: HWND) -> *mut App {
    unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) as *mut App }
}

unsafe extern "system" fn main_wnd_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        match message {
            WM_NCCREATE => {
                let create =
                    &*(lparam as *const windows_sys::Win32::UI::WindowsAndMessaging::CREATESTRUCTW);
                SetWindowLongPtrW(window, GWLP_USERDATA, create.lpCreateParams as isize);
                DefWindowProcW(window, message, wparam, lparam)
            }
            WM_CREATE => {
                if !app_ptr(window).is_null() {
                    create_main_controls(window, &mut (*app_ptr(window)).controls);
                    initialize_status(window);
                    restore_login(window);
                }
                0
            }
            WM_COMMAND => {
                let id = wparam & 0xffff;
                if !app_ptr(window).is_null() {
                    let app = &mut *app_ptr(window);
                    if id == ID_OUTPUT && wparam >> 16 == EN_KILLFOCUS as usize {
                        if let Err(error) = save_preferences(app) {
                            set_status(app.controls.status, &error);
                        }
                    } else {
                        handle_command(window, app, id);
                    }
                }
                0
            }
            WM_PAINT => {
                paint_main(window);
                0
            }
            WM_DRAWITEM => {
                let item = &*(lparam as *const DRAWITEMSTRUCT);
                if item.CtlID as usize == ID_LIST {
                    draw_list_item(item);
                } else {
                    draw_button(item);
                }
                1
            }
            WM_MEASUREITEM if wparam == ID_LIST => {
                (*(lparam as *mut MEASUREITEMSTRUCT)).itemHeight = scaled(30) as u32;
                1
            }
            WM_CTLCOLORSTATIC | WM_CTLCOLOREDIT | WM_CTLCOLORLISTBOX => {
                let app = &*app_ptr(window);
                let dc = wparam as HDC;
                let is_field = message != WM_CTLCOLORSTATIC;
                SetTextColor(
                    dc,
                    if lparam as HWND == app.controls.login_status {
                        MUTED
                    } else {
                        INK
                    },
                );
                SetBkMode(dc, TRANSPARENT as i32);
                SetBkColor(dc, if is_field { SURFACE } else { BACKGROUND });
                if is_field {
                    app.surface as isize
                } else {
                    app.background as isize
                }
            }
            MAIN_EVENT => {
                if lparam != 0 && !app_ptr(window).is_null() {
                    let event = *Box::from_raw(lparam as *mut UiEvent);
                    handle_event(window, &mut *app_ptr(window), event);
                }
                0
            }
            WM_CLOSE => {
                if let Err(error) = save_preferences(&mut *app_ptr(window)) {
                    let text = wide(&error);
                    let caption = wide("无法保存设置");
                    MessageBoxW(window, text.as_ptr(), caption.as_ptr(), 0);
                }
                DestroyWindow(window);
                0
            }
            WM_DESTROY => {
                PostQuitMessage(0);
                0
            }
            WM_NCDESTROY => {
                let pointer = GetWindowLongPtrW(window, GWLP_USERDATA) as *mut App;
                SetWindowLongPtrW(window, GWLP_USERDATA, 0);
                if !pointer.is_null() {
                    drop(Box::from_raw(pointer));
                }
                DefWindowProcW(window, message, wparam, lparam)
            }
            _ => DefWindowProcW(window, message, wparam, lparam),
        }
    }
}

unsafe fn create_main_controls(window: HWND, controls: &mut Controls) {
    let instance = unsafe { GetModuleHandleW(null()) };
    let base = WS_CHILD | WS_VISIBLE;
    let edit = base | WS_TABSTOP | ES_AUTOHSCROLL as u32;
    let button = base | WS_TABSTOP | BS_OWNERDRAW as u32;
    unsafe {
        controls.title = child(
            window,
            "STATIC",
            "哔取 BiliPick",
            base,
            ID_TITLE,
            116,
            34,
            360,
            40,
            instance,
        );
        controls.login_status = child(
            window,
            "STATIC",
            "未登录",
            base,
            ID_FFMPEG_LABEL,
            630,
            26,
            168,
            26,
            instance,
        );
        controls.remember = child(
            window,
            "BUTTON",
            "记住登录",
            base | WS_TABSTOP | BS_AUTOCHECKBOX as u32,
            ID_REMEMBER,
            630,
            60,
            168,
            28,
            instance,
        );
        controls.logout = child(
            window,
            "BUTTON",
            "退出登录",
            button,
            ID_LOGOUT,
            810,
            34,
            106,
            40,
            instance,
        );
        controls.login = child(
            window,
            "BUTTON",
            "扫码登录",
            button,
            ID_LOGIN,
            810,
            34,
            106,
            40,
            instance,
        );
        child(
            window,
            "STATIC",
            "视频链接",
            base,
            ID_MEDIA_LABEL,
            28,
            120,
            150,
            24,
            instance,
        );
        controls.input = child(
            window, "EDIT", "", edit, ID_INPUT, 42, 163, 704, 24, instance,
        );
        controls.parse = child(
            window,
            "BUTTON",
            "解析链接",
            button,
            ID_PARSE,
            778,
            152,
            138,
            46,
            instance,
        );
        child(
            window,
            "STATIC",
            "下载内容",
            base,
            ID_QUALITY_LABEL,
            28,
            220,
            140,
            24,
            instance,
        );
        let hint = child(
            window,
            "STATIC",
            "按住 Ctrl / Shift 可多选",
            base,
            123,
            694,
            224,
            222,
            20,
            instance,
        );
        controls.list = child(
            window,
            "LISTBOX",
            "",
            base | WS_VSCROLL
                | WS_TABSTOP
                | (LBS_NOTIFY
                    | LBS_EXTENDEDSEL
                    | LBS_NOINTEGRALHEIGHT
                    | LBS_OWNERDRAWFIXED
                    | LBS_HASSTRINGS) as u32,
            ID_LIST,
            38,
            264,
            868,
            212,
            instance,
        );
        child(
            window,
            "STATIC",
            "下载画质",
            base,
            ID_OUTPUT_LABEL,
            28,
            510,
            180,
            24,
            instance,
        );
        controls.quality = child(
            window,
            "COMBOBOX",
            "",
            base | WS_VSCROLL | WS_TABSTOP | (CBS_DROPDOWNLIST | CBS_HASSTRINGS) as u32,
            ID_QUALITY,
            28,
            543,
            192,
            200,
            instance,
        );
        child(
            window,
            "STATIC",
            "保存目录",
            base,
            ID_FFMPEG_LABEL + 10,
            244,
            510,
            180,
            24,
            instance,
        );
        controls.output = child(
            window, "EDIT", "", edit, ID_OUTPUT, 258, 551, 540, 24, instance,
        );
        controls.browse = child(
            window, "BUTTON", "浏览", button, ID_BROWSE, 828, 540, 88, 46, instance,
        );
        child(
            window,
            "STATIC",
            "ffmpeg 目录",
            base,
            130,
            28,
            607,
            124,
            24,
            instance,
        );
        controls.ffmpeg = child(
            window, "EDIT", "", edit, ID_FFMPEG, 166, 607, 736, 24, instance,
        );
        controls.start = child(
            window,
            "BUTTON",
            "开始下载",
            button,
            ID_START,
            28,
            662,
            148,
            44,
            instance,
        );
        controls.cancel = child(
            window, "BUTTON", "取消", button, ID_CANCEL, 188, 662, 88, 44, instance,
        );
        controls.retry = child(
            window,
            "BUTTON",
            "重试失败项",
            button,
            ID_RETRY,
            288,
            662,
            132,
            44,
            instance,
        );
        controls.open_folder = child(
            window,
            "BUTTON",
            "打开目录",
            button,
            ID_OPEN_FOLDER,
            432,
            662,
            112,
            44,
            instance,
        );
        controls.progress = child(
            window,
            "msctls_progress32",
            "",
            base | PBS_SMOOTH,
            ID_PROGRESS,
            568,
            678,
            348,
            12,
            instance,
        );
        SendMessageW(controls.progress, PBM_SETRANGE32, 0, 1000);
        controls.status = child(
            window,
            "STATIC",
            "粘贴 BV 号、视频链接或合集链接，然后点击“解析”。",
            base,
            ID_STATUS,
            28,
            722,
            888,
            44,
            instance,
        );
        let app = &*app_ptr(window);
        EnumChildWindows(window, Some(set_font), app.font as isize);
        SendMessageW(controls.title, WM_SETFONT, app.brand_font as usize, 1);
        for control in [hint, controls.status] {
            SendMessageW(control, WM_SETFONT, app.small_font as usize, 1);
        }
        SendMessageW(controls.list, LB_SETITEMHEIGHT, 0, scaled(30) as isize);
        let cue = wide("粘贴 BV 号、视频链接或合集链接");
        SendMessageW(controls.input, EM_SETCUEBANNER, 0, cue.as_ptr() as isize);
        let cue = wide("留空时自动从系统 PATH 查找");
        SendMessageW(controls.ffmpeg, EM_SETCUEBANNER, 0, cue.as_ptr() as isize);
        EnableWindow(controls.cancel, 0);
        EnableWindow(controls.retry, 0);
        EnableWindow(controls.logout, 0);
        ShowWindow(controls.logout, SW_HIDE);
    }
}

unsafe extern "system" fn set_font(control: HWND, font: LPARAM) -> i32 {
    unsafe {
        SendMessageW(control, WM_SETFONT, font as usize, 1);
    }
    1
}

#[allow(clippy::too_many_arguments)]
unsafe fn child(
    parent: HWND,
    class: &str,
    text: &str,
    style: u32,
    id: usize,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
    instance: *mut std::ffi::c_void,
) -> HWND {
    let class = wide(class);
    let text = wide(text);
    unsafe {
        let control = CreateWindowExW(
            0,
            class.as_ptr(),
            text.as_ptr(),
            style,
            scaled(x),
            scaled(y),
            scaled(width),
            scaled(height),
            parent,
            id as isize as _,
            instance,
            null(),
        );
        if class == wide("EDIT") {
            SetWindowSubclass(control, Some(edit_subclass), 1, 0);
        }
        control
    }
}

unsafe extern "system" fn edit_subclass(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    _id: usize,
    _data: usize,
) -> LRESULT {
    // Standard Win32 EDIT controls do not implement Ctrl+A themselves.
    unsafe {
        if (message == WM_KEYDOWN && wparam == b'A' as usize && GetKeyState(VK_CONTROL as i32) < 0)
            || (message == WM_CHAR && wparam == 1)
        {
            SendMessageW(window, EM_SETSEL, 0, -1);
            return 0;
        }
        if message == WM_GETDLGCODE && wparam == b'A' as usize && GetKeyState(VK_CONTROL as i32) < 0
        {
            return DefSubclassProc(window, message, wparam, lparam) | DLGC_WANTALLKEYS as isize;
        }
        if message == WM_NCDESTROY {
            RemoveWindowSubclass(window, Some(edit_subclass), 1);
        }
        DefSubclassProc(window, message, wparam, lparam)
    }
}

unsafe fn paint_main(window: HWND) {
    unsafe {
        let mut paint: PAINTSTRUCT = std::mem::zeroed();
        let dc = BeginPaint(window, &mut paint);
        let app = &*app_ptr(window);
        FillRect(dc, &paint.rcPaint, app.background);
        let pen = CreatePen(PS_SOLID, scaled(1), BORDER);
        let old_pen = SelectObject(dc, pen);
        let old_brush = SelectObject(dc, app.surface);
        for (left, top, right, bottom) in [
            (28, 152, 760, 198),
            (28, 254, 916, 490),
            (244, 540, 812, 586),
            (152, 596, 916, 642),
        ] {
            RoundRect(
                dc,
                scaled(left),
                scaled(top),
                scaled(right),
                scaled(bottom),
                scaled(8),
                scaled(8),
            );
        }
        SelectObject(dc, old_brush);
        SelectObject(dc, old_pen);
        DeleteObject(pen);
        let line = RECT {
            left: scaled(28),
            top: scaled(106),
            right: scaled(916),
            bottom: scaled(107),
        };
        let brush = CreateSolidBrush(BORDER);
        FillRect(dc, &line, brush);
        DeleteObject(brush);

        DrawIconEx(
            dc,
            scaled(28),
            scaled(16),
            app.icon,
            scaled(72),
            scaled(72),
            0,
            null_mut(),
            DI_NORMAL,
        );
        EndPaint(window, &paint);
    }
}

unsafe fn draw_button(item: &DRAWITEMSTRUCT) {
    unsafe {
        let primary = item.CtlID as usize == ID_START || item.CtlID as usize == ID_PARSE;
        let disabled = item.itemState & ODS_DISABLED != 0;
        let pressed = item.itemState & ODS_SELECTED != 0;
        let color = if disabled {
            BACKGROUND
        } else if pressed {
            BORDER
        } else if primary {
            ACCENT
        } else {
            SURFACE
        };
        let brush = CreateSolidBrush(color);
        let pen = CreatePen(
            PS_SOLID,
            scaled(1),
            if primary && !disabled { ACCENT } else { BORDER },
        );
        let old_brush = SelectObject(item.hDC, brush);
        let old_pen = SelectObject(item.hDC, pen);
        RoundRect(
            item.hDC,
            item.rcItem.left,
            item.rcItem.top,
            item.rcItem.right,
            item.rcItem.bottom,
            scaled(8),
            scaled(8),
        );
        SetBkMode(item.hDC, TRANSPARENT as i32);
        SetTextColor(
            item.hDC,
            if disabled {
                MUTED
            } else if primary && !pressed {
                SURFACE
            } else {
                INK
            },
        );
        let label = wide(&get_text(item.hwndItem));
        let mut rect = item.rcItem;
        DrawTextW(
            item.hDC,
            label.as_ptr(),
            -1,
            &mut rect,
            DT_CENTER | DT_VCENTER | DT_SINGLELINE,
        );
        if item.itemState & ODS_FOCUS != 0 {
            let focus = RECT {
                left: rect.left + scaled(12),
                right: rect.right - scaled(12),
                top: rect.bottom - scaled(5),
                bottom: rect.bottom - scaled(3),
            };
            let focus_brush = CreateSolidBrush(if primary { SURFACE } else { ACCENT });
            FillRect(item.hDC, &focus, focus_brush);
            DeleteObject(focus_brush);
        }
        SelectObject(item.hDC, old_brush);
        SelectObject(item.hDC, old_pen);
        DeleteObject(brush);
        DeleteObject(pen);
    }
}

unsafe fn draw_list_item(item: &DRAWITEMSTRUCT) {
    unsafe {
        let selected = item.itemState & ODS_SELECTED != 0;
        let brush = CreateSolidBrush(if selected { 0x00fa_f0e8 } else { SURFACE });
        FillRect(item.hDC, &item.rcItem, brush);
        DeleteObject(brush);
        if item.itemID == u32::MAX {
            return;
        }
        let length = SendMessageW(item.hwndItem, LB_GETTEXTLEN, item.itemID as usize, 0);
        if length < 0 {
            return;
        }
        let mut text = vec![0_u16; length as usize + 1];
        SendMessageW(
            item.hwndItem,
            LB_GETTEXT,
            item.itemID as usize,
            text.as_mut_ptr() as isize,
        );
        SetTextColor(item.hDC, if selected { ACCENT } else { INK });
        SetBkMode(item.hDC, TRANSPARENT as i32);
        let mut rect = item.rcItem;
        rect.left += scaled(10);
        rect.right -= scaled(10);
        DrawTextW(
            item.hDC,
            text.as_ptr(),
            -1,
            &mut rect,
            DT_VCENTER | DT_SINGLELINE | DT_END_ELLIPSIS,
        );
        if item.itemState & ODS_FOCUS != 0 {
            let bar = RECT {
                left: item.rcItem.left,
                right: item.rcItem.left + scaled(3),
                top: item.rcItem.top + scaled(5),
                bottom: item.rcItem.bottom - scaled(5),
            };
            let brush = CreateSolidBrush(ACCENT);
            FillRect(item.hDC, &bar, brush);
            DeleteObject(brush);
        }
    }
}

unsafe fn initialize_status(window: HWND) {
    unsafe {
        let app = &mut *app_ptr(window);
        match Settings::load() {
            Ok(settings) => app.settings = settings,
            Err(error) => set_status(app.controls.status, &format!("{error}，本次使用默认设置。")),
        }
        set_text(
            app.controls.output,
            &app.settings.output_directory.to_string_lossy(),
        );
        SendMessageW(
            app.controls.remember,
            BM_SETCHECK,
            app.settings.remember_login as usize,
            0,
        );
        set_account(&app.controls, false, false);
        if let Ok(tools) = discover_tools(None)
            && let Some(directory) = tools.ffmpeg.parent()
        {
            set_text(app.controls.ffmpeg, &directory.to_string_lossy());
        }
    }
}

unsafe fn restore_login(window: HWND) {
    unsafe {
        let app = &mut *app_ptr(window);
        if !app.settings.remember_login
            || !bilipick::session::session_path().is_ok_and(|path| path.exists())
        {
            return;
        }
        app.running.store(true, Ordering::Relaxed);
        set_busy(&app.controls, true, false);
        set_text(app.controls.login_status, "正在恢复登录……");
        let client = Arc::clone(&app.client);
        let target = window as isize;
        std::thread::spawn(move || {
            let result = lock_client(&client).restore_session();
            post_main(target, UiEvent::SessionRestored(result));
        });
    }
}

unsafe fn save_preferences(app: &mut App) -> Result<(), String> {
    let directory = get_text(app.controls.output);
    if !directory.trim().is_empty() {
        let directory = PathBuf::from(directory.trim());
        app.settings.output_directory = if directory.is_absolute() {
            directory
        } else {
            std::env::current_dir()
                .map_err(|error| error.to_string())?
                .join(directory)
        };
    }
    app.settings.save()
}

unsafe fn set_account(controls: &Controls, logged_in: bool, remembered: bool) {
    unsafe {
        set_text(
            controls.login_status,
            if logged_in {
                if remembered {
                    "已登录 · 已记住"
                } else {
                    "已登录 · 本次有效"
                }
            } else {
                "未登录"
            },
        );
        ShowWindow(controls.login, if logged_in { SW_HIDE } else { SW_SHOW });
        ShowWindow(controls.logout, if logged_in { SW_SHOW } else { SW_HIDE });
        EnableWindow(controls.login, (!logged_in) as i32);
        EnableWindow(controls.logout, logged_in as i32);
    }
}

unsafe fn handle_command(window: HWND, app: &mut App, id: usize) {
    unsafe {
        match id {
            ID_PARSE => start_parse(window, app),
            ID_LOGIN if !app.running.load(Ordering::Relaxed) => open_login(window, app),
            ID_BROWSE => {
                if let Some(path) = choose_folder(window) {
                    set_text(app.controls.output, &path.to_string_lossy());
                    if let Err(error) = save_preferences(app) {
                        set_status(app.controls.status, &error);
                    }
                }
            }
            ID_START => start_selected(window, app, false),
            ID_CANCEL => {
                app.cancel.store(true, Ordering::Relaxed);
                set_status(app.controls.status, "正在取消当前任务……");
            }
            ID_RETRY => start_selected(window, app, true),
            ID_OPEN_FOLDER => {
                let directory = get_text(app.controls.output);
                let operation = wide("open");
                let path = wide(&directory);
                ShellExecuteW(
                    window,
                    operation.as_ptr(),
                    path.as_ptr(),
                    null(),
                    null(),
                    SW_SHOWNORMAL,
                );
            }
            ID_REMEMBER => {
                let remember = SendMessageW(app.controls.remember, BM_GETCHECK, 0, 0) != 0;
                let result = if remember {
                    let client = lock_client(&app.client);
                    if client.is_logged_in() {
                        client.remember_session()
                    } else {
                        Ok(())
                    }
                } else {
                    bilipick::session::forget()
                };
                if let Err(error) = result {
                    SendMessageW(
                        app.controls.remember,
                        BM_SETCHECK,
                        app.settings.remember_login as usize,
                        0,
                    );
                    set_status(app.controls.status, &error);
                } else {
                    app.settings.remember_login = remember;
                    let logged_in = get_text(app.controls.login_status).starts_with("已登录");
                    set_account(&app.controls, logged_in, remember);
                    if let Err(error) = save_preferences(app) {
                        set_status(app.controls.status, &error);
                        return;
                    }
                    set_status(
                        app.controls.status,
                        if remember {
                            "登录成功后，将为当前 Windows 用户加密保存登录记录。"
                        } else {
                            "已关闭记住登录并删除保存的记录，当前登录可继续使用。"
                        },
                    );
                }
            }
            ID_LOGOUT => {
                if app.running.load(Ordering::Relaxed) {
                    return;
                }
                match lock_client(&app.client).logout() {
                    Ok(()) => {
                        set_account(&app.controls, false, false);
                        set_status(
                            app.controls.status,
                            "已退出登录。保存目录和记住登录偏好已保留。",
                        );
                    }
                    Err(error) => set_status(app.controls.status, &error),
                }
            }
            _ => {}
        }
    }
}

unsafe fn start_parse(window: HWND, app: &mut App) {
    let input = get_text(app.controls.input);
    unsafe {
        if app.running.load(Ordering::Relaxed) {
            return;
        }
        if input.trim().is_empty() {
            set_status(app.controls.status, "请先输入链接或 BV 号。");
            return;
        }
        app.running.store(true, Ordering::Relaxed);
        app.pending_job = None;
        set_busy(&app.controls, true, false);
        set_status(app.controls.status, "正在解析，请稍候……");
    }
    let client = Arc::clone(&app.client);
    let target = window as isize;
    let _ = std::thread::Builder::new()
        .stack_size(1024 * 1024)
        .spawn(move || {
            let result = lock_client(&client).resolve(&input);
            match result {
                Ok(content) => post_main(target, UiEvent::Parsed(content)),
                Err(error) => post_main(target, UiEvent::ParseFailed(error.to_string())),
            }
        });
}

unsafe fn start_selected(window: HWND, app: &mut App, retry_only: bool) {
    unsafe {
        if app.running.load(Ordering::Relaxed) {
            return;
        }
        let Some(resolved) = app.resolved.as_ref() else {
            set_status(app.controls.status, "请先解析链接。");
            return;
        };
        let indices = if retry_only {
            app.failed_indices.clone()
        } else {
            selected_indices(app.controls.list, resolved.items.len())
        };
        if indices.is_empty() {
            set_status(app.controls.status, "请至少选择一个下载项目。");
            return;
        }
        let ffmpeg = get_text(app.controls.ffmpeg);
        if let Err(error) = discover_tools(Some(&ffmpeg)) {
            set_status(app.controls.status, &error.to_string());
            return;
        }
        let output = PathBuf::from(get_text(app.controls.output));
        let quality = selected_quality(app.controls.quality);
        let entries = indices
            .into_iter()
            .filter_map(|index| resolved.items.get(index).cloned().map(|item| (index, item)))
            .collect();
        let job = DownloadJob {
            entries,
            quality,
            output,
            ffmpeg,
        };
        app.pending_job = None;
        app.failed_indices.clear();
        app.last_failure = None;
        launch_job(window, app, job);
    }
}

unsafe fn launch_job(window: HWND, app: &mut App, job: DownloadJob) {
    unsafe {
        app.running.store(true, Ordering::Relaxed);
        app.cancel.store(false, Ordering::Relaxed);
        set_busy(&app.controls, true, true);
        set_status(app.controls.status, "正在准备下载……");
    }
    let client = Arc::clone(&app.client);
    let cancel = Arc::clone(&app.cancel);
    let target = window as isize;
    let _ = std::thread::Builder::new()
        .stack_size(1024 * 1024)
        .spawn(move || {
            let tools = match discover_tools(Some(&job.ffmpeg)) {
                Ok(tools) => tools,
                Err(error) => {
                    post_main(target, UiEvent::ParseFailed(error.to_string()));
                    post_main(target, UiEvent::DownloadFinished { cancelled: false });
                    return;
                }
            };
            let total = job.entries.len();
            let mut cancelled = false;
            let mut client = lock_client(&client);
            for (position, (index, item)) in job.entries.iter().enumerate() {
                if cancel.load(Ordering::Relaxed) {
                    cancelled = true;
                    break;
                }
                post_main(
                    target,
                    UiEvent::ItemStarted {
                        position: position + 1,
                        total,
                        label: item.part.clone(),
                    },
                );
                let mut last_update = Instant::now();
                let mut last_bytes = 0_u64;
                let mut last_speed_time = Instant::now();
                let mut callback = |event: DownloadEvent| match event {
                    DownloadEvent::Status(text) => post_main(target, UiEvent::Status(text)),
                    DownloadEvent::Progress { current, total } => {
                        if last_update.elapsed() >= Duration::from_millis(100) {
                            let elapsed = last_speed_time.elapsed().as_secs_f64();
                            let speed = if elapsed > 0.0 && current >= last_bytes {
                                ((current - last_bytes) as f64 / elapsed) as u64
                            } else {
                                0
                            };
                            last_bytes = current;
                            last_speed_time = Instant::now();
                            last_update = Instant::now();
                            post_main(
                                target,
                                UiEvent::Progress {
                                    current,
                                    total,
                                    speed,
                                },
                            );
                        }
                    }
                };
                match download_item(
                    &mut client,
                    item,
                    job.quality,
                    &job.output,
                    &tools,
                    &cancel,
                    &mut callback,
                ) {
                    Ok(result) => post_main(
                        target,
                        UiEvent::ItemFinished {
                            index: *index,
                            path: result.path,
                        },
                    ),
                    Err(DownloadError::Cancelled) => {
                        cancelled = true;
                        break;
                    }
                    Err(DownloadError::Api(ApiError::LoginRequired)) => {
                        let remaining = DownloadJob {
                            entries: job.entries[position..].to_vec(),
                            ..job.clone()
                        };
                        drop(client);
                        post_main(target, UiEvent::LoginRequired(remaining));
                        return;
                    }
                    Err(error) => {
                        post_main(
                            target,
                            UiEvent::ItemFailed {
                                index: *index,
                                message: error.to_string(),
                            },
                        );
                    }
                }
            }
            post_main(target, UiEvent::DownloadFinished { cancelled });
        });
}

unsafe fn handle_event(window: HWND, app: &mut App, event: UiEvent) {
    unsafe {
        match event {
            UiEvent::Parsed(content) => {
                populate_content(app, &content);
                set_status(
                    app.controls.status,
                    &format!(
                        "解析完成：{}，共 {} 个分 P。{}",
                        content.title,
                        content.items.len(),
                        content
                            .access_notice
                            .as_ref()
                            .map(|notice| format!("\r\n状态提示：{notice}"))
                            .unwrap_or_default()
                    ),
                );
                app.running.store(false, Ordering::Relaxed);
                set_busy(&app.controls, false, false);
            }
            UiEvent::ParseFailed(message) => {
                set_status(app.controls.status, &format!("解析失败：{message}"));
                set_progress(&app.controls, 0, false);
                app.running.store(false, Ordering::Relaxed);
                set_busy(&app.controls, false, false);
            }
            UiEvent::SessionRestored(result) => {
                app.running.store(false, Ordering::Relaxed);
                set_busy(&app.controls, false, false);
                match result {
                    Ok(true) => {
                        set_account(&app.controls, true, true);
                        set_status(app.controls.status, "已恢复登录。粘贴链接即可解析。");
                    }
                    Ok(false) => {
                        set_account(&app.controls, false, false);
                        set_status(
                            app.controls.status,
                            "登录记录已失效，下载需要登录的内容时可重新扫码。",
                        );
                    }
                    Err(message) => {
                        set_account(&app.controls, false, false);
                        set_status(app.controls.status, &message);
                    }
                }
            }
            UiEvent::LoginRequired(remaining) => {
                app.running.store(false, Ordering::Relaxed);
                set_busy(&app.controls, false, false);
                app.pending_job = Some(remaining);
                set_account(&app.controls, false, false);
                set_status(app.controls.status, "下载已暂停，等待登录。");
                let text = wide(
                    "该内容需要登录后才能获取播放地址。\r\n\r\n是否现在使用哔哩哔哩 App 扫码登录？",
                );
                let caption = wide("需要登录");
                let answer = MessageBoxW(window, text.as_ptr(), caption.as_ptr(), MB_YESNO);
                if answer == IDYES {
                    open_login(window, app);
                } else {
                    set_status(app.controls.status, "已取消登录。需要登录的项目尚未完成。");
                }
            }
            UiEvent::LoginSucceeded { remember, saved } => {
                app.login_window = null_mut();
                set_account(&app.controls, true, remember && saved.is_ok());
                if let Err(message) = saved {
                    set_status(
                        app.controls.status,
                        &format!("登录成功，但保存登录记录失败：{message}。请点击开始下载。"),
                    );
                    return;
                }
                if let Some(job) = app.pending_job.take() {
                    launch_job(window, app, job);
                } else {
                    set_status(app.controls.status, "登录成功，可以解析或下载内容。");
                }
            }
            UiEvent::ItemStarted {
                position,
                total,
                label,
            } => {
                set_status(
                    app.controls.status,
                    &format!("正在处理 {position}/{total}：{label}"),
                );
            }
            UiEvent::Status(text) => set_status(app.controls.status, &text),
            UiEvent::Progress {
                current,
                total,
                speed,
            } => {
                if let Some(total) = total.filter(|total| *total > 0) {
                    set_progress(
                        &app.controls,
                        (current * 1000 / total).min(1000) as usize,
                        false,
                    );
                }
                set_status(
                    app.controls.status,
                    &format!(
                        "已下载 {} / {}  ·  {}/s",
                        human_size(current),
                        total.map_or_else(|| "未知".to_string(), human_size),
                        human_size(speed)
                    ),
                );
            }
            UiEvent::ItemFinished { index, path } => {
                set_status(app.controls.status, &format!("已完成：{}", path.display()));
                let _ = index;
            }
            UiEvent::ItemFailed { index, message } => {
                if !app.failed_indices.contains(&index) {
                    app.failed_indices.push(index);
                }
                app.last_failure = Some(message.clone());
                set_status(app.controls.status, &format!("下载失败：{message}"));
            }
            UiEvent::DownloadFinished { cancelled } => {
                app.running.store(false, Ordering::Relaxed);
                set_busy(&app.controls, false, false);
                let has_failures = !app.failed_indices.is_empty();
                EnableWindow(app.controls.retry, has_failures as i32);
                set_progress(
                    &app.controls,
                    if cancelled { 0 } else { 1000 },
                    has_failures,
                );
                if cancelled {
                    set_status(
                        app.controls.status,
                        "任务已取消。已下载的临时文件会在下次重试时继续。",
                    );
                } else if has_failures {
                    set_status(
                        app.controls.status,
                        &format!(
                            "下载结束，有 {} 项失败。可点击“重试失败项”。{}",
                            app.failed_indices.len(),
                            app.last_failure
                                .as_ref()
                                .map(|message| format!("\r\n最近错误：{message}"))
                                .unwrap_or_default()
                        ),
                    );
                } else {
                    set_status(app.controls.status, "全部下载、合并和校验完成。");
                }
            }
        }
    }
}

unsafe fn populate_content(app: &mut App, content: &ResolvedContent) {
    unsafe {
        SendMessageW(app.controls.list, LB_RESETCONTENT, 0, 0);
        for (index, item) in content.items.iter().enumerate() {
            let duration = if item.duration >= 60 {
                format!("{}:{:02}", item.duration / 60, item.duration % 60)
            } else {
                format!("{}s", item.duration)
            };
            let number = if content.is_collection {
                format!("{:02}", index + 1)
            } else {
                format!("P{:02}", item.page)
            };
            let label = format!("{number}   {}   ·   {duration}", item.part);
            let label = wide(&label);
            SendMessageW(app.controls.list, LB_ADDSTRING, 0, label.as_ptr() as isize);
            SendMessageW(
                app.controls.list,
                LB_SETSEL,
                (content.is_collection || item.page == content.focus_page) as usize,
                index as isize,
            );
        }
        SendMessageW(app.controls.list, LB_SETTOPINDEX, 0, 0);
        SendMessageW(app.controls.quality, CB_RESETCONTENT, 0, 0);
        for quality in &content.qualities {
            let label = wide(&format!("{}  ({})", quality.label, quality.id));
            let index = SendMessageW(
                app.controls.quality,
                CB_ADDSTRING,
                0,
                label.as_ptr() as isize,
            );
            SendMessageW(
                app.controls.quality,
                CB_SETITEMDATA,
                index as usize,
                quality.id as isize,
            );
        }
        SendMessageW(app.controls.quality, CB_SETCURSEL, 0, 0);
        app.resolved = Some(content.clone());
        app.failed_indices.clear();
        app.last_failure = None;
        EnableWindow(app.controls.retry, 0);
        set_progress(&app.controls, 0, false);
    }
}

fn selected_indices(list: HWND, count: usize) -> Vec<usize> {
    (0..count)
        .filter(|&index| unsafe { SendMessageW(list, LB_GETSEL, index, 0) > 0 })
        .collect()
}

fn selected_quality(combo: HWND) -> u32 {
    unsafe {
        let selected = SendMessageW(combo, CB_GETCURSEL, 0, 0);
        if selected < 0 {
            return 80;
        }
        SendMessageW(combo, CB_GETITEMDATA, selected as usize, 0) as u32
    }
}

unsafe fn set_busy(controls: &Controls, busy: bool, can_cancel: bool) {
    unsafe {
        EnableWindow(controls.parse, (!busy) as i32);
        EnableWindow(controls.start, (!busy) as i32);
        EnableWindow(controls.cancel, (busy && can_cancel) as i32);
        EnableWindow(controls.browse, (!busy) as i32);
        EnableWindow(controls.input, (!busy) as i32);
        EnableWindow(controls.remember, (!busy) as i32);
        EnableWindow(
            controls.login,
            (!busy && !get_text(controls.login_status).starts_with("已登录")) as i32,
        );
        EnableWindow(
            controls.logout,
            (!busy && get_text(controls.login_status).starts_with("已登录")) as i32,
        );
    }
}

unsafe fn set_progress(controls: &Controls, position: usize, error: bool) {
    unsafe {
        SendMessageW(controls.progress, PBM_SETPOS, position, 0);
        SendMessageW(
            controls.progress,
            PBM_SETSTATE,
            (if error { PBST_ERROR } else { PBST_NORMAL }) as usize,
            0,
        );
    }
}

unsafe fn set_status(control: HWND, text: &str) {
    unsafe {
        let text = wide(text);
        SetWindowTextW(control, text.as_ptr());
    }
}

unsafe fn set_text(control: HWND, text: &str) {
    unsafe {
        let text = wide(text);
        SetWindowTextW(control, text.as_ptr());
    }
}

fn get_text(control: HWND) -> String {
    unsafe {
        let length = GetWindowTextLengthW(control);
        if length <= 0 {
            return String::new();
        }
        let mut buffer = vec![0_u16; length as usize + 1];
        GetWindowTextW(control, buffer.as_mut_ptr(), buffer.len() as i32);
        String::from_utf16_lossy(&buffer[..length as usize])
    }
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}

fn choose_folder(owner: HWND) -> Option<PathBuf> {
    unsafe {
        let title = wide("选择保存目录");
        let mut display = [0_u16; 260];
        let browse = BROWSEINFOW {
            hwndOwner: owner,
            pidlRoot: null_mut(),
            pszDisplayName: display.as_mut_ptr(),
            lpszTitle: title.as_ptr(),
            ulFlags: BIF_RETURNONLYFSDIRS,
            lpfn: None,
            lParam: 0,
            iImage: 0,
        };
        let item = SHBrowseForFolderW(&browse);
        if item.is_null() {
            return None;
        }
        let mut path = [0_u16; 32768];
        let ok = SHGetPathFromIDListW(item, path.as_mut_ptr());
        CoTaskMemFree(item.cast());
        if ok == 0 {
            return None;
        }
        let length = path.iter().position(|&value| value == 0).unwrap_or(0);
        Some(PathBuf::from(String::from_utf16_lossy(&path[..length])))
    }
}

fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn lock_client(client: &Arc<Mutex<BiliClient>>) -> MutexGuard<'_, BiliClient> {
    client
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn post_main(target: isize, event: UiEvent) {
    unsafe {
        PostMessageW(
            target as HWND,
            MAIN_EVENT,
            0,
            Box::into_raw(Box::new(event)) as isize,
        );
    }
}

struct LoginWindow {
    owner: HWND,
    client: Arc<Mutex<BiliClient>>,
    cancel: Arc<AtomicBool>,
    qr: Option<QrCode>,
    qr_rect: RECT,
    status: HWND,
    refresh: HWND,
    remember: bool,
}

enum LoginEvent {
    Qr(QrLogin),
    Status(&'static str),
    Expired,
    Failed(String),
    Success,
}

unsafe fn open_login(owner: HWND, app: &mut App) {
    unsafe {
        if IsWindow(app.login_window) != 0 {
            SetForegroundWindow(app.login_window);
            return;
        }
        let instance = GetModuleHandleW(null());
        let class_name = wide(LOGIN_CLASS);
        let window_class = WNDCLASSW {
            lpfnWndProc: Some(login_wnd_proc),
            hInstance: instance,
            hCursor: LoadCursorW(null_mut(), IDC_ARROW),
            hIcon: app.small_icon,
            hbrBackground: app.background,
            lpszClassName: class_name.as_ptr(),
            ..Default::default()
        };
        RegisterClassW(&window_class);
        let login = Box::new(LoginWindow {
            owner,
            client: Arc::clone(&app.client),
            cancel: Arc::new(AtomicBool::new(false)),
            qr: None,
            qr_rect: RECT {
                left: scaled(30),
                top: scaled(60),
                right: scaled(430),
                bottom: scaled(460),
            },
            status: null_mut(),
            refresh: null_mut(),
            remember: app.settings.remember_login,
        });
        let style = WS_CAPTION | WS_SYSMENU | WS_CLIPCHILDREN;
        let mut bounds = RECT {
            left: 0,
            top: 0,
            right: scaled(460),
            bottom: scaled(574),
        };
        AdjustWindowRectEx(&mut bounds, style, 0, 0);
        let title = wide("BiliPick · 扫码登录");
        let window = CreateWindowExW(
            0,
            class_name.as_ptr(),
            title.as_ptr(),
            style,
            250,
            80,
            bounds.right - bounds.left,
            bounds.bottom - bounds.top,
            owner,
            null_mut(),
            instance,
            Box::into_raw(login).cast(),
        );
        if window.is_null() {
            return;
        }
        app.login_window = window;
        EnableWindow(owner, 0);
        ShowWindow(window, SW_SHOW);
        SetForegroundWindow(window);
        UpdateWindow(window);
        start_login_worker(window);
    }
}

unsafe extern "system" fn login_wnd_proc(
    window: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> LRESULT {
    unsafe {
        match message {
            WM_NCCREATE => {
                let create =
                    &*(lparam as *const windows_sys::Win32::UI::WindowsAndMessaging::CREATESTRUCTW);
                SetWindowLongPtrW(window, GWLP_USERDATA, create.lpCreateParams as isize);
                DefWindowProcW(window, message, wparam, lparam)
            }
            WM_CREATE => {
                let login = &mut *login_ptr(window);
                let instance = GetModuleHandleW(null());
                login.status = child(
                    window,
                    "STATIC",
                    "正在生成二维码……",
                    WS_CHILD | WS_VISIBLE,
                    ID_LOGIN_STATUS_TEXT,
                    30,
                    20,
                    400,
                    36,
                    instance,
                );
                login.refresh = child(
                    window,
                    "BUTTON",
                    "刷新二维码",
                    WS_CHILD | WS_VISIBLE | WS_TABSTOP | BS_OWNERDRAW as u32,
                    ID_REFRESH,
                    145,
                    518,
                    160,
                    40,
                    instance,
                );
                let mode = child(
                    window,
                    "STATIC",
                    if login.remember {
                        "本次登录将加密保存，下次自动恢复。"
                    } else {
                        "仅本次有效；可在主界面开启“记住登录”。"
                    },
                    WS_CHILD | WS_VISIBLE,
                    ID_LOGIN_MODE_TEXT,
                    30,
                    474,
                    400,
                    28,
                    instance,
                );
                let app = &*app_ptr(login.owner);
                EnumChildWindows(window, Some(set_font), app.font as isize);
                SendMessageW(mode, WM_SETFONT, app.small_font as usize, 1);
                0
            }
            WM_DRAWITEM => {
                draw_button(&*(lparam as *const DRAWITEMSTRUCT));
                1
            }
            WM_CTLCOLORSTATIC => {
                let app = &*app_ptr((*login_ptr(window)).owner);
                SetTextColor(wparam as HDC, INK);
                SetBkMode(wparam as HDC, TRANSPARENT as i32);
                SetBkColor(wparam as HDC, BACKGROUND);
                app.background as isize
            }
            WM_COMMAND if wparam & 0xffff == ID_REFRESH => {
                let login = &mut *login_ptr(window);
                login.cancel.store(true, Ordering::Relaxed);
                login.cancel = Arc::new(AtomicBool::new(false));
                login.qr = None;
                InvalidateRect(window, null(), 1);
                start_login_worker(window);
                0
            }
            LOGIN_EVENT => {
                if lparam != 0 {
                    let event = *Box::from_raw(lparam as *mut LoginEvent);
                    handle_login_event(window, event);
                }
                0
            }
            WM_PAINT => {
                paint_login(window);
                0
            }
            WM_CLOSE => {
                let login = &*login_ptr(window);
                login.cancel.store(true, Ordering::Relaxed);
                DestroyWindow(window);
                0
            }
            WM_NCDESTROY => {
                let pointer = GetWindowLongPtrW(window, GWLP_USERDATA) as *mut LoginWindow;
                SetWindowLongPtrW(window, GWLP_USERDATA, 0);
                if !pointer.is_null() {
                    (*pointer).cancel.store(true, Ordering::Relaxed);
                    let owner = (*pointer).owner;
                    EnableWindow(owner, 1);
                    (*app_ptr(owner)).login_window = null_mut();
                    SetForegroundWindow(owner);
                    drop(Box::from_raw(pointer));
                }
                DefWindowProcW(window, message, wparam, lparam)
            }
            _ => DefWindowProcW(window, message, wparam, lparam),
        }
    }
}

unsafe fn login_ptr(window: HWND) -> *mut LoginWindow {
    unsafe { GetWindowLongPtrW(window, GWLP_USERDATA) as *mut LoginWindow }
}

unsafe fn start_login_worker(window: HWND) {
    unsafe {
        let login = &*login_ptr(window);
        let client = Arc::clone(&login.client);
        let cancel = Arc::clone(&login.cancel);
        let target = window as isize;
        set_status(login.status, "正在生成二维码……");
        InvalidateRect(window, null(), 1);
        let _ = std::thread::Builder::new()
            .stack_size(1024 * 1024)
            .spawn(move || {
                let login = match lock_client(&client).qr_generate() {
                    Ok(login) => login,
                    Err(error) => {
                        post_login(target, LoginEvent::Failed(error.to_string()));
                        return;
                    }
                };
                let key = login.key.clone();
                post_login(target, LoginEvent::Qr(login));
                loop {
                    if cancel.load(Ordering::Relaxed) {
                        return;
                    }
                    for _ in 0..20 {
                        if cancel.load(Ordering::Relaxed) {
                            return;
                        }
                        std::thread::sleep(Duration::from_millis(100));
                    }
                    match lock_client(&client).qr_poll(&key) {
                        Ok(QrState::Waiting) => post_login(target, LoginEvent::Status("等待扫码")),
                        Ok(QrState::Scanned) => {
                            post_login(target, LoginEvent::Status("已扫码，请在手机上确认"))
                        }
                        Ok(QrState::Expired) => {
                            post_login(target, LoginEvent::Expired);
                            return;
                        }
                        Ok(QrState::Confirmed) => {
                            post_login(target, LoginEvent::Success);
                            return;
                        }
                        Ok(QrState::Failed(message)) => {
                            post_login(target, LoginEvent::Failed(message));
                            return;
                        }
                        Err(error) => {
                            post_login(target, LoginEvent::Failed(error.to_string()));
                            return;
                        }
                    }
                }
            });
    }
}

unsafe fn handle_login_event(window: HWND, event: LoginEvent) {
    unsafe {
        let login = &mut *login_ptr(window);
        match event {
            LoginEvent::Qr(qr_login) => match QrCode::encode_text(&qr_login.url, QrCodeEcc::Medium)
            {
                Ok(qr) => {
                    login.qr = Some(qr);
                    set_status(login.status, "请使用哔哩哔哩 App 扫描二维码");
                    InvalidateRect(window, null(), 1);
                }
                Err(_) => set_status(login.status, "二维码内容过长，无法生成"),
            },
            LoginEvent::Status(text) => set_status(login.status, text),
            LoginEvent::Expired => set_status(login.status, "二维码已过期，请点击“刷新二维码”"),
            LoginEvent::Failed(message) => {
                set_status(login.status, &format!("登录失败：{message}"))
            }
            LoginEvent::Success => {
                set_status(login.status, "登录成功");
                let remember = login.remember;
                let saved = if remember {
                    lock_client(&login.client).remember_session()
                } else {
                    bilipick::session::forget()
                };
                post_main(
                    login.owner as isize,
                    UiEvent::LoginSucceeded { remember, saved },
                );
                DestroyWindow(window);
            }
        }
    }
}

unsafe fn paint_login(window: HWND) {
    unsafe {
        let mut paint: PAINTSTRUCT = std::mem::zeroed();
        let dc = BeginPaint(window, &mut paint);
        let login = &*login_ptr(window);
        let white = CreateSolidBrush(0x00ff_ffff);
        let black = CreateSolidBrush(0);
        FillRect(dc, &login.qr_rect, white);
        if let Some(qr) = &login.qr {
            let side = (login.qr_rect.right - login.qr_rect.left)
                .min(login.qr_rect.bottom - login.qr_rect.top);
            let modules = qr.size() + 8;
            let module = (side / modules).max(1);
            let qr_side = module * modules;
            let left = login.qr_rect.left + (side - qr_side) / 2 + module * 4;
            let top = login.qr_rect.top + (side - qr_side) / 2 + module * 4;
            for y in 0..qr.size() {
                for x in 0..qr.size() {
                    if qr.get_module(x, y) {
                        let rect = RECT {
                            left: left + x * module,
                            top: top + y * module,
                            right: left + (x + 1) * module,
                            bottom: top + (y + 1) * module,
                        };
                        FillRect(dc, &rect, black);
                    }
                }
            }
        }
        DeleteObject(white);
        DeleteObject(black);
        EndPaint(window, &paint);
    }
}

fn post_login(target: isize, event: LoginEvent) {
    unsafe {
        PostMessageW(
            target as HWND,
            LOGIN_EVENT,
            0,
            Box::into_raw(Box::new(event)) as isize,
        );
    }
}
