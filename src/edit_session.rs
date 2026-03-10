use windows::core::*;
use windows::Win32::UI::TextServices::*;

/// EditSession のコールバック型
type EditCallback = Box<dyn FnOnce(&ITfContext, u32) -> Result<()>>;

/// TSF の編集セッション。テキスト変更は必ずこの仕組みを通す。
#[implement(ITfEditSession)]
pub struct EditSession {
    context: ITfContext,
    callback: std::cell::RefCell<Option<EditCallback>>,
}

impl EditSession {
    pub fn execute(
        context: &ITfContext,
        client_id: u32,
        flags: TF_CONTEXT_EDIT_CONTEXT_FLAGS,
        callback: impl FnOnce(&ITfContext, u32) -> Result<()> + 'static,
    ) -> Result<HRESULT> {
        let session = EditSession {
            context: context.clone(),
            callback: std::cell::RefCell::new(Some(Box::new(callback))),
        };
        let session: ITfEditSession = session.into();
        unsafe {
            let hr = context.RequestEditSession(client_id, &session, flags)?;
            crate::dll_log(&format!("RequestEditSession: hr={:#010X}", hr.0));
            Ok(hr)
        }
    }
}

impl ITfEditSession_Impl for EditSession_Impl {
    fn DoEditSession(&self, ec: u32) -> Result<()> {
        if let Some(callback) = self.callback.borrow_mut().take() {
            if let Err(e) = callback(&self.context, ec) {
                crate::dll_log(&format!("DoEditSession: callback error: {e}"));
                return Err(e);
            }
        }
        Ok(())
    }
}
