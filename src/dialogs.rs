#[cfg(not(target_os = "windows"))]
use std::process::Command;

pub fn format_duration(hours: f64) -> String {
    if hours < 1.0 {
        let mins = (hours * 60.0).round() as i32;
        format!("{mins} minutes")
    } else if hours == 1.0 {
        "1 hour".to_string()
    } else if hours < 24.0 {
        format!("{} hours", hours as i32)
    } else {
        let days = (hours / 24.0).round() as i32;
        if days == 1 {
            "1 day".to_string()
        } else {
            format!("{days} days")
        }
    }
}

pub fn parse_duration(s: &str) -> Option<f64> {
    let parts: Vec<&str> = s.split_whitespace().collect();
    if parts.len() < 2 {
        return None;
    }
    let val: f64 = parts[0].parse().ok()?;
    match parts[1] {
        "minutes" | "minute" | "mins" | "min" => Some(val / 60.0),
        "hour" | "hours" | "h" => Some(val),
        "day" | "days" | "d" => Some(val * 24.0),
        _ => None,
    }
}

pub fn input_dialog(title: &str, prompt: &str, default: &str) -> Option<String> {
    input_dialog_impl(title, prompt, default)
}

pub fn open_file_dialog(title: &str, filename: &str) -> Option<String> {
    open_file_impl(title, filename)
}

pub fn open_files_dialog(title: &str) -> Option<Vec<String>> {
    open_files_impl(title)
}

pub fn select_list(
    title: &str,
    text: &str,
    options: &[String],
    selected: Option<&str>,
) -> Option<String> {
    select_list_impl(title, text, options, selected)
}

