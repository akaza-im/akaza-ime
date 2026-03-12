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
    /// 入力されたローマ字の全履歴 (F9/F10 用)
    pub raw_input: String,
    /// 変換前のひらがな
    pub preedit: String,
    /// セグメントごとの候補リスト: segments[seg_idx][cand_idx]
    pub segments: Vec<Vec<Candidate>>,
    /// 各セグメントで選択中の候補インデックス
    pub segment_indices: Vec<usize>,
    /// フォーカス中のセグメントインデックス
    pub focus_segment: usize,
}

impl InputState {
    pub fn new() -> Self {
        Self {
            mode: InputMode::Direct,
            romaji_buffer: String::new(),
            raw_input: String::new(),
            preedit: String::new(),
            segments: Vec::new(),
            segment_indices: Vec::new(),
            focus_segment: 0,
        }
    }

    pub fn reset(&mut self) {
        self.romaji_buffer.clear();
        self.raw_input.clear();
        self.preedit.clear();
        self.segments.clear();
        self.segment_indices.clear();
        self.focus_segment = 0;
        // Converting → Hiragana に戻す。Direct はそのまま維持。
        if self.mode == InputMode::Converting {
            self.mode = InputMode::Hiragana;
        }
    }

    /// 確定するテキストを返す
    pub fn commit_text(&self) -> String {
        if self.mode == InputMode::Converting && !self.segments.is_empty() {
            self.composed_text()
        } else {
            self.preedit.clone()
        }
    }

    /// プリエディットとして表示するテキスト
    pub fn display_text(&self) -> String {
        if self.mode == InputMode::Converting && !self.segments.is_empty() {
            self.composed_text()
        } else {
            // ひらがな + 未変換ローマ字
            format!("{}{}", self.preedit, self.romaji_buffer)
        }
    }

    /// セグメントごとの選択候補を結合したテキスト
    fn composed_text(&self) -> String {
        self.segments
            .iter()
            .enumerate()
            .map(|(i, seg)| {
                let idx = self.segment_indices.get(i).copied().unwrap_or(0);
                seg.get(idx)
                    .map(|c| c.surface.as_str())
                    .unwrap_or("")
            })
            .collect()
    }

    /// フォーカス中セグメントの選択候補を返す
    #[allow(dead_code)]
    pub fn focused_candidate(&self) -> Option<&Candidate> {
        let seg = self.segments.get(self.focus_segment)?;
        let idx = self.segment_indices.get(self.focus_segment).copied().unwrap_or(0);
        seg.get(idx)
    }

    /// 学習用: 各セグメントの選択済み候補を返す
    pub fn selected_candidates(&self) -> Vec<Candidate> {
        self.segments
            .iter()
            .enumerate()
            .filter_map(|(i, seg)| {
                let idx = self.segment_indices.get(i).copied().unwrap_or(0);
                seg.get(idx).cloned()
            })
            .collect()
    }

    pub fn is_empty(&self) -> bool {
        self.preedit.is_empty() && self.romaji_buffer.is_empty()
    }
}
