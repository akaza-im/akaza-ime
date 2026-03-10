use libakaza::graph::candidate::Candidate;

/// IME の入力状態
#[derive(Debug, Clone, PartialEq)]
#[allow(dead_code)]
pub enum InputMode {
    /// 直接入力 (IME オフ)
    Direct,
    /// ひらがな入力中 (ローマ字→かな変換)
    Hiragana,
    /// 変換候補選択中
    Converting,
}

/// 入力バッファの状態を管理
pub struct InputState {
    pub mode: InputMode,
    /// ローマ字入力バッファ (未確定のローマ字)
    pub romaji_buffer: String,
    /// 変換前のひらがな
    pub preedit: String,
    /// 変換候補リスト
    pub candidates: Vec<String>,
    /// 選択中の候補インデックス
    pub candidate_index: usize,
    /// 変換セグメント情報 (学習用) — segments[candidate_index] が選択中の候補に対応
    pub segments: Vec<Vec<Candidate>>,
}

impl InputState {
    pub fn new() -> Self {
        Self {
            mode: InputMode::Direct,
            romaji_buffer: String::new(),
            preedit: String::new(),
            candidates: Vec::new(),
            candidate_index: 0,
            segments: Vec::new(),
        }
    }

    pub fn reset(&mut self) {
        self.romaji_buffer.clear();
        self.preedit.clear();
        self.candidates.clear();
        self.candidate_index = 0;
        self.segments.clear();
        // Converting → Hiragana に戻す。Direct はそのまま維持。
        if self.mode == InputMode::Converting {
            self.mode = InputMode::Hiragana;
        }
    }

    /// 確定するテキストを返す
    pub fn commit_text(&self) -> String {
        if self.mode == InputMode::Converting && !self.candidates.is_empty() {
            self.candidates[self.candidate_index].clone()
        } else {
            self.preedit.clone()
        }
    }

    /// プリエディットとして表示するテキスト
    pub fn display_text(&self) -> String {
        if self.mode == InputMode::Converting && !self.candidates.is_empty() {
            self.candidates[self.candidate_index].clone()
        } else {
            // ひらがな + 未変換ローマ字
            format!("{}{}", self.preedit, self.romaji_buffer)
        }
    }

    pub fn is_empty(&self) -> bool {
        self.preedit.is_empty() && self.romaji_buffer.is_empty()
    }
}
