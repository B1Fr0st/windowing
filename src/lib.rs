
/// Api Definition:
/// get_window_info(window) -> returns a WindowInfo struct with the position and size of the window
/// find_window_by_pid(target_pid) -> returns the given process's first matching Window
/// find_windows_by_pid(target_pid) -> returns all the given process's matching Windows
/// get_active_window_pid() -> returns the active window's pid


pub struct WindowInfo{
    pub pos: (i32,i32),
    pub size: (u32,u32),
}
#[cfg(target_os="linux")]
mod platform {
    use std::error::Error;
    use crate::WindowInfo;
    use x11rb::{
        connection::Connection,
        protocol::xproto::{AtomEnum, ConnectionExt, GetGeometryReply, Window},
        rust_connection::RustConnection,
    };

    impl Into<WindowInfo> for GetGeometryReply {
        fn into(self) -> WindowInfo {
            WindowInfo { 
                pos: (self.x as i32,self.y as i32), 
                size: (self.width as u32,self.height as u32) 
            }
        }
    }

    /// Get the active (foreground) window ID.
    fn get_active_window(conn: &RustConnection, root: Window) -> Result<Window, Box<dyn std::error::Error>> {
        let net_active_window = conn.intern_atom(false, b"_NET_ACTIVE_WINDOW")?.reply()?.atom;
        let prop = conn.get_property(
            false,
            root,
            net_active_window,
            AtomEnum::WINDOW,
            0,
            1,
        )?.reply()?;
        
        if prop.value_len == 0 || prop.format != 32 {
            return Err("No active window found".into());
        }
        
        // Extract window ID (convert bytes to u32)
        let active_window = prop.value32()
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
    fn get_top_level_windows(conn: &RustConnection, root: Window) -> Result<Vec<Window>, Box<dyn Error>> {
        let client_list_atom = conn.intern_atom(false, b"_NET_CLIENT_LIST")?.reply()?.atom;
        let prop = conn.get_property(
            false,
            root,
            client_list_atom,
            AtomEnum::WINDOW,
            0,
            u32::MAX,
        )?.reply()?;
        
        Ok(prop.value32()
            .ok_or("Failed to read _NET_CLIENT_LIST")?
            .collect())
    }

    /// Get the process ID (PID) of a given window
    fn get_window_pid(conn: &RustConnection, window: Window) -> Result<Option<u32>, Box<dyn Error>> {
        let net_wm_pid_atom = conn.intern_atom(false, b"_NET_WM_PID")?.reply()?.atom;
        
        let reply = conn.get_property(
            false,
            window,
            net_wm_pid_atom,
            AtomEnum::CARDINAL,
            0,
            1,
        )?.reply()?;
        
        if reply.value_len == 0 || reply.format != 32 {
            return Ok(None);
        }
        
        let pid = reply.value32()
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
}

//#[cfg(target_os="windows")]
mod platform {
    use windows::Win32::{
        Foundation::{HWND,LPARAM,},
        UI::WindowsAndMessaging::{
        GetWindowThreadProcessId,
        EnumWindows,
        IsWindowVisible,
        GetWindowTextLengthW
    }};
    struct EnumWindowsData {
        process_id: u32,
        windows: Vec<HWND>,
    }

    // Callback function for EnumWindows
    unsafe extern "system" fn enum_windows_proc(hwnd: HWND, lparam: LPARAM) -> BOOL {
        let data = unsafe { &mut *(lparam as *mut EnumWindowsData) };
        let mut window_process_id: u32 = 0;
        
        // Get the process ID that owns this window
        unsafe { GetWindowThreadProcessId(hwnd, &mut window_process_id) };
        
        // If it matches our target process ID, add it to the list
        if window_process_id == data.process_id {
            data.windows.push(hwnd);
        }
        
        TRUE // Continue enumeration
    }

    pub fn find_windows_by_pid(process_id: u32) -> Vec<HWND> {
        let mut data = EnumWindowsData {
            process_id,
            windows: Vec::new(),
        };
        
        unsafe {
            EnumWindows(
                Some(enum_windows_proc),
                &mut data as *mut _ as LPARAM,
            );
        }
        
        data.windows
    }

    pub fn find_window_by_pid(process_id: u32) -> Option<HWND> {
        let windows = find_window_by_pid(process_id);
        
        for &hwnd in &windows {
            unsafe {
                // Check if window is visible and has a title
                if IsWindowVisible(hwnd) != 0 {
                    let title_length = GetWindowTextLengthW(hwnd);
                    if title_length > 0 {
                        return Some(hwnd); // Return first visible window with title
                    }
                }
            }
        }
        
        // If no main window found, return first window (if any)
        windows.first().copied()
    }

    pub fn get_window_info() -> Option<WindowInfo>{
        todo!();
    }

    pub fn get_active_window_pid() -> Option<u32>{
        todo!();
    }
}


//#[cfg(any(target_os="windows",target_os="linux"))]
pub use platform::*;