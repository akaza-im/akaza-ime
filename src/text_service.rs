use std::cell::RefCell;
use std::rc::Rc;
use std::sync::OnceLock;

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::UI::TextServices::*;

use libakaza::config::Config;
use libakaza::engine::base::HenkanEngine;
use libakaza::engine::bigram_word_viterbi_engine::BigramWordViterbiEngineBuilder;
use libakaza::graph::candidate::Candidate;
use libakaza::romkan::RomKanConverter;

use crate::edit_session::EditSession;
use crate::input_state::{InputMode, InputState};

// ---------------------------------------------------------------------------
// ログ
// ---------------------------------------------------------------------------

fn log(msg: &str) {
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

// ---------------------------------------------------------------------------
// CompositionSink — アプリ側からのコンポジション終了通知を受け取る
// ---------------------------------------------------------------------------

#[implement(ITfCompositionSink)]
struct CompositionSink {
    composition: Rc<RefCell<Option<ITfComposition>>>,
}

impl ITfCompositionSink_Impl for CompositionSink_Impl {
    fn OnCompositionTerminated(
        &self,
        _ecwrite: u32,
        _pcomposition: Ref<'_, ITfComposition>,
    ) -> Result<()> {
        // try_borrow_mut で borrow 衝突時のパニックを防ぐ
        if let Ok(mut comp) = self.composition.try_borrow_mut() {
            *comp = None;
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// グローバルエンジン (thread-local)
// ---------------------------------------------------------------------------

thread_local! {
    static THREAD_ENGINE: RefCell<Option<Box<dyn HenkanEngine>>> = const { RefCell::new(None) };
    static ENGINE_LOADED: RefCell<bool> = const { RefCell::new(false) };
}

fn romkan() -> Option<&'static RomKanConverter> {
    static ROMKAN: OnceLock<Option<RomKanConverter>> = OnceLock::new();
    ROMKAN
        .get_or_init(|| RomKanConverter::default_mapping().ok())
        .as_ref()
}

fn load_engine() -> Option<Box<dyn HenkanEngine>> {
    log("load_engine: start");
    let config = Config::load().unwrap_or_default();
    log(&format!("load_engine: model={:?}, dicts={}", config.engine.model, config.engine.dicts.len()));

    let builder = BigramWordViterbiEngineBuilder::new(config.engine);
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| builder.build())) {
        Ok(Ok(engine)) => {
            log("load_engine: OK");
            Some(Box::new(engine))
        }
        Ok(Err(e)) => {
            log(&format!("load_engine: error: {e}"));
            None
        }
        Err(p) => {
            let msg = p
                .downcast_ref::<&str>()
                .map(|s| s.to_string())
                .or_else(|| p.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown".to_string());
            log(&format!("load_engine: PANIC: {msg}"));
            None
        }
    }
}

fn ensure_engine() {
    ENGINE_LOADED.with(|loaded| {
        if *loaded.borrow() {
            return;
        }
        *loaded.borrow_mut() = true;
        if let Some(engine) = load_engine() {
            THREAD_ENGINE.with(|e| *e.borrow_mut() = Some(engine));
        }
    });
}

// ---------------------------------------------------------------------------
// TF_SELECTION ヘルパー
// ---------------------------------------------------------------------------

fn make_selection(range: ITfRange) -> TF_SELECTION {
    TF_SELECTION {
        range: std::mem::ManuallyDrop::new(Some(range)),
        style: TF_SELECTIONSTYLE {
            ase: TF_AE_NONE,
            fInterimChar: false.into(),
        },
    }
}

// ---------------------------------------------------------------------------
// AkazaTextService
// ---------------------------------------------------------------------------

#[implement(ITfTextInputProcessorEx, ITfKeyEventSink)]
pub struct AkazaTextService {
    thread_mgr: RefCell<Option<ITfThreadMgr>>,
    client_id: RefCell<u32>,
    composition: Rc<RefCell<Option<ITfComposition>>>,
    state: RefCell<InputState>,
}

impl AkazaTextService {
    pub fn new() -> Self {
        Self {
            thread_mgr: RefCell::new(None),
            client_id: RefCell::new(0),
            composition: Rc::new(RefCell::new(None)),
            state: RefCell::new(InputState::new()),
        }
    }

    fn advise_key_event_sink(this: &AkazaTextService_Impl) -> Result<()> {
        let thread_mgr = this.thread_mgr.borrow();
        let thread_mgr = thread_mgr.as_ref().unwrap();
        let keystroke_mgr: ITfKeystrokeMgr = thread_mgr.cast()?;
        let sink: ITfKeyEventSink = this.to_interface();
        unsafe { keystroke_mgr.AdviseKeyEventSink(*this.client_id.borrow(), &sink, true) }
    }

    fn unadvise_key_event_sink(this: &AkazaTextService_Impl) -> Result<()> {
        let thread_mgr = this.thread_mgr.borrow();
        let thread_mgr = thread_mgr.as_ref().unwrap();
        let keystroke_mgr: ITfKeystrokeMgr = thread_mgr.cast()?;
        unsafe { keystroke_mgr.UnadviseKeyEventSink(*this.client_id.borrow()) }
    }

    fn is_ctrl_down() -> bool {
        unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState(0x11) < 0 }
    }

    fn client_id(&self) -> u32 {
        *self.client_id.borrow()
    }

    // -----------------------------------------------------------------------
    // キー判定
    // -----------------------------------------------------------------------

    fn should_handle_key(&self, wparam: WPARAM) -> bool {
        let vk = wparam.0 as u32;
        let state = self.state.borrow();

        match vk {
            0xF3 | 0xF4 => true,                          // 漢字 on/off
            0x4A | 0x4D if Self::is_ctrl_down()            // Ctrl+J/M
                => !state.is_empty(),
            0x41..=0x5A                                    // A-Z
                => state.mode == InputMode::Hiragana || state.mode == InputMode::Converting,
            0xBE | 0xBC | 0xBF | 0xBA | 0xBB | 0xBD      // 句読点・記号
                => state.mode == InputMode::Hiragana || !state.is_empty(),
            0x20 => !state.is_empty(),                     // Space
            0x0D => !state.is_empty(),                     // Enter
            0x1B => !state.is_empty(),                     // Escape
            0x08 => !state.is_empty(),                     // Backspace
            0x26 | 0x28 => state.mode == InputMode::Converting, // ↑↓
            _ => false,
        }
    }

    // -----------------------------------------------------------------------
    // キー入力ディスパッチ
    // -----------------------------------------------------------------------

    fn handle_key(&self, context: &ITfContext, wparam: WPARAM) -> Result<()> {
        let vk = wparam.0 as u32;

        match vk {
            // 漢字キー → トグル
            0xF3 | 0xF4 => {
                if self.state.borrow().mode == InputMode::Direct {
                    self.state.borrow_mut().mode = InputMode::Hiragana;
                } else {
                    if !self.state.borrow().is_empty() {
                        self.handle_commit(context)?;
                    }
                    self.state.borrow_mut().mode = InputMode::Direct;
                }
            }
            0x4A | 0x4D if Self::is_ctrl_down() => self.handle_commit(context)?,
            0x41..=0x5A => self.handle_char(context, (vk as u8 + 0x20) as char)?,
            0x20 => self.handle_convert(context)?,
            0x0D => self.handle_commit(context)?,
            0x1B => self.handle_cancel(context)?,
            0x08 => self.handle_backspace(context)?,
            0x26 => self.handle_candidate_prev(context)?,
            0x28 => self.handle_candidate_next(context)?,
            0xBE => self.handle_punctuation(context, '。')?,
            0xBC => self.handle_punctuation(context, '、')?,
            0xBF => self.handle_punctuation(context, '・')?,
            0xBA => self.handle_punctuation(context, '：')?,
            0xBB => self.handle_punctuation(context, '；')?,
            0xBD => self.handle_punctuation(context, 'ー')?,
            _ => {}
        }
        Ok(())
    }

    // -----------------------------------------------------------------------
    // 文字入力
    // -----------------------------------------------------------------------

    fn handle_char(&self, context: &ITfContext, ch: char) -> Result<()> {
        {
            let mut state = self.state.borrow_mut();
            if state.mode == InputMode::Converting {
                drop(state);
                return self.handle_commit_and_continue(context, ch);
            }

            state.romaji_buffer.push(ch);

            if let Some(romkan) = romkan() {
                let converted = romkan.to_hiragana(&state.romaji_buffer);
                if converted != state.romaji_buffer {
                    let (kana, pending) = split_kana_pending(&converted, &state.romaji_buffer);
                    if !kana.is_empty() {
                        state.preedit.push_str(&kana);
                    }
                    state.romaji_buffer = pending;
                }
            }
        }
        self.update_composition(context)
    }

    fn handle_punctuation(&self, context: &ITfContext, ch: char) -> Result<()> {
        {
            let mut state = self.state.borrow_mut();
            if state.mode == InputMode::Converting {
                drop(state);
                self.handle_commit(context)?;
                self.state.borrow_mut().preedit.push(ch);
                return self.update_composition(context);
            }
            // 残りのローマ字をかなに変換
            if !state.romaji_buffer.is_empty() {
                if let Some(romkan) = romkan() {
                    let converted = romkan.to_hiragana(&state.romaji_buffer);
                    state.preedit.push_str(&converted);
                    state.romaji_buffer.clear();
                }
            }
            state.preedit.push(ch);
        }
        self.update_composition(context)
    }

    // -----------------------------------------------------------------------
    // 変換
    // -----------------------------------------------------------------------

    fn handle_convert(&self, context: &ITfContext) -> Result<()> {
        let mut state = self.state.borrow_mut();

        if state.mode == InputMode::Converting {
            if !state.candidates.is_empty() {
                state.candidate_index = (state.candidate_index + 1) % state.candidates.len();
            }
            drop(state);
            return self.update_composition(context);
        }

        // 残りのローマ字をかなに変換
        if !state.romaji_buffer.is_empty() {
            if let Some(romkan) = romkan() {
                let converted = romkan.to_hiragana(&state.romaji_buffer);
                state.preedit.push_str(&converted);
                state.romaji_buffer.clear();
            }
        }

        if state.preedit.is_empty() {
            return Ok(());
        }

        let hiragana = state.preedit.clone();
        drop(state);

        let (candidates, segments) = self.convert_candidates(&hiragana);

        let mut state = self.state.borrow_mut();
        if candidates.is_empty() {
            state.candidates = vec![hiragana];
        } else {
            state.candidates = candidates;
        }
        state.segments = segments;
        state.candidate_index = 0;
        state.mode = InputMode::Converting;
        drop(state);

        self.update_composition(context)
    }

    fn convert_candidates(&self, hiragana: &str) -> (Vec<String>, Vec<Vec<Candidate>>) {
        let mut candidates = Vec::new();
        let mut all_segments = Vec::new();

        THREAD_ENGINE.with(|e| {
            let mut engine = e.borrow_mut();
            let Some(engine) = engine.as_mut() else { return };
            let Ok(segments) = engine.convert(hiragana, None) else { return };

            if segments.len() == 1 {
                // 単一分節 → その分節の全候補
                for cand in &segments[0] {
                    if !candidates.contains(&cand.surface) {
                        candidates.push(cand.surface.clone());
                        all_segments.push(vec![cand.clone()]);
                    }
                }
            } else {
                // 複数分節 → 先頭候補を連結
                let main: String = segments
                    .iter()
                    .filter_map(|seg| seg.first().map(|c| c.surface.as_str()))
                    .collect();
                let main_segs: Vec<_> = segments
                    .iter()
                    .filter_map(|seg| seg.first().cloned())
                    .collect();
                candidates.push(main);
                all_segments.push(main_segs);

                // k-best で異なる分節パターンも追加
                if let Ok(paths) = engine.convert_k_best(hiragana, None, 5) {
                    for path in &paths {
                        let text: String = path
                            .segments
                            .iter()
                            .filter_map(|seg| seg.first().map(|c| c.surface.as_str()))
                            .collect();
                        if !candidates.contains(&text) {
                            let segs: Vec<_> = path
                                .segments
                                .iter()
                                .filter_map(|seg| seg.first().cloned())
                                .collect();
                            candidates.push(text);
                            all_segments.push(segs);
                        }
                    }
                }
            }
        });

        (candidates, all_segments)
    }

    // -----------------------------------------------------------------------
    // 確定
    // -----------------------------------------------------------------------

    fn learn_committed(&self) {
        let state = self.state.borrow();
        if state.mode != InputMode::Converting || state.segments.is_empty() {
            return;
        }
        if let Some(segs) = state.segments.get(state.candidate_index) {
            let segs = segs.clone();
            drop(state);
            THREAD_ENGINE.with(|e| {
                if let Some(engine) = e.borrow_mut().as_mut() {
                    engine.learn(&segs);
                }
            });
        }
    }

    fn handle_commit(&self, context: &ITfContext) -> Result<()> {
        self.learn_committed();
        let text = self.state.borrow().commit_text();
        let comp = self.composition.borrow_mut().take();
        let Some(comp) = comp else {
            self.state.borrow_mut().reset();
            return Ok(());
        };

        let text_w: Vec<u16> = text.encode_utf16().collect();
        let client_id = self.client_id();

        let _ = EditSession::execute(
            &context.clone(),
            client_id,
            TF_ES_READWRITE,
            move |ctx, ec| unsafe {
                let range = comp.GetRange()?;
                range.SetText(ec, 0, &text_w)?;
                range.Collapse(ec, TfAnchor(1))?;
                ctx.SetSelection(ec, &[make_selection(range)])?;
                let _ = comp.EndComposition(ec);
                Ok(())
            },
        )?;

        self.state.borrow_mut().reset();
        Ok(())
    }

    /// 確定して次の文字入力を開始 (gvim 対応: 単一 EditSession で実行)
    fn handle_commit_and_continue(&self, context: &ITfContext, next_ch: char) -> Result<()> {
        self.learn_committed();
        let text = self.state.borrow().commit_text();
        let comp = self.composition.borrow_mut().take();
        let Some(comp) = comp else {
            self.state.borrow_mut().reset();
            self.state.borrow_mut().romaji_buffer.push(next_ch);
            return self.update_composition(context);
        };

        let text_w: Vec<u16> = text.encode_utf16().collect();
        let client_id = self.client_id();
        let composition_rc = self.composition.clone();
        let next_ch_w: Vec<u16> = next_ch.to_string().encode_utf16().collect();

        let _ = EditSession::execute(
            &context.clone(),
            client_id,
            TF_ES_READWRITE,
            move |ctx, ec| unsafe {
                // 1. 確定
                let range = comp.GetRange()?;
                range.SetText(ec, 0, &text_w)?;
                range.Collapse(ec, TfAnchor(1))?;
                ctx.SetSelection(ec, &[make_selection(range)])?;
                let _ = comp.EndComposition(ec);

                // 2. 新しいコンポジションを開始
                let ctx_composition: ITfContextComposition = ctx.cast()?;
                let insertion: ITfInsertAtSelection = ctx.cast()?;
                let mut range_ptr = std::ptr::null_mut::<std::ffi::c_void>();

                let hr = (Interface::vtable(&insertion).InsertTextAtSelection)(
                    Interface::as_raw(&insertion),
                    ec,
                    TF_IAS_QUERYONLY,
                    PCWSTR(std::ptr::null()),
                    0,
                    &mut range_ptr as *mut _ as *mut _,
                );
                hr.ok()?;

                let new_range: ITfRange = std::mem::transmute(range_ptr);
                let sink: ITfCompositionSink =
                    (CompositionSink { composition: composition_rc.clone() }).into();

                let hr = (Interface::vtable(&ctx_composition).StartComposition)(
                    Interface::as_raw(&ctx_composition),
                    ec,
                    Interface::as_raw(&new_range),
                    Interface::as_raw(&sink),
                    &mut range_ptr as *mut _ as *mut _,
                );
                if hr.is_ok() && !range_ptr.is_null() {
                    let new_comp: ITfComposition = std::mem::transmute(range_ptr);
                    let comp_range = new_comp.GetRange()?;
                    comp_range.SetText(ec, 0, &next_ch_w)?;
                    comp_range.Collapse(ec, TfAnchor(1))?;
                    ctx.SetSelection(ec, &[make_selection(comp_range)])?;
                    *composition_rc.borrow_mut() = Some(new_comp);
                }
                Ok(())
            },
        )?;

        self.state.borrow_mut().reset();
        self.state.borrow_mut().romaji_buffer.push(next_ch);
        Ok(())
    }

    // -----------------------------------------------------------------------
    // キャンセル・Backspace・候補移動
    // -----------------------------------------------------------------------

    fn handle_cancel(&self, context: &ITfContext) -> Result<()> {
        self.set_composition_text(context, "")?;
        self.end_composition(context)?;
        self.state.borrow_mut().reset();
        Ok(())
    }

    fn handle_backspace(&self, context: &ITfContext) -> Result<()> {
        {
            let mut state = self.state.borrow_mut();
            if state.mode == InputMode::Converting {
                state.candidates.clear();
                state.candidate_index = 0;
                state.mode = InputMode::Hiragana;
            } else if !state.romaji_buffer.is_empty() {
                state.romaji_buffer.pop();
            } else if !state.preedit.is_empty() {
                state.preedit.pop();
            }

            if state.is_empty() {
                drop(state);
                self.set_composition_text(context, "")?;
                self.end_composition(context)?;
                self.state.borrow_mut().reset();
                return Ok(());
            }
        }
        self.update_composition(context)
    }

    fn handle_candidate_prev(&self, context: &ITfContext) -> Result<()> {
        {
            let mut state = self.state.borrow_mut();
            if !state.candidates.is_empty() && state.candidate_index > 0 {
                state.candidate_index -= 1;
            }
        }
        self.update_composition(context)
    }

    fn handle_candidate_next(&self, context: &ITfContext) -> Result<()> {
        {
            let mut state = self.state.borrow_mut();
            if !state.candidates.is_empty() {
                state.candidate_index =
                    (state.candidate_index + 1).min(state.candidates.len() - 1);
            }
        }
        self.update_composition(context)
    }

    // -----------------------------------------------------------------------
    // コンポジション操作
    // -----------------------------------------------------------------------

    fn update_composition(&self, context: &ITfContext) -> Result<()> {
        let text = self.state.borrow().display_text();
        self.start_composition_if_needed(context)?;
        self.set_composition_text(context, &text)
    }

    fn start_composition_if_needed(&self, context: &ITfContext) -> Result<()> {
        if self.composition.borrow().is_some() {
            return Ok(());
        }

        let client_id = self.client_id();
        let composition = self.composition.clone();

        let _ = EditSession::execute(
            &context.clone(),
            client_id,
            TF_ES_READWRITE,
            move |ctx, ec| unsafe {
                let ctx_composition: ITfContextComposition = ctx.cast()?;
                let insertion: ITfInsertAtSelection = ctx.cast()?;
                let mut range = std::ptr::null_mut::<std::ffi::c_void>();

                let hr = (Interface::vtable(&insertion).InsertTextAtSelection)(
                    Interface::as_raw(&insertion),
                    ec,
                    TF_IAS_QUERYONLY,
                    PCWSTR(std::ptr::null()),
                    0,
                    &mut range as *mut _ as *mut _,
                );
                hr.ok()?;
                let range: ITfRange = std::mem::transmute(range);

                let sink: ITfCompositionSink =
                    (CompositionSink { composition: composition.clone() }).into();
                let mut comp_ptr = std::ptr::null_mut::<std::ffi::c_void>();

                let hr = (Interface::vtable(&ctx_composition).StartComposition)(
                    Interface::as_raw(&ctx_composition),
                    ec,
                    Interface::as_raw(&range),
                    Interface::as_raw(&sink),
                    &mut comp_ptr as *mut _ as *mut _,
                );
                if hr.is_ok() && !comp_ptr.is_null() {
                    let comp: ITfComposition = std::mem::transmute(comp_ptr);
                    *composition.borrow_mut() = Some(comp);
                }
                Ok(())
            },
        )?;

        Ok(())
    }

    fn set_composition_text(&self, context: &ITfContext, text: &str) -> Result<()> {
        let comp = self.composition.borrow().clone();
        let Some(comp) = comp else { return Ok(()) };

        let text_w: Vec<u16> = text.encode_utf16().collect();
        let client_id = self.client_id();

        let _ = EditSession::execute(
            &context.clone(),
            client_id,
            TF_ES_READWRITE,
            move |ctx, ec| unsafe {
                let range = comp.GetRange()?;
                range.SetText(ec, 0, &text_w)?;
                range.Collapse(ec, TfAnchor(1))?;
                ctx.SetSelection(ec, &[make_selection(range)])?;
                Ok(())
            },
        )?;

        Ok(())
    }

    fn end_composition(&self, context: &ITfContext) -> Result<()> {
        let comp = self.composition.borrow_mut().take();
        let Some(comp) = comp else { return Ok(()) };

        let client_id = self.client_id();
        let _ = EditSession::execute(
            &context.clone(),
            client_id,
            TF_ES_READWRITE,
            move |_ctx, ec| unsafe {
                let _ = comp.EndComposition(ec);
                Ok(())
            },
        )?;

        Ok(())
    }
}

// ---------------------------------------------------------------------------
// ローマ字→かな 分離ヘルパー
// ---------------------------------------------------------------------------

/// 変換結果を「確定かな」と「未確定ローマ字」に分離する。
/// "n" → "ん" の早すぎる変換も防ぐ。
fn split_kana_pending(converted: &str, raw_buffer: &str) -> (String, String) {
    let trailing_start = converted
        .char_indices()
        .rev()
        .take_while(|(_, c)| c.is_ascii_alphabetic())
        .last()
        .map(|(i, _)| i)
        .unwrap_or(converted.len());

    let mut kana = converted[..trailing_start].to_string();
    let mut pending = converted[trailing_start..].to_string();

    // "n" → "ん" の早すぎる変換を防ぐ (na, ni, nyu 等の可能性)
    if kana.ends_with('ん') && !raw_buffer.ends_with("nn") {
        if raw_buffer.ends_with('n') || !pending.is_empty() {
            kana.truncate(kana.len() - "ん".len());
            pending = format!("n{pending}");
        }
    }

    (kana, pending)
}

// ---------------------------------------------------------------------------
// ITfTextInputProcessorEx / ITfTextInputProcessor
// ---------------------------------------------------------------------------

impl ITfTextInputProcessorEx_Impl for AkazaTextService_Impl {
    fn ActivateEx(&self, ptim: Ref<'_, ITfThreadMgr>, tid: u32, _flags: u32) -> Result<()> {
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            log(&format!("ActivateEx: tid={tid}"));
            *self.thread_mgr.borrow_mut() = ptim.cloned();
            *self.client_id.borrow_mut() = tid;

            ensure_engine();
            AkazaTextService::advise_key_event_sink(self)?;
            log("ActivateEx: done");
            Ok(())
        }));
        match result {
            Ok(r) => r,
            Err(p) => {
                let msg = p
                    .downcast_ref::<&str>()
                    .map(|s| s.to_string())
                    .or_else(|| p.downcast_ref::<String>().cloned())
                    .unwrap_or_else(|| "unknown".to_string());
                log(&format!("ActivateEx: PANIC: {msg}"));
                Ok(())
            }
        }
    }
}

