use windows::Win32::UI::WindowsAndMessaging::{
    GetForegroundWindow, GetWindowTextW, GetWindowThreadProcessId,
};

use std::time::Duration;
use windows::{
    Win32::System::SystemInformation::GetTickCount,
    Win32::UI::Input::KeyboardAndMouse::{GetLastInputInfo, LASTINPUTINFO},
};

pub fn get_last_input() -> Duration {
    let tick_count = unsafe { GetTickCount() };
    let mut last_input_info = LASTINPUTINFO {
        cbSize: 8,
        dwTime: 0,
    };

    let p_last_input_info = &mut last_input_info as *mut LASTINPUTINFO;

    let _success = unsafe { GetLastInputInfo(p_last_input_info) };
    let diff = tick_count - last_input_info.dwTime;
    return Duration::from_millis(diff.into());
}

pub fn get_active_window() -> (u32, String) {
    unsafe {
        let hwnd = GetForegroundWindow();

        let mut pid: u32 = 0;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));

        let mut bytes: [u16; 500] = [0; 500];
        let len = GetWindowTextW(hwnd, &mut bytes);
        let title = String::from_utf16_lossy(&bytes[..len as usize]);

        (pid, title)
    }
}
