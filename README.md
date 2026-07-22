# PixPipe

画像ファイル収集・重複排除・リネーム・JXL変換ツール (Ratatui TUI)

## Full Process (Move → Rename → Encode)

```
┌─────────────────────────────────────────────────────────────────┐
│                     Full Process Flow                           │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│   ┌──────────┐    ┌──────────┐    ┌──────────┐    ┌──────────┐ │
│   │  Input   │───▶│  Move    │───▶│  Rename  │───▶│  Encode  │ │
│   │  Files   │    │          │    │          │    │          │ │
│   └──────────┘    └──────────┘    └──────────┘    └──────────┘ │
│                                                                 │
│   Step 1: Move          Step 2: Rename       Step 3: Encode    │
│   ─────────────         ──────────────       ──────────────    │
│   • WalkDir で入力      • 日時ベースの       • HEIC → JPEG    │
│     ディレクトリを        リネーム            • RAW → JPEG     │
│     再帰的に走査        • 重複ファイルの      • JPEG → JXL     │
│   • 対象拡張子で          検出・削除          • 画質設定可能    │
│     フィルタリング      • Undo ログ記録      • lossless/lossy  │
│   • SHA256 ハッシュ      で巻き戻し可能                        │
│     で重複検出                                                │
│   • dest へコピー                                              │
│                                                                 │
└─────────────────────────────────────────────────────────────────┘
```

### Step 1: Move（ファイル収集）

```
┌─────────────────────────────────────────────────────────────────┐
│  Input Sources                     Destination (dest)           │
│  ─────────────                     ─────────────────           │
│                                                                 │
│  📁 /photos/2024/                  📁 /output/                  │
│  ├── IMG_001.heic ────────────────▶├── IMG_001.heic            │
│  ├── IMG_002.jpg  ────────────────▶├── IMG_002.jpg             │
│  📁 /camera/raw/                   ├── DSC_003.raw             │
│  ├── DSC_003.raw  ────────────────▶├── photo_004.png           │
│  📁 /downloads/                    └── ...                      │
│  ├── photo_004.png ───────────────▶                             │
│                                                                 │
│  対応拡張子:                                                      │
│  jpg, jpeg, png, gif, bmp, webp, tiff,                         │
│  heic, heif, avif, raw, cr2, nef, arw, rw2, dng               │
└─────────────────────────────────────────────────────────────────┘
```

### Step 2: Rename（リネーム・重複排除）

```
┌─────────────────────────────────────────────────────────────────┐
│  Before Rename                     After Rename                 │
│  ─────────────                     ────────────                 │
│                                                                 │
│  IMG_20240101_120000.heic ────────▶ 2024-01-01_12-00-00_001.jxl│
│  IMG_20240101_120000(1).heic ─────▶ (重複: hash一致 → 削除)     │
│  DSC_003.raw ─────────────────────▶ 2024-03-15_08-30-22_002.jxl│
│  photo_004.png ───────────────────▶ 2024-06-20_15-45-10_003.jxl│
│                                                                 │
│  リネーム規則:                                                    │
│  ──────────────                                                 │
│  {YYYY-MM-DD}_{HH-mm-ss}_{連番}.{ext}                          │
│                                                                 │
│  EXIF の DateTimeOriginal を優先使用                             │
│  EXIF がない場合はファイル更新日時を使用                           │
│                                                                 │
│  重複排除:                                                        │
│  ─────────                                                       │
│  SHA256 ハッシュ比較 → 同一ファイルは最新のみ保持                  │
│  Hash Cache DB でキャッシュ効率化                                 │
└─────────────────────────────────────────────────────────────────┘
```

### Step 3: Encode（JXL変換）

```
┌─────────────────────────────────────────────────────────────────┐
│  Input Format              Conversion              Output       │
│  ─────────────             ──────────              ──────       │
│                                                                 │
│  .heic  ──────────────┐                                        │
│  .heif  ──────────────┤     ┌─────────────┐                    │
│  .avif  ──────────────┼────▶│  cjxl       │──▶ .jxl            │
│  .jpg   ──────────────┤     │  (JXL       │   (JPEG XL)        │
│  .jpeg  ──────────────┤     │   Encoder)  │                    │
│  .png   ──────────────┤     └─────────────┘                    │
│  .bmp   ──────────────┤                                        │
│  .webp  ──────────────┘     設定:                               │
│                             ─────                               │
│  変換前: 100枚 × 平均5MB = 500MB        quality: 0-100         │
│  変換後: 100枚 × 平均2MB = 200MB        lossless: true/false   │
│                             ─────────────────────────────────   │
│  圧縮率: 約60%削減              デフォルト: quality=90, lossy   │
└─────────────────────────────────────────────────────────────────┘
```

## UI Overview (Ratatui TUI)