impl ITfTextInputProcessor_Impl for AkazaTextService_Impl {
    fn Activate(&self, ptim: Ref<'_, ITfThreadMgr>, tid: u32) -> Result<()> {
        self.ActivateEx(ptim, tid, 0)
    }

    fn Deactivate(&self) -> Result<()> {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _ = AkazaTextService::unadvise_key_event_sink(self);
            let _ = self.composition.try_borrow_mut().map(|mut c| c.take());
            self.state.borrow_mut().reset();
            *self.thread_mgr.borrow_mut() = None;
            *self.client_id.borrow_mut() = 0;
        }));
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// ITfKeyEventSink
// ---------------------------------------------------------------------------

impl ITfKeyEventSink_Impl for AkazaTextService_Impl {
    fn OnSetFocus(&self, _fforeground: BOOL) -> Result<()> {
        Ok(())
    }

    fn OnTestKeyDown(
        &self,
        _pic: Ref<'_, ITfContext>,
        wparam: WPARAM,
        _lparam: LPARAM,
    ) -> Result<BOOL> {
        Ok(BOOL::from(self.should_handle_key(wparam)))
    }

    fn OnTestKeyUp(
        &self,
        _pic: Ref<'_, ITfContext>,
        _wparam: WPARAM,
        _lparam: LPARAM,
    ) -> Result<BOOL> {
        Ok(FALSE)
    }

