/// Api Definition:
/// get_window_info(window) -> returns a WindowInfo struct with the position and size of the window
/// find_window_by_pid(target_pid) -> returns the given process's first matching Window
/// find_windows_by_pid(target_pid) -> returns all the given process's matching Windows
/// get_active_window_pid() -> returns the active window's pid

pub struct WindowInfo {
    pub pos: (i32, i32),
    pub size: (u32, u32),
}
#[cfg(target_os = "linux")]
mod platform {
    use crate::WindowInfo;
    use std::error::Error;
    use x11rb::{
        connection::Connection,
        protocol::xproto::{AtomEnum, ConnectionExt, GetGeometryReply, PropMode, Window},
        rust_connection::RustConnection,
    };

    impl Into<WindowInfo> for GetGeometryReply {
        fn into(self) -> WindowInfo {
            WindowInfo {
                pos: (self.x as i32, self.y as i32),
                size: (self.width as u32, self.height as u32),
            }
        }
    }

    /// Get the active (foreground) window ID.
    fn get_active_window(
        conn: &RustConnection,
        root: Window,
    ) -> Result<Window, Box<dyn std::error::Error>> {
        let net_active_window = conn
            .intern_atom(false, b"_NET_ACTIVE_WINDOW")?
            .reply()?
            .atom;
        let prop = conn
            .get_property(false, root, net_active_window, AtomEnum::WINDOW, 0, 1)?
            .reply()?;

        if prop.value_len == 0 || prop.format != 32 {
            return Err("No active window found".into());
        }

        // Extract window ID (convert bytes to u32)
        let active_window = prop
            .value32()
            .ok_or("Failed to parse active window ID")?
            .next()
            .ok_or("Active window property is empty")?;
        Ok(active_window)
    }

    /// Get the geometry (x, y, width, height) of a window.
    pub fn get_window_info(window: Window) -> Result<WindowInfo, Box<dyn std::error::Error>> {
        let (conn, _) = RustConnection::connect(None).unwrap();
        let geom = conn.get_geometry(window)?.reply()?;
        Ok(geom.into())
    }

    /// Get a list of top-level windows from the root window (_NET_CLIENT_LIST)
    fn get_top_level_windows(
        conn: &RustConnection,
        root: Window,
    ) -> Result<Vec<Window>, Box<dyn Error>> {
        let client_list_atom = conn.intern_atom(false, b"_NET_CLIENT_LIST")?.reply()?.atom;
        let prop = conn
            .get_property(false, root, client_list_atom, AtomEnum::WINDOW, 0, u32::MAX)?
            .reply()?;

        Ok(prop
            .value32()
            .ok_or("Failed to read _NET_CLIENT_LIST")?
            .collect())
    }

    /// Get the process ID (PID) of a given window
    fn get_window_pid(
        conn: &RustConnection,
        window: Window,
    ) -> Result<Option<u32>, Box<dyn Error>> {
        let net_wm_pid_atom = conn.intern_atom(false, b"_NET_WM_PID")?.reply()?.atom;

        let reply = conn
            .get_property(false, window, net_wm_pid_atom, AtomEnum::CARDINAL, 0, 1)?
            .reply()?;

        if reply.value_len == 0 || reply.format != 32 {
            return Ok(None);
        }

        let pid = reply
            .value32()
            .ok_or("Failed to parse PID")?
            .next()
            .ok_or("PID property is empty")?;

        Ok(Some(pid))
    }

    /// Search for a window by process ID (exact match)
    pub fn find_window_by_pid(target_pid: u32) -> Result<Option<Window>, Box<dyn Error>> {
        let (conn, screen_num) = RustConnection::connect(None)?;
        let screen = &conn.setup().roots[screen_num];
        let windows = get_top_level_windows(&conn, screen.root)?;

        for window in windows {
            if let Some(pid) = get_window_pid(&conn, window)? {
                if pid == target_pid {
                    return Ok(Some(window));
                }
            }
        }

        Ok(None)
    }

    /// Search for all windows belonging to a specific process ID
    pub fn find_windows_by_pid(target_pid: u32) -> Result<Vec<Window>, Box<dyn Error>> {
        let (conn, screen_num) = RustConnection::connect(None)?;
        let screen = &conn.setup().roots[screen_num];
        let windows = get_top_level_windows(&conn, screen.root)?;
        let mut matching_windows = Vec::new();

        for window in windows {
            if let Some(pid) = get_window_pid(&conn, window)? {
                if pid == target_pid {
                    matching_windows.push(window);
                }
            }
        }

        Ok(matching_windows)
    }

    /// Get the process ID of the currently active window
    pub fn get_active_window_pid() -> Result<Option<u32>, Box<dyn Error>> {
        let (conn, screen_num) = RustConnection::connect(None)?;
        let screen = &conn.setup().roots[screen_num];
        let active_window = get_active_window(&conn, screen.root)?;
        get_window_pid(&conn, active_window)
    }