```
┌─ io-tool ──────────────────────────────────────────────────────┐
│ ■ ■ ■ ■ ■ ■ Theme: Cyberpunk                                   │
├─────────────────────────────────────────────────────────────────┤
│                                                                 │
│   ┌─ Menu ──────────────────────────────────────────────────┐  │
│   │                                                         │  │
│   │  ▶ 1. Full Process (Move → Rename → Encode)            │  │
│   │    2. Rename Only                                       │  │
│   │    3. Timestamp Rename                                  │  │
│   │    4. Image to JXL                                      │  │
│   │    5. Hash Cache DB                                     │  │
│   │    6. Batch Queue                                       │  │
│   │    7. Profiles                                          │  │
│   │    8. Watch Mode                                        │  │
│   │    9. Statistics                                        │  │
│   │   10. Duplicate Groups                                  │  │
│   │   11. JXL Settings                                      │  │
│   │   12. Settings                                          │  │
│   │                                                         │  │
│   └─────────────────────────────────────────────────────────┘  │
│                                                                 │
│ ┌─ Processing ────────────────────────────────────────────────┐ │
│ │ Step 3/3: Converting to JXL...                              │ │
│ │ ████████████████████░░░░░░░░░░  65%  32/50 files           │ │
│ │                                                             │ │
│ │ [18:30:15] ✓ Moved: photo001.jpg                           │ │
│ │ [18:30:16] ✓ Renamed: 2024-01-01_12-00-00_001.jpg         │ │
│ │ [18:30:17] ✓ Converted: 2024-01-01_12-00-00_001.jxl       │ │
│ │ [18:30:18] Converting: IMG_002.heic...                     │ │
│ └─────────────────────────────────────────────────────────────┘ │
│                                                                 │
│ ┌─ Info Bar ──────────────────────────────────────────────────┐ │
│ │ Memory: 128MB/512MB │ Errors: 0 │ Retry: 0 │ Filter: Off   │ │
│ └─────────────────────────────────────────────────────────────┘ │
│                                                                 │
├─────────────────────────────────────────────────────────────────┤
│ t:Theme d:DryRun u:Undo ?:Help Ctrl+P:Pause  [DRY RUN OFF]    │
└─────────────────────────────────────────────────────────────────┘
```

## Screens

### Help Screen (?)

```
┌─ Help ─────────────────────────────────────────────────────────┐
│  ═══════════════════════════════════════════════════════════    │
│    io-tool — Key Bindings Reference                            │
│  ═══════════════════════════════════════════════════════════    │
│                                                                │
│    Global Keys:                                                │
│    ──────────────                                              │
│      t         Cycle theme (6 colors)                          │
│      d         Toggle dry run mode                             │
│      u         Undo last rename                                │
│      ?         Show this help screen                           │
│      Ctrl+P    Pause/Resume processing                         │
│      Ctrl+E    Export log to file                              │
│                                                                │
│    Menu Navigation:                                            │
│    ──────────────                                              │
│      j/k       Navigate up/down                                │
│      1-9       Quick select menu item                          │
│      Enter     Run selected item                               │
│      q/Esc     Quit                                            │
│                                                                │
│  (j/k: Scroll │ PgUp/PgDn │ Esc/?: Close)                     │
└────────────────────────────────────────────────────────────────┘
```

### Statistics Dashboard (S)

```
┌─ Statistics Dashboard ─────────────────────────────────────────┐
│ ┌─ Summary ──────────────────────────────────────────────────┐ │
│ │  Total Runs: 42              Total Files Processed: 1,250  │ │
│ │  Total Files Removed: 89     Profiles: 3                   │ │
│ └────────────────────────────────────────────────────────────┘ │
│ ┌─ Files per Run (recent) ───────────────────────────────────┐ │
│ │    ██                                                      │ │
│ │    ██ ██                                                    │ │
│ │    ██ ██ ██ ██                                              │ │
│ │    ██ ██ ██ ██ ██ ██                                        │ │
│ │   ─────────────────────                                     │ │
│ │    R1  R2  R3  R4  R5  R6                                   │ │
│ └────────────────────────────────────────────────────────────┘ │
│ ┌─ History ──────────────────────────────────────────────────┐ │
│ │  2024-01-15  45 files  12s  0 errors                       │ │
│ │  2024-01-14  32 files   8s  1 error                        │ │
│ │  2024-01-13  28 files   7s  0 errors                       │ │
│ └────────────────────────────────────────────────────────────┘ │
│                                                                │
│  j/k: Scroll │ Esc: Back                                       │
└────────────────────────────────────────────────────────────────┘
```

### Duplicate Groups (d)

