# Akaza for Windows

Windows 用の日本語かな漢字変換 IME。

[akaza](https://github.com/akaza-im/akaza) の変換エンジン（Rust）を利用し、Windows フロントエンドを Rust (TSF: Text Services Framework) で実装する。

## 動作環境

- Windows 10 以上
- x86_64

## アーキテクチャ

### 概要

単一の COM DLL として実装。TSF の `ITfTextInputProcessorEx` / `ITfKeyEventSink` を実装し、`libakaza` を直接リンクして変換を行う。

```
┌──────────────────────────────────────────────────────────┐
│                   akaza_ime.dll (COM DLL)                │
│                                                          │
│  ┌────────────────────┐    ┌──────────────────────────┐  │
│  │  TSF Frontend      │    │     libakaza             │  │
│  │                    │    │                          │  │
│  │ • ITfTextInput-    │    │ • かな漢字変換           │  │
│  │   ProcessorEx      │    │ • k-best 変換            │  │
│  │ • ITfKeyEventSink  │    │ • ユーザー学習           │  │
│  │ • ITfComposition-  │    │ • モデル/辞書ロード      │  │
│  │   Sink             │    │                          │  │
│  │ • ローマ字=>かな   │    │                           │  │
│  │ • キー入力処理     │    │                           │  │
│  └────────────────────┘    └──────────────────────────┘  │
│                                                          │
│  %APPDATA%/akaza/                                        │
│  ├── model/default/                                      │
│  │   ├── unigram.model      (MARISA Trie)                │
│  │   ├── bigram.model       (MARISA Trie)                │
│  │   └── SKK-JISYO.akaza    (MARISA Trie)                │
│  └── romkan/                                             │
│      └── default.yml                                     │
└──────────────────────────────────────────────────────────┘
```

## 開発

### 前提条件

- Rust (stable, msvc または mingw ターゲット)
- 管理者権限 (regsvr32 による登録)

### ビルド

```bash
cargo build --release
```

### DLL 登録

```bash
regsvr32 target\release\akaza_ime.dll
```

### DLL 登録解除

```bash
regsvr32 /u target\release\akaza_ime.dll
```

### コード変更後の反映

IME の DLL はプロセスにロードされるため、ビルド前にロックを解除する必要がある。

```bash
regsvr32 /u target\release\akaza_ime.dll
taskkill /F /IM explorer.exe
cargo build --release
start explorer.exe
regsvr32 target\release\akaza_ime.dll
```

### モデルデータの配置

[akaza-default-model](https://github.com/akaza-im/akaza-default-model/releases) からダウンロードし、`%APPDATA%\akaza\model\default\` に手動で配置する。

## キーバインド

| キー | 動作 |
|------|------|
| 全角/半角 | IME on/off トグル |
| Space | 変換 / 次の候補 |
| Enter | 確定 |
| Escape | キャンセル |
| Backspace | 削除 / 変換取り消し |
| ↑ / ↓ | 候補選択 |

### 句読点・記号

| 入力 | 出力 |
|------|------|
| `.` | 。 |
| `,` | 、 |
| `/` | ・ |
| `-` | ー |

## TODO

- [x] 候補ウィンドウ（候補一覧の UI 表示）
- [x] 文節区切りの移動・伸縮（Shift+←/→）
- [x] カタカナ変換（F7）
- [x] 半角カタカナ変換（F8）
- [x] 英数変換（F10）
- [x] ひらがなそのまま確定（F6）
- [x] 数字キー入力（全角数字 / 候補番号選択）
- [x] 記号の全角入力（! ? ( ) 等）
- [ ] ユーザー辞書の登録・編集 UI
- [x] 変換履歴の永続化
- [x] IME のオン/オフ状態をタスクバーに表示（言語バーアイコン）
- [ ] 入力モード表示（ひらがな/カタカナ/英数の切り替え表示）
- [ ] 設定画面
- [x] Shift キーでの大文字英字入力
- [ ] Tab キーでの予測変換
- [ ] 再変換（確定済みテキストの再変換）
- [ ] 前後の文脈を使った変換精度向上

## 関連プロジェクト

- [akaza](https://github.com/akaza-im/akaza) - Rust 製かな漢字変換エンジン (コア)
- [mac-akaza](https://github.com/akaza-im/mac-akaza) - macOS 版 Akaza IME
- [akaza-default-model](https://github.com/akaza-im/akaza-default-model) - デフォルト言語モデル
