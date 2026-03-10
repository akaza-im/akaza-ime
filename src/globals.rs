use std::sync::atomic::{AtomicU32, Ordering};

/// DLL モジュールハンドル
static mut DLL_INSTANCE: windows::Win32::Foundation::HMODULE =
    windows::Win32::Foundation::HMODULE(std::ptr::null_mut());

/// COM オブジェクトの参照カウント (DllCanUnloadNow 用)
static SERVER_LOCK_COUNT: AtomicU32 = AtomicU32::new(0);

pub fn dll_instance() -> windows::Win32::Foundation::HMODULE {
    unsafe { DLL_INSTANCE }
}

pub unsafe fn set_dll_instance(h: windows::Win32::Foundation::HMODULE) {
    DLL_INSTANCE = h;
}

pub fn server_lock() {
    SERVER_LOCK_COUNT.fetch_add(1, Ordering::SeqCst);
}

pub fn server_unlock() {
    SERVER_LOCK_COUNT.fetch_sub(1, Ordering::SeqCst);
}

pub fn server_lock_count() -> u32 {
    SERVER_LOCK_COUNT.load(Ordering::SeqCst)
}
