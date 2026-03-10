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
│                   akaza_ime.dll (COM DLL)                 │
│                                                          │
│  ┌────────────────────┐    ┌──────────────────────────┐  │
│  │  TSF Frontend      │    │     libakaza             │  │
│  │                    │    │                           │  │
│  │ • ITfTextInput-    │    │ • かな漢字変換           │  │
│  │   ProcessorEx      │    │ • k-best 変換            │  │
│  │ • ITfKeyEventSink  │    │ • ユーザー学習           │  │
│  │ • ITfComposition-  │    │ • モデル/辞書ロード      │  │
│  │   Sink             │    │                           │  │
│  │ • ローマ字→かな    │    │                           │  │
│  │ • キー入力処理     │    │                           │  │
│  └────────────────────┘    └──────────────────────────┘  │
│                                                          │
│  %APPDATA%/akaza/                                        │
│  ├── model/default/                                      │
│  │   ├── unigram.model      (MARISA Trie)               │
│  │   ├── bigram.model       (MARISA Trie)               │
│  │   └── SKK-JISYO.akaza   (MARISA Trie)               │
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

## 関連プロジェクト

- [akaza](https://github.com/akaza-im/akaza) - Rust 製かな漢字変換エンジン (コア)
- [mac-akaza](https://github.com/akaza-im/mac-akaza) - macOS 版 Akaza IME
- [akaza-default-model](https://github.com/akaza-im/akaza-default-model) - デフォルト言語モデル