```
┌─ Duplicate Groups ─────────────────────────────────────────────┐
│ ┌─ Groups (3) ────────┐ ┌─ Files (hash: a1b2c3...) ─────────┐ │
│ │▶Group #1: 3 files   │ │ ★ KEEP  photo_001.jpg  (5.2MB)    │ │
│ │ Group #2: 2 files   │ │   delete IMG_001(1).jpg (5.2MB)    │ │
│ │ Group #3: 2 files   │ │   delete IMG_001(2).jpg (5.2MB)    │ │
│ │                     │ │                                    │ │
│ │                     │ │                                    │ │
│ └─────────────────────┘ └────────────────────────────────────┘ │
│                                                                │
│  j/k: Group │ h/l: File │ Space: Keep │ x: Delete │ Esc: Back │
└────────────────────────────────────────────────────────────────┘
```

### JXL Settings

```
┌─ JXL Settings ─────────────────────────────────────────────────┐
│                                                                │
│  ▶ Quality: 90 [████████████████████░░░░░░░░░░]                │
│    Lossless: false                                             │
│    Save & Back                                                 │
│                                                                │
│  h/l: Quality │ Space: Lossless │ Enter: Save │ Esc: Back     │
└────────────────────────────────────────────────────────────────┘
```

### Confirm Dialog

```
┌────────────────────────────────────────────────────────────────┐
│                                                                │
│              Start processing with selected steps?             │
│                                                                │
│              ▶ Yes    No                                        │
│                                                                │
│              j/k: Toggle │ y/Enter: Confirm │ n/Esc: Cancel    │
│                                                                │
└────────────────────────────────────────────────────────────────┘
```

## Key Bindings

| Key | Action |
|-----|--------|
| `t` | テーマ切替 (6色) |
| `d` | ドライラン ON/OFF |
| `u` | 直前のリネームを元に戻す |
| `?` | ヘルプ画面 |
| `Ctrl+P` | 処理の一時停止/再開 |
| `Ctrl+E` | ログをファイルにエクスポート |
| `j/k` | 上下移動 |
| `h/l` | 左右移動 / 値の調整 |
| `1-9` | メニュー項目のクイック選択 |
| `Enter` | 実行 / 確認 |
| `Esc/q` | 戻る / 終了 |
| `Space` | トグル / ステップ選択 |
| `/` | ログ検索 |
| `f` | フィルター＆ソート設定 |
| `s` | ソート順クイック切替 |
| `S` | 統計ダッシュボード |
| `p` | プロファイル管理 |
| `b` | バッチキュー |
| `w` | ウォッチモード |
| `i` | ファイル情報パネル |

## Build & Run

```bash
cargo build --release
cargo run
```

## New Features (Batch 2: 20 features)

### Size Comparison (#1)
Before/After comparison showing original vs compressed file sizes with compression ratio.

### Error Panel (#2)
Detailed error log with filename, message, timestamp, and step information.

### Conversion Presets (#3)
4 presets: Web (quality 80), Archive (lossless), Balance (quality 90), Max Quality (quality 100).

### Scheduler (#4)
Schedule recurring processing jobs with hour/minute/day-of-week configuration.

### History Export (#5)
Export processing history as CSV or JSON format.

### Theme Editor (#6)
Visual theme customization with RGB color editing for all theme channels.

### Dashboard Customization (#7)
Toggle visibility of individual dashboard widgets.

### Compression Graph (#8)
Bar chart visualization showing compression statistics (original vs compressed sizes).

### File Classification (#9)
Pattern-based file classification rules to organize files into target folders.

### Meta Edit (#10)
Edit file metadata (EXIF data) for processed images.

### Config Import/Export (#11)
Export/import configuration as JSON for migration between environments.

### Plugin System (#12)
Load and manage plugins from JSON descriptor files in ./plugins/ directory.

### Statusbar Customization (#13)
Customize which status bar items are visible and save preferences.

### Auto Parallelism (#14)
Dynamically adjust worker count based on CPU usage threshold.

### GPU Settings (#15)
Toggle GPU acceleration and configure GPU effort level (1-9).

### Memory-mapped I/O (#16)
Toggle memory-mapped file I/O for improved performance on large files.

### Additional Features (#17-20)
- Undo log with checkpoint restore
- Dynamic batch queue management
- Enhanced duplicate group navigation
- Real-time memory monitoring with sysinfo

## Ferrocopy-Inspired UI Components

New UI components inspired by ferrocopy's rendering engine:

