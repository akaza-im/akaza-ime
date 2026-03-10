use std::mem::ManuallyDrop;

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::System::Com::*;

use crate::globals;
use crate::text_service::AkazaTextService;

#[implement(IClassFactory)]
pub struct AkazaClassFactory;

impl IClassFactory_Impl for AkazaClassFactory_Impl {
    fn CreateInstance(
        &self,
        punkouter: Ref<'_, IUnknown>,
        riid: *const GUID,
        ppvobject: *mut *mut std::ffi::c_void,
    ) -> Result<()> {
        unsafe {
            *ppvobject = std::ptr::null_mut();
        }

        if punkouter.is_some() {
            return Err(Error::from(CLASS_E_NOAGGREGATION));
        }

        let service = AkazaTextService::new();
        let unknown: IUnknown = service.into();
        unsafe {
            let manual = ManuallyDrop::new(unknown);
            manual.query(riid, ppvobject).ok()
        }
    }

    fn LockServer(&self, flock: BOOL) -> Result<()> {
        if flock.as_bool() {
            globals::server_lock();
        } else {
            globals::server_unlock();
        }
        Ok(())
    }
}