#[cfg(target_os = "linux")]
fn input_dialog_impl(title: &str, prompt: &str, default: &str) -> Option<String> {
    let output = Command::new("zenity")
        .arg("--entry")
        .arg("--title")
        .arg(title)
        .arg("--text")
        .arg(prompt)
        .arg("--entry-text")
        .arg(default)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let s = String::from_utf8(output.stdout).ok()?;
    let s = s.trim_end_matches('\n').to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

#[cfg(target_os = "windows")]
fn input_dialog_impl(title: &str, prompt: &str, default: &str) -> Option<String> {
    win32::windows_input_dialog(title, prompt, default)
}

#[cfg(target_os = "macos")]
fn input_dialog_impl(title: &str, prompt: &str, default: &str) -> Option<String> {
    let script = format!(
        "text returned of (display dialog \"{}\" default answer \"{}\" with title \"{}\")",
        macos_escape(prompt),
        macos_escape(default),
        macos_escape(title),
    );

    let output = Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let s = String::from_utf8(output.stdout).ok()?;
    let s = s.trim_end_matches('\n').to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

#[cfg(target_os = "linux")]
fn open_file_impl(title: &str, filename: &str) -> Option<String> {
    let mut cmd = Command::new("zenity");
    cmd.arg("--file-selection").arg("--title").arg(title);

    if !filename.is_empty() {
        cmd.arg("--filename").arg(filename);
    }

    let output = cmd.output().ok()?;

    if !output.status.success() {
        return None;
    }

    let s = String::from_utf8(output.stdout).ok()?;
    let s = s.trim_end_matches('\n').to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

#[cfg(not(target_os = "linux"))]
fn open_file_impl(title: &str, filename: &str) -> Option<String> {
    use rfd::FileDialog;

    let mut dialog = FileDialog::new().set_title(title);
    if !filename.is_empty() {
        dialog = dialog.set_file_name(filename);
    }
    dialog.pick_file().map(|p| p.to_string_lossy().into_owned())
}

#[cfg(target_os = "linux")]
fn open_files_impl(title: &str) -> Option<Vec<String>> {
    let output = Command::new("zenity")
        .arg("--file-selection")
        .arg("--title")
        .arg(title)
        .arg("--multiple")
        .arg("--separator")
        .arg("\n")
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let s = String::from_utf8(output.stdout).ok()?;
    let files: Vec<String> = s.lines().map(|l| l.to_string()).filter(|l| !l.is_empty()).collect();

    if files.is_empty() {
        None
    } else {
        Some(files)
    }
}

#[cfg(not(target_os = "linux"))]
fn open_files_impl(title: &str) -> Option<Vec<String>> {
    use rfd::FileDialog;

    FileDialog::new()
        .set_title(title)
        .pick_files()
        .map(|files| {
            files
                .iter()
                .map(|p| p.to_string_lossy().into_owned())
                .collect()
        })
}

#[cfg(target_os = "linux")]
fn select_list_impl(
    title: &str,
    text: &str,
    options: &[String],
    _selected: Option<&str>,
) -> Option<String> {
    let mut cmd = Command::new("zenity");
    cmd.arg("--list")
        .arg("--title")
        .arg(title)
        .arg("--column")
        .arg("")
        .arg("--hide-header")
        .arg("--print-column")
        .arg("1");

    if !text.is_empty() {
        cmd.arg("--text").arg(text);
    }

    for opt in options {
        cmd.arg(opt);
    }

    let output = cmd.output().ok()?;

    if !output.status.success() {
        return None;
    }

    let s = String::from_utf8(output.stdout).ok()?;
    let s = s.trim_end_matches('\n').to_string();
    if s.is_empty() {
        None
    } else {
        Some(s)
    }
}

#[cfg(target_os = "windows")]
fn select_list_impl(
    title: &str,
    text: &str,
    options: &[String],
    selected: Option<&str>,
) -> Option<String> {
    win32::windows_select_dialog(title, text, options, selected)
}

#[cfg(target_os = "macos")]
fn select_list_impl(
    title: &str,
    text: &str,
    options: &[String],
    _selected: Option<&str>,
) -> Option<String> {
    let items: Vec<String> = options
        .iter()
        .map(|o| format!("\"{}\"", macos_escape(o)))
        .collect();

    let mut script = format!(
        "choose from list {{{}}} with title \"{}\"",
        items.join(", "),
        macos_escape(title),
    );

    if !text.is_empty() {
        script.push_str(&format!(" with prompt \"{}\"", macos_escape(text)));
    }

    let output = Command::new("osascript")
        .arg("-e")
        .arg(&script)
        .output()
        .ok()?;

    if !output.status.success() {
        return None;
    }

    let s = String::from_utf8(output.stdout).ok()?;
    let s = s.trim_end_matches('\n').to_string();
    if s == "false" || s.is_empty() {
        None
    } else {
        Some(s)
    }
}

#[cfg(target_os = "macos")]
fn macos_escape(s: &str) -> String {
    s.replace('\\', "\\\\")
        .replace('"', "\\\"")
        .replace('\n', "\\n")
}

#[cfg(target_os = "windows")]
mod win32 {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Foundation::*;
    use windows::Win32::Graphics::Gdi::HBRUSH;
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::UI::Input::KeyboardAndMouse::{SetActiveWindow, SetFocus};
    use windows::Win32::UI::WindowsAndMessaging::*;

    const COLOR_BTNFACE: i32 = 15;
    const SS_LEFT: i32 = 0;

    #[inline]
    fn loword(val: u32) -> u16 {
        val as u16
    }

    fn ws(style: i32) -> WINDOW_STYLE {
        WINDOW_STYLE(style as u32)
    }

    fn exws(style: i32) -> WINDOW_EX_STYLE {
        WINDOW_EX_STYLE(style as u32)
    }

    fn hmenu(id: usize) -> HMENU {
        HMENU(id as *mut core::ffi::c_void)
    }

    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    fn read_edit(hwnd: HWND, id: i32) -> String {
        unsafe {
            let edit = GetDlgItem(Some(hwnd), id).unwrap_or_default();
            let len =
                SendMessageW(edit, WM_GETTEXTLENGTH, Some(WPARAM(0)), Some(LPARAM(0)));
            let mut buf = vec![0u16; len.0 as usize + 1];
            SendMessageW(
                edit,
                WM_GETTEXT,
                Some(WPARAM(buf.len())),
                Some(LPARAM(buf.as_mut_ptr() as isize)),
            );
            String::from_utf16_lossy(&buf[..len.0 as usize])
        }
    }

    fn show_dialog(hwnd: HWND, width: i32, height: i32) {
        unsafe {
            let sw = GetSystemMetrics(SM_CXSCREEN);
            let sh = GetSystemMetrics(SM_CYSCREEN);
            let x = ((sw - width) / 2).max(0);
            let y = ((sh - height) / 3).max(0);
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), x, y, width, height, SWP_SHOWWINDOW);
            let _ = SetForegroundWindow(hwnd);
            let _ = SetActiveWindow(hwnd);
        }
    }

    fn run_dialog(hwnd: HWND, focus: Option<HWND>) -> Option<String> {
        unsafe {
            if let Some(f) = focus {
                let _ = SetFocus(Some(f));
            }
            let mut msg = MSG::default();
            loop {
                let ret = GetMessageW(&mut msg, None, 0, 0);
                if !ret.as_bool() {
                    let ptr = GetWindowLongPtrW(hwnd, GWLP_USERDATA);
                    if ptr != 0 {
                        break Some(*Box::from_raw(ptr as *mut String));
                    }
                    break None;
                }
                if !IsDialogMessageW(hwnd, &msg).as_bool() {
                    let _ = TranslateMessage(&msg);
                    DispatchMessageW(&msg);
                }
            }
        }
    }

    pub fn windows_input_dialog(title: &str, prompt: &str, default: &str) -> Option<String> {
        unsafe {
            let title_wide = wide(title);
            let prompt_wide = wide(prompt);
            let default_wide = wide(default);
            let static_class = wide("STATIC");
            let edit_class = wide("EDIT");
            let button_class = wide("BUTTON");
            let class_name = wide("JuiceboxPlusInputDlg");
            let instance = GetModuleHandleW(None).unwrap_or_default();

            extern "system" fn dlg_proc(
                hwnd: HWND,
                msg: u32,
                wparam: WPARAM,
                lparam: LPARAM,
            ) -> LRESULT {
                unsafe {
                    match msg {
                        WM_COMMAND => {
                            let id = loword(wparam.0 as u32) as u32;
                            if id == 2 {
                                let text = read_edit(hwnd, 101);
                                SetWindowLongPtrW(
                                    hwnd,
                                    GWLP_USERDATA,
                                    Box::into_raw(Box::new(text)) as isize,
                                );
                                let _ = DestroyWindow(hwnd);
                            } else if id == 1 {
                                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                                let _ = DestroyWindow(hwnd);
                            }
                        }
                        WM_DESTROY => {
                            PostQuitMessage(0);
                        }
                        _ => return DefWindowProcW(hwnd, msg, wparam, lparam),
                    }
                    LRESULT(0)
                }
            }

            let hinst = HINSTANCE(instance.0);

            let wc = WNDCLASSW {
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(dlg_proc),
                hInstance: hinst,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                hbrBackground: HBRUSH((COLOR_BTNFACE + 1) as *mut core::ffi::c_void),
                lpszClassName: PCWSTR::from_raw(class_name.as_ptr()),
                ..Default::default()
            };
            let _ = RegisterClassW(&wc);

            let hwnd = CreateWindowExW(
                exws(WS_EX_DLGMODALFRAME.0 as i32),
                PCWSTR::from_raw(class_name.as_ptr()),
                PCWSTR::from_raw(title_wide.as_ptr()),
                ws(WS_CAPTION.0 as i32 | WS_SYSMENU.0 as i32),
                200,
                200,
                380,
                180,
                None,
                None,
                Some(hinst),
                None,
            )
            .ok();

            let Some(hwnd) = hwnd else {
                return None;
            };

            let _ = CreateWindowExW(
                exws(0),
                PCWSTR::from_raw(static_class.as_ptr()),
                PCWSTR::from_raw(prompt_wide.as_ptr()),
                ws(WS_CHILD.0 as i32 | WS_VISIBLE.0 as i32 | SS_LEFT),
                10,
                10,
                340,
                40,
                Some(hwnd),
                Some(hmenu(3)),
                Some(hinst),
                None,
            );

            let edit = CreateWindowExW(
                exws(WS_EX_CLIENTEDGE.0 as i32),
                PCWSTR::from_raw(edit_class.as_ptr()),
                PCWSTR::from_raw(default_wide.as_ptr()),
                ws(WS_CHILD.0 as i32 | WS_VISIBLE.0 as i32 | WS_BORDER.0 as i32 | ES_AUTOHSCROLL),
                10,
                60,
                340,
                24,
                Some(hwnd),
                Some(hmenu(101)),
                Some(hinst),
                None,
            )
            .ok();

            let Some(edit) = edit else {
                return None;
            };

            let _ = CreateWindowExW(
                exws(0),
                PCWSTR::from_raw(button_class.as_ptr()),
                PCWSTR::from_raw(HSTRING::from("Cancel").as_ptr() as *const u16),
                ws(WS_CHILD.0 as i32 | WS_VISIBLE.0 as i32 | BS_PUSHBUTTON),
                260,
                110,
                90,
                28,
                Some(hwnd),
                Some(hmenu(1)),
                Some(hinst),
                None,
            );

            let _ = CreateWindowExW(
                exws(0),
                PCWSTR::from_raw(button_class.as_ptr()),
                PCWSTR::from_raw(HSTRING::from("OK").as_ptr() as *const u16),
                ws(WS_CHILD.0 as i32 | WS_VISIBLE.0 as i32 | BS_DEFPUSHBUTTON),
                160,
                110,
                90,
                28,
                Some(hwnd),
                Some(hmenu(2)),
                Some(hinst),
                None,
            );

            show_dialog(hwnd, 380, 180);
            run_dialog(hwnd, Some(edit))
        }
    }

    pub fn windows_select_dialog(
        title: &str,
        prompt: &str,
        options: &[String],
        selected: Option<&str>,
    ) -> Option<String> {
        unsafe {
            let title_wide = wide(title);
            let prompt_wide = wide(prompt);
            let static_class = wide("STATIC");
            let combo_class = wide("COMBOBOX");
            let button_class = wide("BUTTON");
            let class_name = wide("JuiceboxPlusSelectDlg");
            let instance = GetModuleHandleW(None).unwrap_or_default();

            extern "system" fn dlg_proc(
                hwnd: HWND,
                msg: u32,
                wparam: WPARAM,
                lparam: LPARAM,
            ) -> LRESULT {
                unsafe {
                    match msg {
                        WM_COMMAND => {
                            let id = loword(wparam.0 as u32) as u32;
                            if id == 2 {
                                let combo = GetDlgItem(Some(hwnd), 101).unwrap_or_default();
                                let cur = SendMessageW(
                                    combo,
                                    CB_GETCURSEL,
                                    Some(WPARAM(0)),
                                    Some(LPARAM(0)),
                                );
                                if cur.0 != (CB_ERR as isize) {
                                    let len = SendMessageW(
                                        combo,
                                        CB_GETLBTEXTLEN,
                                        Some(WPARAM(cur.0 as usize)),
                                        Some(LPARAM(0)),
                                    );
                                    let mut buf = vec![0u16; len.0 as usize + 1];
                                    SendMessageW(
                                        combo,
                                        CB_GETLBTEXT,
                                        Some(WPARAM(cur.0 as usize)),
                                        Some(LPARAM(buf.as_mut_ptr() as isize)),
                                    );
                                    let text =
                                        String::from_utf16_lossy(&buf[..len.0 as usize]);
                                    SetWindowLongPtrW(
                                        hwnd,
                                        GWLP_USERDATA,
                                        Box::into_raw(Box::new(text)) as isize,
                                    );
                                } else {
                                    SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                                }
                                let _ = DestroyWindow(hwnd);
                            } else if id == 1 {
                                SetWindowLongPtrW(hwnd, GWLP_USERDATA, 0);
                                let _ = DestroyWindow(hwnd);
                            }
                        }
                        WM_DESTROY => {
                            PostQuitMessage(0);
                        }
                        _ => return DefWindowProcW(hwnd, msg, wparam, lparam),
                    }
                    LRESULT(0)
                }
            }

            let hinst = HINSTANCE(instance.0);

            let wc = WNDCLASSW {
                style: CS_HREDRAW | CS_VREDRAW,
                lpfnWndProc: Some(dlg_proc),
                hInstance: hinst,
                hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(),
                hbrBackground: HBRUSH((COLOR_BTNFACE + 1) as *mut core::ffi::c_void),
                lpszClassName: PCWSTR::from_raw(class_name.as_ptr()),
                ..Default::default()
            };
            let _ = RegisterClassW(&wc);

            let hwnd = CreateWindowExW(
                exws(WS_EX_DLGMODALFRAME.0 as i32),
                PCWSTR::from_raw(class_name.as_ptr()),
                PCWSTR::from_raw(title_wide.as_ptr()),
                ws(WS_CAPTION.0 as i32 | WS_SYSMENU.0 as i32),
                200,
                200,
                400,
                220,
                None,
                None,
                Some(hinst),
                None,
            )
            .ok();

            let Some(hwnd) = hwnd else {
                return None;
            };

            let _ = CreateWindowExW(
                exws(0),
                PCWSTR::from_raw(static_class.as_ptr()),
                PCWSTR::from_raw(prompt_wide.as_ptr()),
                ws(WS_CHILD.0 as i32 | WS_VISIBLE.0 as i32 | SS_LEFT),
                10,
                10,
                360,
                40,
                Some(hwnd),
                Some(hmenu(3)),
                Some(hinst),
                None,
            );

            let combo = CreateWindowExW(
                exws(0),
                PCWSTR::from_raw(combo_class.as_ptr()),
                PCWSTR::null(),
                ws(
                    WS_CHILD.0 as i32
                        | WS_VISIBLE.0 as i32
                        | WS_VSCROLL.0 as i32
                        | CBS_DROPDOWNLIST
                        | CBS_HASSTRINGS,
                ),
                10,
                60,
                360,
                120,
                Some(hwnd),
                Some(hmenu(101)),
                Some(hinst),
                None,
            )
            .ok();

            let Some(combo) = combo else {
                return None;
            };

            let preselect = selected.and_then(|s| options.iter().position(|o| o == s));

            for opt in options {
                let wide_opt = wide(opt);
                SendMessageW(
                    combo,
                    CB_ADDSTRING,
                    Some(WPARAM(0)),
                    Some(LPARAM(wide_opt.as_ptr() as isize)),
                );
            }

            if let Some(idx) = preselect {
                SendMessageW(combo, CB_SETCURSEL, Some(WPARAM(idx)), Some(LPARAM(0)));
            }

            let _ = CreateWindowExW(
                exws(0),
                PCWSTR::from_raw(button_class.as_ptr()),
                PCWSTR::from_raw(HSTRING::from("Cancel").as_ptr() as *const u16),
                ws(WS_CHILD.0 as i32 | WS_VISIBLE.0 as i32 | BS_PUSHBUTTON),
                280,
                140,
                90,
                28,
                Some(hwnd),
                Some(hmenu(1)),
                Some(hinst),
                None,
            );

            let _ = CreateWindowExW(
                exws(0),
                PCWSTR::from_raw(button_class.as_ptr()),
                PCWSTR::from_raw(HSTRING::from("OK").as_ptr() as *const u16),
                ws(WS_CHILD.0 as i32 | WS_VISIBLE.0 as i32 | BS_DEFPUSHBUTTON),
                180,
                140,
                90,
                28,
                Some(hwnd),
                Some(hmenu(2)),
                Some(hinst),
                None,
            );

            show_dialog(hwnd, 400, 220);
            run_dialog(hwnd, Some(combo))
        }
    }
}