| Component | Description | Usage |
|-----------|-------------|-------|
| `Toast` | Auto-dismiss notifications with type variants | `render_toasts()` |
| `StatusBadge` | Processing/Success/Failed/Warning status indicators | `render_status_badge()` |
| `ButtonVariant` | Primary/Secondary/Danger/Success/Ghost buttons | `render_button_variant()` |
| `BadgeVariant` | Info/Success/Warning/Error/Draft badges | `render_badge()` |
| `AlertVariant` | Info/Success/Warning/Error alerts with messages | `render_alert()` |
| `SectionHeading` | Themed section titles with emoji indicators | `render_section_heading()` |
| `EmptyState` | Placeholder for empty data with icon and message | `render_empty_state()` |
| `CardFrame` | Framed card containers with optional footer | `render_card_frame()` |
| `FileTableRow` | File listing with selection, favorite, size, status | `render_file_table_row()` |
| `ProgressDetail` | Step-by-step progress with status icons | `render_progress_detail()` |

### Toast System
```rust
// Add a toast notification
app.add_toast(ToastType::Success, "Operation completed".to_string());

// Auto-dismiss after 5 seconds
// Dismiss manually with 'n' key
```

### Status Badge Usage
```rust
StatusBadge::Processing  // "⏳ Processing"
StatusBadge::Success     // "✓ Success"
StatusBadge::Failed      // "✗ Failed"
StatusBadge::Warning     // "⚠ Warning"
StatusBadge::Pending     // "○ Pending"
```

## Bug Fixes (Latest)

### Extension Dot Fix
Fixed missing dot before file extension in timestamp-based filenames:
- Before: `20260720150238jpg` (incorrect)
- After: `20260720150238.jpg` (correct)

### UTF-8 Safety
Fixed `truncate_str` panic on multi-byte characters (Japanese, emoji) by checking char boundaries before slicing.

### NaN Safety
Fixed `partial_cmp().unwrap()` panic in file sorting when file sizes contain NaN values. Now uses `unwrap_or(Ordering::Equal)` for safe comparison.

### Key Bindings (New)
| Key | Action |
|-----|--------|
| `7` | Size Comparison |
| `8` | Error Panel |
| `9` | Presets |
| `0` | Scheduler |
| `Shift+E` | History Export |
| `Shift+T` | Theme Editor |
| `Shift+G` | Compression Graph |
| `Shift+C` | File Classification |
| `Shift+M` | Meta Edit |
| `Shift+I` | Config Import/Export |
| `Shift+P` | Plugins |
| `Shift+S` | Statusbar Customization |
| `n` | Dismiss toast notification |

### Ferrocopy UI Components
| Component | Key | Description |
|-----------|-----|-------------|
| Toast | Auto | Shows success/error/warning/info messages |
| StatusBadge | Auto | Displays processing status |
| ButtonVariant | Various | Different button styles for actions |
| BadgeVariant | Various | Status indicators |
| AlertVariant | Various | Alert messages with severity levels |

## Module Separation (Phase 3)

The monolithic `main.rs` (~10,500 lines) has been refactored into organized modules:

```
src/
├── main.rs           — App struct, event loop, render functions (~10,500 lines)
├── ui/
│   ├── mod.rs        — UI module declarations
│   ├── components.rs — Ferrocopy-inspired UI components (Toast, Badge, Alert, etc.)
│   └── render.rs     — Stub render functions for all screens
├── core_mod/
│   ├── mod.rs        — Core module declarations
│   ├── files.rs      — File operations (copy, move, hash, sanitize)
│   └── hash.rs       — Hash computation (SHA256, BLAKE3, XXH3, perceptual hash)
└── config/
    └── mod.rs        — AppConfig, KeyBindings, PluginConfig, ScheduledTask
```

### Key Changes
- `App`, `Theme`, `FileTreeNode` structs are now `pub(crate)` with `pub(crate)` fields
- All referenced types (AppState, MenuItem, Config, etc.) are `pub(crate)` for visibility consistency
- New modules have `#![allow(dead_code)]` for stub functions pending integration
- Added `toml` dependency for configuration serialization

## CI/CD Status

✅ All 3 platforms pass (ubuntu, windows, macos)
- Zero clippy warnings with `-W warnings` flag
- All 35 tests pass (24 unit + 11 integration)
- cargo fmt clean

## Dependencies

- `ratatui` 0.29 — TUI フレームワーク
- `crossterm` 0.28 — ターミナル制御
- `sha2` — ハッシュ計算
- `chrono` — 日時処理
- `walkdir` — ディレクトリ走査
- `serde` / `serde_json` — 設定ファイル
- `sysinfo` 0.30 — メモリ監視
- `rayon` — 並列処理
- `image` — 画像処理
- `jxl-oxide` — JPEG XL エンコード/デコード
- `blake3` — 高速ハッシュ
- `glob` — パターンマッチング
- `trash` — ゴミ箱移動
- `zip` — ZIP アーカイブ
- `open` — ファイル関連付け実行
- `anyhow` / `thiserror` — エラーハンドリング
- `toml` — 設定ファイルシリアライズ
- `log` / `env_logger` — ロギング
- `num_cpus` — CPU コア検出
- `zip-extract` — ZIP 展開