    pub fn hide_window(window: Window) -> Result<(), Box<dyn std::error::Error>> {
        let (conn, _) = RustConnection::connect(None)?;
        // Unmap the window first
        conn.unmap_window(window)?;
        
        // Get required atoms
        let net_wm_state = conn.intern_atom(false, b"_NET_WM_STATE")?
            .reply()?
            .atom;
        
        let skip_taskbar = conn.intern_atom(false, b"_NET_WM_STATE_SKIP_TASKBAR")?
            .reply()?
            .atom;
        
        let skip_pager = conn.intern_atom(false, b"_NET_WM_STATE_SKIP_PAGER")?
            .reply()?
            .atom;
        
        // Set both properties to hide from taskbar AND alt-tab
        let properties = [skip_taskbar, skip_pager];
        
        conn.change_property(
            PropMode::REPLACE,
            window,
            net_wm_state,
            AtomEnum::ATOM,
            32,
            properties.len() as u32,
            bytemuck::cast_slice(&properties), // Convert to bytes
        )?;
        
        // Map the window back
        conn.map_window(window)?;
        conn.flush()?;
        
        Ok(())
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use windows::{
        core::BOOL, Win32::{
            Foundation::{FALSE, HWND, LPARAM, RECT, TRUE},
            UI::WindowsAndMessaging::{
                EnumWindows, GetForegroundWindow, GetWindowRect, GetWindowTextLengthW, GetWindowThreadProcessId, IsWindowVisible, SetWindowLongA, ShowWindow, GWL_EXSTYLE, SW_HIDE, SW_SHOW, WS_EX_TOOLWINDOW
            },
        }
    };

    use crate::WindowInfo;
    struct EnumWindowsData {
        process_id: u32,
        windows: Vec<HWND>,
    }
    
    type Window = HWND;

    // Callback function for EnumWindows
    unsafe extern "system" fn enum_windows_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let data = unsafe { &mut *(lparam.0 as *mut EnumWindowsData) };
        let mut window_process_id: u32 = 0;

        // Get the process ID that owns this window
        unsafe { GetWindowThreadProcessId(hwnd, Some(&mut window_process_id)) };

        // If it matches our target process ID, add it to the list
        if window_process_id == data.process_id {
            data.windows.push(hwnd);
        }

        TRUE // Continue enumeration
    }

    pub fn find_windows_by_pid(process_id: u32) -> Result<Vec<Window>, Box<dyn std::error::Error>> {
        let mut data = EnumWindowsData {
            process_id,
            windows: Vec::new(),
        };

        unsafe {
            EnumWindows(
                Some(enum_windows_proc),
                LPARAM(&mut data as *mut _ as isize),
            )?;
        }

        Ok(data.windows)
    }

    pub fn find_window_by_pid(process_id: u32) -> Result<Option<Window>, Box<dyn std::error::Error>> {
        let windows = find_windows_by_pid(process_id)?;

        for &hwnd in &windows {
            unsafe {
                // Check if window is visible and has a title
                if IsWindowVisible(hwnd) != FALSE {
                    let title_length = GetWindowTextLengthW(hwnd);
                    if title_length > 0 {
                        return Ok(Some(hwnd)); // Return first visible window with title
                    }
                }
            }
        }

        // If no main window found, return first window (if any)
        Ok(windows.first().copied())
    }

    pub fn get_window_info(window:Window) -> Result<Option<WindowInfo>, Box<dyn std::error::Error>> {
        let mut window_rect = RECT {
            left: 0,
            top: 0,
            right: 0,
            bottom: 0,
        }; unsafe { GetWindowRect(window, &mut window_rect)?};
        Ok(Some(WindowInfo{
            size: ((window_rect.right - window_rect.left) as u32, (window_rect.bottom - window_rect.top) as u32),
            pos: (window_rect.left, window_rect.top)
        }))
        
    }

    pub fn get_active_window_pid() -> Result<Option<u32>, Box<dyn std::error::Error>> {
        let active_window = unsafe{GetForegroundWindow()};
        let mut pid = 0;
        unsafe{GetWindowThreadProcessId(active_window, Some(&mut pid))};
        Ok(Some(pid))
    }

    pub fn hide_window(window:Window) -> Result<(), Box<dyn std::error::Error>>{
        unsafe {
        ShowWindow(window, SW_HIDE).ok()?;
        SetWindowLongA(window, GWL_EXSTYLE, WS_EX_TOOLWINDOW.0 as i32);
        ShowWindow(window, SW_SHOW).ok()?;
        };
        Ok(())
    }
}

//#[cfg(any(target_os="windows",target_os="linux"))]
pub use platform::*;
