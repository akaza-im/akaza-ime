use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use windows::core::*;
use windows::Win32::Foundation::*;
use windows::Win32::UI::TextServices::*;

use libakaza::config::Config;
use libakaza::engine::base::HenkanEngine;
use libakaza::engine::bigram_word_viterbi_engine::BigramWordViterbiEngineBuilder;
use libakaza::graph::candidate::Candidate;
use libakaza::keymap::{KeyPattern, KeyState, Keymap};
use libakaza::romkan::RomKanConverter;

use crate::candidate_window::CandidateWindow;
use crate::edit_session::EditSession;
use crate::input_state::{InputMode, InputState};

// ---------------------------------------------------------------------------
// ログ
// ---------------------------------------------------------------------------

#[cfg(debug_assertions)]
fn log(msg: &str) {
    use std::io::Write;
    let path = dirs::data_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."))
        .join("akaza")
        .join("ime_log.txt");
    if let Ok(mut f) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
    {
        let _ = writeln!(f, "{msg}");
        let _ = f.flush();
    }
}

#[cfg(not(debug_assertions))]
fn log(_msg: &str) {}

// ---------------------------------------------------------------------------
// CompositionSink
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
        if let Ok(mut comp) = self.composition.try_borrow_mut() {
            *comp = None;
        }
        crate::candidate_window::hide_candidate_window();
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// グローバルリソース (thread-local)
// ---------------------------------------------------------------------------

thread_local! {
    static THREAD_ENGINE: RefCell<Option<Box<dyn HenkanEngine>>> = const { RefCell::new(None) };
    static ENGINE_LOADED: RefCell<bool> = const { RefCell::new(false) };
    static THREAD_KEYMAP: RefCell<Option<HashMap<KeyPattern, String>>> = const { RefCell::new(None) };
    static THREAD_ROMKAN: RefCell<Option<RomKanConverter>> = const { RefCell::new(None) };
}

fn romkan_convert(input: &str) -> Option<String> {
    THREAD_ROMKAN.with(|r| {
        let r = r.borrow();
        r.as_ref().map(|rk| rk.to_hiragana(input))
    })
}

fn load_engine() -> Option<Box<dyn HenkanEngine>> {
    log("load_engine: start");
    let config = Config::load().unwrap_or_default();
    log(&format!(
        "load_engine: model={:?}, dicts={}",
        config.engine.model,
        config.engine.dicts.len()
    ));

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
            let msg = panic_message(&p);
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
        // キーマップをロード
        let config = Config::load().unwrap_or_default();
        match Keymap::load(&config.keymap) {
            Ok(km) => {
                log(&format!("keymap: loaded {} entries", km.len()));
                THREAD_KEYMAP.with(|k| *k.borrow_mut() = Some(km));
            }
            Err(e) => log(&format!("keymap: load error: {e}")),
        }
        // romkan をロード
        match RomKanConverter::default_mapping() {
            Ok(rk) => {
                log("romkan: loaded");
                THREAD_ROMKAN.with(|r| *r.borrow_mut() = Some(rk));
            }
            Err(e) => log(&format!("romkan: load error: {e}")),
        }
    });
}

fn invalidate_engine() {
    ENGINE_LOADED.with(|loaded| *loaded.borrow_mut() = false);
}

fn panic_message(p: &Box<dyn std::any::Any + Send>) -> String {
    p.downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown".to_string())
}

// ---------------------------------------------------------------------------
// VK → キー名変換
// ---------------------------------------------------------------------------

