use windows::{
    Win32::{
        Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HANDLE},
        System::Threading::CreateMutexW,
    },
    core::w,
};

pub struct SingleInstanceGuard {
    handle: Option<HANDLE>,
}

unsafe impl Send for SingleInstanceGuard {}
unsafe impl Sync for SingleInstanceGuard {}

impl Drop for SingleInstanceGuard {
    fn drop(&mut self) {
        if let Some(handle) = self.handle.take() {
            unsafe {
                let _ = CloseHandle(handle);
            }
        }
    }
}

pub fn acquire() -> Option<SingleInstanceGuard> {
    let result = unsafe { CreateMutexW(None, true, w!("Global\\TrustTunnelUI_SingleInstance")) };

    match result {
        Ok(handle) => {
            if unsafe { GetLastError() } == ERROR_ALREADY_EXISTS {
                unsafe {
                    let _ = CloseHandle(handle);
                }
                return None;
            }

            Some(SingleInstanceGuard {
                handle: Some(handle),
            })
        }
        Err(error) => {
            log::warn!("[single_instance] CreateMutexW failed: {error}");
            Some(SingleInstanceGuard { handle: None })
        }
    }
}

#[cfg(not(debug_assertions))]
pub fn show_already_running_message() {
    use windows::Win32::UI::WindowsAndMessaging::{MB_ICONINFORMATION, MB_OK, MessageBoxW};

    unsafe {
        let _ = MessageBoxW(
            None,
            w!("TrustTunnel UI is already running."),
            w!("TrustTunnel"),
            MB_OK | MB_ICONINFORMATION,
        );
    }
}
