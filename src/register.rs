use windows::core::*;
use windows::Win32::System::Com::*;
use windows::Win32::System::Registry::*;
use windows::Win32::UI::TextServices::*;

use crate::globals;
use crate::guid::*;

const LANG_JA: u16 = 0x0411;

/// COM クラスとして登録し、TSF にプロファイルを登録する
pub fn register_server() -> Result<()> {
    crate::dll_log("register: com_server...");
    register_com_server()?;
    crate::dll_log("register: profile...");
    register_profile()?;
    crate::dll_log("register: categories...");
    register_categories()?;
    crate::dll_log("register: done!");
    Ok(())
}

/// 登録を解除する
pub fn unregister_server() -> Result<()> {
    // 各ステップが失敗しても残りを続行する
    let _ = unregister_categories();
    let _ = unregister_profile();
    let _ = unregister_com_server();
    Ok(())
}

fn get_module_path() -> Result<String> {
    let mut buf = [0u16; 260];
    let len = unsafe {
        windows::Win32::System::LibraryLoader::GetModuleFileNameW(
            Some(globals::dll_instance()),
            &mut buf,
        )
    };
    if len == 0 {
        return Err(Error::from_win32());
    }
    Ok(String::from_utf16_lossy(&buf[..len as usize]))
}

fn register_com_server() -> Result<()> {
    let clsid_str = format!("{{{:?}}}", CLSID_AKAZA_TEXT_SERVICE);
    let module_path = get_module_path()?;

    let key_path = format!("CLSID\\{clsid_str}");

    unsafe {
        let mut hkey = HKEY::default();
        let key_path_w: Vec<u16> = key_path.encode_utf16().chain(std::iter::once(0)).collect();

        RegCreateKeyW(HKEY_CLASSES_ROOT, PCWSTR(key_path_w.as_ptr()), &mut hkey).ok()?;

        let desc: Vec<u16> = "Akaza IME\0".encode_utf16().collect();
        let _ = RegSetValueExW(
            hkey,
            None,
            None,
            REG_SZ,
            Some(std::slice::from_raw_parts(
                desc.as_ptr() as *const u8,
                desc.len() * 2,
            )),
        );
        let _ = RegCloseKey(hkey);

        let inproc_path = format!("{key_path}\\InprocServer32");
        let inproc_path_w: Vec<u16> =
            inproc_path.encode_utf16().chain(std::iter::once(0)).collect();

        RegCreateKeyW(
            HKEY_CLASSES_ROOT,
            PCWSTR(inproc_path_w.as_ptr()),
            &mut hkey,
        )
        .ok()?;

        let path_w: Vec<u16> = module_path
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let _ = RegSetValueExW(
            hkey,
            None,
            None,
            REG_SZ,
            Some(std::slice::from_raw_parts(
                path_w.as_ptr() as *const u8,
                path_w.len() * 2,
            )),
        );

        let model_key: Vec<u16> = "ThreadingModel\0".encode_utf16().collect();
        let model_val: Vec<u16> = "Apartment\0".encode_utf16().collect();
        let _ = RegSetValueExW(
            hkey,
            PCWSTR(model_key.as_ptr()),
            None,
            REG_SZ,
            Some(std::slice::from_raw_parts(
                model_val.as_ptr() as *const u8,
                model_val.len() * 2,
            )),
        );
        let _ = RegCloseKey(hkey);
    }

    Ok(())
}

fn unregister_com_server() -> Result<()> {
    let clsid_str = format!("{{{:?}}}", CLSID_AKAZA_TEXT_SERVICE);

    let inproc_path = format!("CLSID\\{clsid_str}\\InprocServer32");
    let inproc_w: Vec<u16> = inproc_path
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let _ = RegDeleteKeyW(HKEY_CLASSES_ROOT, PCWSTR(inproc_w.as_ptr()));
    }

    let key_path = format!("CLSID\\{clsid_str}");
    let key_w: Vec<u16> = key_path.encode_utf16().chain(std::iter::once(0)).collect();
    unsafe {
        let _ = RegDeleteKeyW(HKEY_CLASSES_ROOT, PCWSTR(key_w.as_ptr()));
    }

    Ok(())
}

fn register_profile() -> Result<()> {
    const E_FAIL: HRESULT = HRESULT(0x80004005u32 as i32);

    unsafe {
        let profiles: ITfInputProcessorProfiles =
            CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER)?;

        let hr = (Interface::vtable(&profiles).Register)(
            Interface::as_raw(&profiles),
            &CLSID_AKAZA_TEXT_SERVICE,
        );
        // E_FAIL = already registered, treat as OK
        if hr.is_err() && hr != E_FAIL {
            hr.ok()?;
        }

        let display_name: Vec<u16> = "Akaza\0".encode_utf16().collect();

        // アイコン: DLL 自身に埋め込まれたリソース (インデックス 0)
        let module_path = get_module_path()?;
        let icon_path_w: Vec<u16> = module_path
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();

        // Use raw vtable call — the windows-rs wrapper panics on empty &[] for icon file
        let hr = (Interface::vtable(&profiles).AddLanguageProfile)(
            Interface::as_raw(&profiles),
            &CLSID_AKAZA_TEXT_SERVICE,
            LANG_JA,
            &GUID_AKAZA_PROFILE,
            PCWSTR(display_name.as_ptr()),
            (display_name.len() - 1) as u32, // exclude null terminator
            PCWSTR(icon_path_w.as_ptr()),
            (icon_path_w.len() - 1) as u32,
            0,
        );
        // E_FAIL = already registered, treat as OK
        if hr.is_err() && hr != E_FAIL {
            hr.ok()?;
        }
    }

    Ok(())
}

fn unregister_profile() -> Result<()> {
    unsafe {
        let profiles: ITfInputProcessorProfiles =
            CoCreateInstance(&CLSID_TF_InputProcessorProfiles, None, CLSCTX_INPROC_SERVER)?;

        // 先に言語プロファイルを削除してから Unregister する
        let _ = profiles.RemoveLanguageProfile(
            &CLSID_AKAZA_TEXT_SERVICE,
            LANG_JA,
            &GUID_AKAZA_PROFILE,
        );
        let _ = profiles.Unregister(&CLSID_AKAZA_TEXT_SERVICE);
    }

    Ok(())
}

fn register_categories() -> Result<()> {
    const E_FAIL: HRESULT = HRESULT(0x80004005u32 as i32);

    unsafe {
        let cat_mgr: ITfCategoryMgr =
            CoCreateInstance(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER)?;

        let hr = (Interface::vtable(&cat_mgr).RegisterCategory)(
            Interface::as_raw(&cat_mgr),
            &CLSID_AKAZA_TEXT_SERVICE,
            &GUID_TFCAT_TIP_KEYBOARD,
            &CLSID_AKAZA_TEXT_SERVICE,
        );
        crate::dll_log(&format!("  categories: RegisterCategory hr={:#010X}", hr.0));
        // E_FAIL = already registered
        if hr.is_err() && hr != E_FAIL {
            hr.ok()?;
        }
    }

    Ok(())
}

fn unregister_categories() -> Result<()> {
    unsafe {
        let cat_mgr: ITfCategoryMgr =
            CoCreateInstance(&CLSID_TF_CategoryMgr, None, CLSCTX_INPROC_SERVER)?;

        let _ = cat_mgr.UnregisterCategory(
            &CLSID_AKAZA_TEXT_SERVICE,
            &GUID_TFCAT_TIP_KEYBOARD,
            &CLSID_AKAZA_TEXT_SERVICE,
        );
    }

    Ok(())
}