/// Windows VK コードを keymap.yml のキー名に変換
fn vk_to_key_name(vk: u32) -> Option<&'static str> {
    match vk {
        0x08 => Some("BackSpace"),
        0x09 => Some("Tab"),
        0x0D => Some("Return"),
        0x1B => Some("Escape"),
        0x20 => Some("space"),
        0x21 => Some("Page_Up"),
        0x22 => Some("Page_Down"),
        0x25 => Some("Left"),
        0x26 => Some("Up"),
        0x27 => Some("Right"),
        0x28 => Some("Down"),
        0x30 => Some("0"),
        0x31 => Some("1"),
        0x32 => Some("2"),
        0x33 => Some("3"),
        0x34 => Some("4"),
        0x35 => Some("5"),
        0x36 => Some("6"),
        0x37 => Some("7"),
        0x38 => Some("8"),
        0x39 => Some("9"),
        0x41..=0x5A => None, // A-Z はアルファベット入力として別処理
        0x70 => Some("F1"),
        0x71 => Some("F2"),
        0x72 => Some("F3"),
        0x73 => Some("F4"),
        0x74 => Some("F5"),
        0x75 => Some("F6"),
        0x76 => Some("F7"),
        0x77 => Some("F8"),
        0x78 => Some("F9"),
        0x79 => Some("F10"),
        0x7A => Some("F11"),
        0x7B => Some("F12"),
        // 0xF3/0xF4 は resolve_key で直接処理
        // 句読点・記号 — romkan で処理するので None
        0xBA..=0xBF | 0xDB | 0xDD => None,
        _ => None,
    }
}

/// VK コードを句読点の ASCII 文字に変換 (Shift 考慮)
fn vk_to_punct(vk: u32, shift: bool) -> Option<(char, char)> {
    match (vk, shift) {
        (0xBE, false) => Some(('.', '。')),
        (0xBE, true) => Some(('>', '＞')),
        (0xBC, false) => Some((',', '、')),
        (0xBC, true) => Some(('<', '＜')),
        (0xBF, false) => Some(('/', '・')),
        (0xBF, true) => Some(('?', '？')),
        (0xBA, false) => Some((':', '：')),
        (0xBA, true) => Some(('*', '＊')),
        (0xBB, false) => Some((';', '；')),
        (0xBB, true) => Some(('+', '＋')),
        (0xBD, false) => Some(('-', 'ー')),
        (0xBD, true) => Some(('=', '＝')),
        (0xDB, false) => Some(('[', '「')),
        (0xDB, true) => Some(('{', '｛')),
        (0xDD, false) => Some((']', '」')),
        (0xDD, true) => Some(('}', '｝')),
        (0x31, true) => Some(('!', '！')),
        _ => None,
    }
}

fn is_shift_down() -> bool {
    unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState(0x10) < 0 }
}

fn is_ctrl_down() -> bool {
    unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetKeyState(0x11) < 0 }
}

// ---------------------------------------------------------------------------
// キーマップルックアップ
// ---------------------------------------------------------------------------

fn current_key_state(mode: &InputMode, has_input: bool) -> KeyState {
    match mode {
        InputMode::Direct => KeyState::PreComposition,
        InputMode::Hiragana => {
            if has_input {
                KeyState::Composition
            } else {
                KeyState::PreComposition
            }
        }
        InputMode::Converting => KeyState::Conversion,
    }
}

/// キーマップからコマンドを検索
fn lookup_keymap(key_state: KeyState, key_name: &str, ctrl: bool, shift: bool) -> Option<String> {
    THREAD_KEYMAP.with(|km| {
        let km = km.borrow();
        let km = km.as_ref()?;
        for (pattern, command) in km {
            if pattern.key == key_name
                && pattern.ctrl == ctrl
                && pattern.shift == shift
                && pattern.states.contains(&key_state)
            {
                return Some(command.clone());
            }
        }
        None
    })
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
    candidate_window: RefCell<CandidateWindow>,
}