    fn OnKeyDown(
        &self,
        pic: Ref<'_, ITfContext>,
        wparam: WPARAM,
        _lparam: LPARAM,
    ) -> Result<BOOL> {
        if !self.should_handle_key(wparam) {
            return Ok(FALSE);
        }

        if let Some(ctx) = pic.as_ref() {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                self.handle_key(ctx, wparam)
            }));
            match result {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    log(&format!("OnKeyDown: error: {e}"));
                    return Ok(FALSE);
                }
                Err(p) => {
                    let msg = p
                        .downcast_ref::<&str>()
                        .map(|s| s.to_string())
                        .or_else(|| p.downcast_ref::<String>().cloned())
                        .unwrap_or_else(|| "unknown".to_string());
                    log(&format!("OnKeyDown: PANIC: {msg}"));
                    return Ok(FALSE);
                }
            }
        }

        Ok(TRUE)
    }

    fn OnKeyUp(
        &self,
        _pic: Ref<'_, ITfContext>,
        _wparam: WPARAM,
        _lparam: LPARAM,
    ) -> Result<BOOL> {
        Ok(FALSE)
    }

    fn OnPreservedKey(
        &self,
        _pic: Ref<'_, ITfContext>,
        _rguid: *const GUID,
    ) -> Result<BOOL> {
        Ok(FALSE)
    }
}
