mod class_factory;
mod edit_session;
mod globals;
mod guid;
mod input_state;
mod register;
mod text_service;

use std::mem::ManuallyDrop;

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::SystemServices::*;

use crate::class_factory::AkazaClassFactory;
use crate::guid::CLSID_AKAZA_TEXT_SERVICE;

pub fn dll_log(msg: &str) {
    use std::io::Write;
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(r"C:\dev\akaza-ime\ime_log.txt")
    {
        let _ = writeln!(f, "{msg}");
        let _ = f.flush();
    }
}

#[no_mangle]
pub extern "system" fn DllMain(
    hinst: HMODULE,
    reason: u32,
    _reserved: *mut std::ffi::c_void,
) -> BOOL {
    if reason == DLL_PROCESS_ATTACH {
        dll_log("DllMain: DLL_PROCESS_ATTACH");
        // パニックでホストプロセスを落とさないようにする
        std::panic::set_hook(Box::new(|info| {
            eprintln!("akaza-ime panic: {info}");
        }));
        unsafe {
            globals::set_dll_instance(hinst);
        }
    }
    TRUE
}

#[no_mangle]
pub extern "system" fn DllGetClassObject(
    rclsid: *const GUID,
    riid: *const GUID,
    ppv: *mut *mut std::ffi::c_void,
) -> HRESULT {
    unsafe {
        *ppv = std::ptr::null_mut();

        if *rclsid != CLSID_AKAZA_TEXT_SERVICE {
            return CLASS_E_CLASSNOTAVAILABLE;
        }

        let factory = AkazaClassFactory;
        let unknown: IClassFactory = factory.into();
        let manual = ManuallyDrop::new(unknown);
        manual.query(riid, ppv)
    }
}

#[no_mangle]
pub extern "system" fn DllCanUnloadNow() -> HRESULT {
    if globals::server_lock_count() == 0 {
        S_OK
    } else {
        S_FALSE
    }
}

#[no_mangle]
pub extern "system" fn DllRegisterServer() -> HRESULT {
    match register::register_server() {
        Ok(()) => S_OK,
        Err(e) => e.code(),
    }
}

#[no_mangle]
pub extern "system" fn DllUnregisterServer() -> HRESULT {
    match register::unregister_server() {
        Ok(()) => S_OK,
        Err(e) => e.code(),
    }
}