impl AkazaTextService {
    pub fn new() -> Self {
        Self {
            thread_mgr: RefCell::new(None),
            client_id: RefCell::new(0),
            composition: Rc::new(RefCell::new(None)),
            state: RefCell::new(InputState::new()),
            candidate_window: RefCell::new(CandidateWindow::new()),
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
        let Some(thread_mgr) = thread_mgr.as_ref() else {
            return Ok(());
        };
        let keystroke_mgr: ITfKeystrokeMgr = thread_mgr.cast()?;
        unsafe { keystroke_mgr.UnadviseKeyEventSink(*this.client_id.borrow()) }
    }

    fn client_id(&self) -> u32 {
        *self.client_id.borrow()
    }

    // -----------------------------------------------------------------------
    // キー判定・ディスパッチ
    // -----------------------------------------------------------------------

    fn resolve_key(&self, wparam: WPARAM) -> Option<KeyAction> {
        let vk = wparam.0 as u32;
        let state = self.state.borrow();
        let key_state = current_key_state(&state.mode, !state.is_empty());
        let ctrl = is_ctrl_down();
        let shift = is_shift_down();

        // 0. 全角/半角キー (0xF3/0xF4) は常にトグル
        if vk == 0xF3 || vk == 0xF4 {
            return Some(KeyAction::Command("set_input_mode_hiragana".to_string()));
        }

        // Ctrl+英字 (Ctrl+V, Ctrl+C, Ctrl+A など) はアプリに渡す
        if ctrl && (0x41..=0x5A).contains(&vk) {
            return None;
        }

        // 1. VK → キー名があればキーマップで検索
        if let Some(key_name) = vk_to_key_name(vk) {
            if let Some(cmd) = lookup_keymap(key_state, key_name, ctrl, shift) {
                return Some(KeyAction::Command(cmd));
            }
        }

        // 1.5. 数字キー (0-9) — キーマップにコマンドがなければ全角数字として入力
        if (0x30..=0x39).contains(&vk) && !shift {
            if state.mode == InputMode::Hiragana || state.mode == InputMode::Converting {
                let half = (vk as u8) as char; // '0'-'9'
                let full = char::from_u32('０' as u32 + (vk - 0x30)).unwrap();
                return Some(KeyAction::Punctuation(half, full));
            }
            return None;
        }

        // 2. アルファベット (A-Z) — キーマップにコマンドがあればそちら優先
        if (0x41..=0x5A).contains(&vk) {
            let ch = (vk as u8 + 0x20) as char; // 小文字
            let key_name = ch.to_string();
            if let Some(cmd) = lookup_keymap(key_state, &key_name, ctrl, shift) {
                return Some(KeyAction::Command(cmd));
            }
            // コマンドなし → ひらがなモードまたは変換中なら文字入力
            if state.mode == InputMode::Hiragana || state.mode == InputMode::Converting {
                return Some(KeyAction::CharInput(ch));
            }
            return None;
        }

        // 3. 句読点・記号
        if let Some((ascii_ch, fallback_ch)) = vk_to_punct(vk, shift) {
            if state.mode == InputMode::Hiragana || !state.is_empty() {
                return Some(KeyAction::Punctuation(ascii_ch, fallback_ch));
            }
            return None;
        }

        None
    }

    fn should_handle_key(&self, wparam: WPARAM) -> bool {
        self.resolve_key(wparam).is_some()
    }

    fn handle_key(&self, context: &ITfContext, wparam: WPARAM) -> Result<()> {
        let Some(action) = self.resolve_key(wparam) else {
            return Ok(());
        };

        match action {
            KeyAction::Command(cmd) => self.execute_command(context, &cmd),
            KeyAction::CharInput(ch) => self.handle_char(context, ch),
            KeyAction::Punctuation(ascii, fallback) => {
                self.handle_punctuation(context, ascii, fallback)
            }
        }
    }

    // -----------------------------------------------------------------------
    // コマンド実行
    // -----------------------------------------------------------------------

    fn execute_command(&self, context: &ITfContext, cmd: &str) -> Result<()> {
        match cmd {
            "set_input_mode_hiragana" => {
                if self.state.borrow().mode == InputMode::Direct {
                    self.state.borrow_mut().mode = InputMode::Hiragana;
                } else {
                    // トグル: ひらがな/変換中 → Direct
                    if !self.state.borrow().is_empty() {
                        self.handle_commit(context)?;
                    }
                    self.state.borrow_mut().mode = InputMode::Direct;
                }
                Ok(())
            }
            "set_input_mode_alnum" => {
                if !self.state.borrow().is_empty() {
                    self.handle_commit(context)?;
                }
                self.state.borrow_mut().mode = InputMode::Direct;
                Ok(())
            }
            "update_candidates" => self.handle_convert(context),
            "commit_candidate" | "commit_preedit" => self.handle_commit(context),
            "escape" => self.handle_cancel(context),
            "erase_character_before_cursor" => self.handle_backspace(context),
            "cursor_up" => self.handle_candidate_prev(context),
            "cursor_down" => self.handle_candidate_next(context),
            "cursor_left" => self.handle_segment_prev(context),
            "cursor_right" => self.handle_segment_next(context),
            "convert_to_full_hiragana" => self.handle_convert_to_hiragana(context),
            "convert_to_full_katakana" => self.handle_convert_to_katakana(context),
            _ if cmd.starts_with("press_number_") => {
                // 変換中に数字 → 確定して全角数字を入力
                if let Some(n) = cmd.strip_prefix("press_number_").and_then(|s| s.parse::<u32>().ok()) {
                    let full = char::from_u32('０' as u32 + n).unwrap_or_else(|| (b'0' + n as u8) as char);
                    self.handle_commit(context)?;
                    self.state.borrow_mut().preedit.push(full);
                    self.update_composition(context)
                } else {
                    Ok(())
                }
            }
            _ => {
                log(&format!("unhandled command: {cmd}"));
                Ok(())
            }
        }
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

            if let Some(converted) = romkan_convert(&state.romaji_buffer) {
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

    fn handle_punctuation(
        &self,
        context: &ITfContext,
        ascii_ch: char,
        fallback_ch: char,
    ) -> Result<()> {
        {
            let mut state = self.state.borrow_mut();
            if state.mode == InputMode::Converting {
                drop(state);
                self.handle_commit(context)?;
                let mut state = self.state.borrow_mut();
                state.romaji_buffer.push(ascii_ch);
                if let Some(converted) = romkan_convert(&state.romaji_buffer) {
                    if converted != state.romaji_buffer {
                        let (kana, pending) =
                            split_kana_pending(&converted, &state.romaji_buffer);
                        if !kana.is_empty() {
                            state.preedit.push_str(&kana);
                        }
                        state.romaji_buffer = pending;
                    } else {
                        state.romaji_buffer.pop();
                        state.preedit.push(fallback_ch);
                    }
                } else {
                    state.romaji_buffer.pop();
                    state.preedit.push(fallback_ch);
                }
                drop(state);
                return self.update_composition(context);
            }

            state.romaji_buffer.push(ascii_ch);
            if let Some(converted) = romkan_convert(&state.romaji_buffer) {
                if converted != state.romaji_buffer {
                    let (kana, pending) = split_kana_pending(&converted, &state.romaji_buffer);
                    if !kana.is_empty() {
                        state.preedit.push_str(&kana);
                    }
                    state.romaji_buffer = pending;
                } else {
                    state.romaji_buffer.pop();
                    if !state.romaji_buffer.is_empty() {
                        if let Some(flushed) = romkan_convert(&state.romaji_buffer) {
                            state.preedit.push_str(&flushed);
                        }
                        state.romaji_buffer.clear();
                    }
                    state.preedit.push(fallback_ch);
                }
            } else {
                state.romaji_buffer.pop();
                state.preedit.push(fallback_ch);
            }
        }
        self.update_composition(context)
    }

    // -----------------------------------------------------------------------
    // 変換
    // -----------------------------------------------------------------------

    fn handle_convert(&self, context: &ITfContext) -> Result<()> {
        let mut state = self.state.borrow_mut();

        if state.mode == InputMode::Converting {
            // 変換中にスペース → フォーカスセグメントの次の候補
            let seg_idx = state.focus_segment;
            if let Some(seg) = state.segments.get(seg_idx) {
                if !seg.is_empty() {
                    let cur = state.segment_indices.get(seg_idx).copied().unwrap_or(0);
                    let next = (cur + 1) % seg.len();
                    if let Some(si) = state.segment_indices.get_mut(seg_idx) {
                        *si = next;
                    }
                }
            }
            drop(state);
            return self.update_composition(context);
        }

        if !state.romaji_buffer.is_empty() {
            if let Some(converted) = romkan_convert(&state.romaji_buffer) {
                state.preedit.push_str(&converted);
                state.romaji_buffer.clear();
            }
        }

        if state.preedit.is_empty() {
            return Ok(());
        }

        let hiragana = state.preedit.clone();
        drop(state);

        let segments = self.convert_segments(&hiragana);

        let mut state = self.state.borrow_mut();
        if segments.is_empty() {
            // エンジンが変換できなかった場合、ひらがなをそのまま単一セグメントに
            state.segments = vec![vec![Candidate {
                surface: hiragana.clone(),
                yomi: hiragana,
                cost: 0.0,
                compound_word: false,
            }]];
        } else {
            state.segments = segments;
        }
        state.segment_indices = vec![0; state.segments.len()];
        state.focus_segment = 0;
        state.mode = InputMode::Converting;
        drop(state);

        self.update_composition(context)
    }

    /// エンジンから変換結果をセグメント×候補の形で取得
    fn convert_segments(&self, hiragana: &str) -> Vec<Vec<Candidate>> {
        let mut result: Vec<Vec<Candidate>> = Vec::new();

        THREAD_ENGINE.with(|e| {
            let mut engine = e.borrow_mut();
            let Some(engine) = engine.as_mut() else { return };
            let Ok(segments) = engine.convert(hiragana, None) else {
                return;
            };
            result = segments;
        });

        result
    }

    // -----------------------------------------------------------------------
    // カタカナ・ひらがな変換
    // -----------------------------------------------------------------------

    fn handle_convert_to_hiragana(&self, context: &ITfContext) -> Result<()> {
        let preedit = {
            let state = self.state.borrow();
            if state.mode != InputMode::Converting {
                return Ok(());
            }
            state.preedit.clone()
        };
        let mut state = self.state.borrow_mut();
        state.segments = vec![vec![Candidate {
            surface: preedit.clone(),
            yomi: preedit,
            cost: 0.0,
            compound_word: false,
        }]];
        state.segment_indices = vec![0];
        state.focus_segment = 0;
        drop(state);
        self.update_composition(context)
    }

    fn handle_convert_to_katakana(&self, context: &ITfContext) -> Result<()> {
        let hiragana = {
            let mut state = self.state.borrow_mut();
            if state.mode != InputMode::Converting && state.mode != InputMode::Hiragana {
                return Ok(());
            }
            if !state.romaji_buffer.is_empty() {
                if let Some(converted) = romkan_convert(&state.romaji_buffer) {
                    state.preedit.push_str(&converted);
                    state.romaji_buffer.clear();
                }
            }
            state.preedit.clone()
        };

        let katakana: String = hiragana
            .chars()
            .map(|c| {
                if ('ぁ'..='ん').contains(&c) {
                    char::from_u32(c as u32 + 0x60).unwrap_or(c)
                } else {
                    c
                }
            })
            .collect();

        let mut state = self.state.borrow_mut();
        state.segments = vec![vec![Candidate {
            surface: katakana,
            yomi: hiragana,
            cost: 0.0,
            compound_word: false,
        }]];
        state.segment_indices = vec![0];
        state.focus_segment = 0;
        state.mode = InputMode::Converting;
        drop(state);
        self.update_composition(context)
    }

    // -----------------------------------------------------------------------
    // 確定
    // -----------------------------------------------------------------------

    fn learn_committed(&self) {
        let state = self.state.borrow();
        if state.mode != InputMode::Converting || state.segments.is_empty() {
            return;
        }
        let selected = state.selected_candidates();
        drop(state);
        if !selected.is_empty() {
            THREAD_ENGINE.with(|e| {
                if let Some(engine) = e.borrow_mut().as_mut() {
                    engine.learn(&selected);
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
        self.candidate_window.borrow_mut().hide();
        Ok(())
    }

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
                let range = comp.GetRange()?;
                range.SetText(ec, 0, &text_w)?;
                range.Collapse(ec, TfAnchor(1))?;
                ctx.SetSelection(ec, &[make_selection(range)])?;
                let _ = comp.EndComposition(ec);

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
        let _ = self.set_composition_text_with_rect(context, "");
        self.end_composition(context)?;
        self.state.borrow_mut().reset();
        self.candidate_window.borrow_mut().hide();
        Ok(())
    }

    fn handle_backspace(&self, context: &ITfContext) -> Result<()> {
        {
            let mut state = self.state.borrow_mut();
            if state.mode == InputMode::Converting {
                state.segments.clear();
                state.segment_indices.clear();
                state.focus_segment = 0;
                state.mode = InputMode::Hiragana;
            } else if !state.romaji_buffer.is_empty() {
                state.romaji_buffer.pop();
            } else if !state.preedit.is_empty() {
                state.preedit.pop();
            }

            if state.is_empty() {
                drop(state);
                let _ = self.set_composition_text_with_rect(context, "");
                self.end_composition(context)?;
                self.state.borrow_mut().reset();
                self.candidate_window.borrow_mut().hide();
                return Ok(());
            }
        }
        self.update_composition(context)
    }

    fn handle_candidate_prev(&self, context: &ITfContext) -> Result<()> {
        {
            let mut state = self.state.borrow_mut();
            if state.mode == InputMode::Converting {
                let seg_idx = state.focus_segment;
                if let Some(seg) = state.segments.get(seg_idx) {
                    if !seg.is_empty() {
                        let cur = state.segment_indices.get(seg_idx).copied().unwrap_or(0);
                        if cur > 0 {
                            if let Some(si) = state.segment_indices.get_mut(seg_idx) {
                                *si = cur - 1;
                            }
                        }
                    }
                }
            }
        }
        self.update_composition(context)
    }

    fn handle_candidate_next(&self, context: &ITfContext) -> Result<()> {
        {
            let mut state = self.state.borrow_mut();
            if state.mode == InputMode::Converting {
                let seg_idx = state.focus_segment;
                if let Some(seg) = state.segments.get(seg_idx) {
                    if !seg.is_empty() {
                        let cur = state.segment_indices.get(seg_idx).copied().unwrap_or(0);
                        let next = (cur + 1).min(seg.len() - 1);
                        if let Some(si) = state.segment_indices.get_mut(seg_idx) {
                            *si = next;
                        }
                    }
                }
            }
        }
        self.update_composition(context)
    }

    fn handle_segment_prev(&self, context: &ITfContext) -> Result<()> {
        {
            let mut state = self.state.borrow_mut();
            if state.mode == InputMode::Converting && state.focus_segment > 0 {
                state.focus_segment -= 1;
            }
        }
        self.update_composition(context)
    }

    fn handle_segment_next(&self, context: &ITfContext) -> Result<()> {
        {
            let mut state = self.state.borrow_mut();
            if state.mode == InputMode::Converting
                && state.focus_segment + 1 < state.segments.len()
            {
                state.focus_segment += 1;
            }
        }
        self.update_composition(context)
    }

    // -----------------------------------------------------------------------
    // コンポジション操作
    // -----------------------------------------------------------------------

    fn update_composition(&self, context: &ITfContext) -> Result<()> {
        let state = self.state.borrow();
        let text = state.display_text();
        let is_converting = state.mode == InputMode::Converting && !state.segments.is_empty();
        let cand_info = if is_converting {
            let seg_idx = state.focus_segment;
            let selected = state.segment_indices.get(seg_idx).copied().unwrap_or(0);
            let candidates: Vec<String> = state
                .segments
                .get(seg_idx)
                .map(|seg| seg.iter().map(|c| c.surface.clone()).collect())
                .unwrap_or_default();
            Some((candidates, selected))
        } else {
            None
        };
        drop(state);

        self.start_composition_if_needed(context)?;
        let caret_rect = self.set_composition_text_with_rect(context, &text)?;

        match (cand_info, caret_rect) {
            (Some((candidates, selected)), Some(rect)) => {
                self.candidate_window
                    .borrow_mut()
                    .show(&candidates, selected, rect.left, rect.bottom);
            }
            (Some((candidates, selected)), None) => {
                // キャレット位置が取れなくても表示（左上にフォールバック）
                self.candidate_window
                    .borrow_mut()
                    .show(&candidates, selected, 0, 0);
            }
            _ => {
                self.candidate_window.borrow_mut().hide();
            }
        }

        Ok(())
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

    fn set_composition_text_with_rect(
        &self,
        context: &ITfContext,
        text: &str,
    ) -> Result<Option<RECT>> {
        let comp = self.composition.borrow().clone();
        let Some(comp) = comp else {
            return Ok(None);
        };

        let text_w: Vec<u16> = text.encode_utf16().collect();
        let client_id = self.client_id();
        let rect_out: Rc<RefCell<Option<RECT>>> = Rc::new(RefCell::new(None));
        let rect_out2 = rect_out.clone();

        let _ = EditSession::execute(
            &context.clone(),
            client_id,
            TF_ES_READWRITE,
            move |ctx, ec| unsafe {
                let range = comp.GetRange()?;
                range.SetText(ec, 0, &text_w)?;

                // キャレット位置を取得 (Collapse する前に)
                if let Ok(view) = ctx.GetActiveView() {
                    let mut rect = RECT::default();
                    let mut clipped = BOOL::default();
                    if view.GetTextExt(ec, &range, &mut rect, &mut clipped).is_ok() {
                        *rect_out2.borrow_mut() = Some(rect);
                    }
                }

                range.Collapse(ec, TfAnchor(1))?;
                ctx.SetSelection(ec, &[make_selection(range)])?;
                Ok(())
            },
        )?;

        let result = rect_out.borrow().clone();
        Ok(result)
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
// キーアクション
// ---------------------------------------------------------------------------

enum KeyAction {
    Command(String),
    CharInput(char),
    Punctuation(char, char),
}

// ---------------------------------------------------------------------------
// ローマ字→かな 分離ヘルパー
// ---------------------------------------------------------------------------

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
                log(&format!("ActivateEx: PANIC: {}", panic_message(&p)));
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
            log("Deactivate: start");

            // コンポジション参照を解放する
            // (TSF が OnCompositionTerminated を呼んでクリーンアップする)
            let _ = self.composition.try_borrow_mut().map(|mut c| c.take());

            let _ = AkazaTextService::unadvise_key_event_sink(self);
            self.candidate_window.borrow_mut().destroy();
            self.state.borrow_mut().reset();
            *self.thread_mgr.borrow_mut() = None;
            *self.client_id.borrow_mut() = 0;
            // 次回 Activate 時にエンジン・キーマップ・romkan を再読み込み
            invalidate_engine();
            log("Deactivate: done");
        }));
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// ITfKeyEventSink
// ---------------------------------------------------------------------------

impl ITfKeyEventSink_Impl for AkazaTextService_Impl {
    fn OnSetFocus(&self, fforeground: BOOL) -> Result<()> {
        if !fforeground.as_bool() {
            // フォーカス喪失: TSF がコンポジションを終了するので内部状態もリセット
            self.candidate_window.borrow_mut().hide();
            let _ = self.composition.try_borrow_mut().map(|mut c| c.take());
            self.state.borrow_mut().reset();
        }
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
                    log(&format!("OnKeyDown: PANIC: {}", panic_message(&p)));
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
