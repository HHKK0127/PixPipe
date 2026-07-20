use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::io::{self, Write, BufReader, Read};
use std::collections::{HashSet, HashMap};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Instant, Duration};
use sha2::{Sha256, Digest};
use chrono::Utc;
use walkdir::WalkDir;
use serde::{Deserialize, Serialize};
use sysinfo::System;
use image::GenericImageView;
use crossterm::{
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
    execute,
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Gauge, List, ListItem, Paragraph, BarChart},
    Frame, Terminal,
};

// ============================================================
// Constants
// ============================================================

const BUFFER_SIZE: usize = 65536;

const DEFAULT_IMAGE_EXTENSIONS: &[&str] = &[
    ".jxl", ".jpg", ".jpeg", ".png", ".gif", ".bmp", ".webp",
    ".heic", ".heif", ".cr2", ".nef", ".arw", ".tiff", ".tif",
];

const FULL_STEP_LABELS: &[&str] = &[
    "STEP 1: Move files (Twitter & Downloads)",
    "STEP 2: Remove duplicates (SHA256)",
    "STEP 3: Remove files in reference folder",
    "STEP 4: Rename by timestamp + clean names",
    "STEP 5: Convert to JXL",
];

const THEME_NAMES: &[&str] = &["Cyan", "Green", "Magenta", "Yellow", "Blue", "Red"];

// ============================================================
// Config
// ============================================================

#[derive(Serialize, Deserialize, Clone)]
struct Config {
    twitter_src: String,
    download_src: String,
    dest: String,
    reference: String,
    days_to_check: i64,
    image_extensions: Vec<String>,
    min_file_size_kb: u64,
    max_workers: usize,
    // Feature #9: Profiles
    profiles: Vec<Profile>,
    // Feature #10: Keybindings
    keybindings: KeyBindings,
    // Feature #19: JXL quality
    jxl_quality: u32,
    jxl_lossless: bool,
    // Feature #20: Watch mode
    watch_dirs: Vec<String>,
    watch_interval_secs: u64,
    // Feature #17: Error retry
    max_retries: usize,
    // Feature #2: Pause/Resume
    checkpoint_file: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            twitter_src: r"Z:\gallery-dl\twitter\A_Quei_72".into(),
            download_src: r"C:\Users\E1192\Downloads".into(),
            dest: r"Z:\Pictures\Rename".into(),
            reference: r"Z:\R1".into(),
            days_to_check: 7,
            image_extensions: DEFAULT_IMAGE_EXTENSIONS.iter().map(|s| s.to_string()).collect(),
            min_file_size_kb: 0,
            max_workers: 4,
            profiles: Vec::new(),
            keybindings: KeyBindings::default(),
            jxl_quality: 90,
            jxl_lossless: true,
            watch_dirs: Vec::new(),
            watch_interval_secs: 5,
            max_retries: 3,
            checkpoint_file: "checkpoint.json".into(),
        }
    }
}

impl Config {
    fn load() -> Self {
        let path = PathBuf::from("config.json");
        if path.exists() {
            if let Ok(data) = fs::read_to_string(&path) {
                if let Ok(cfg) = serde_json::from_str(&data) {
                    return cfg;
                }
            }
        }
        let cfg = Self::default();
        let _ = cfg.save();
        cfg
    }

    fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let data = serde_json::to_string_pretty(self)?;
        fs::write("config.json", data)?;
        Ok(())
    }
}

// ============================================================
// History & Undo
// ============================================================

#[derive(Serialize, Deserialize, Clone, Default)]
struct HistoryEntry {
    timestamp: String,
    action: String,
    source: String,
    files_processed: usize,
    files_removed: usize,
    files_renamed: usize,
    original_size: u64,
    compressed_size: u64,
    duration_secs: f64,
    errors: usize,
}

#[derive(Serialize, Deserialize, Clone, Default)]
struct History {
    entries: Vec<HistoryEntry>,
    total_runs: usize,
    total_files_processed: usize,
    total_files_removed: usize,
}

impl History {
    fn load() -> Self {
        let path = PathBuf::from("history.json");
        if path.exists() {
            if let Ok(data) = fs::read_to_string(&path) {
                if let Ok(h) = serde_json::from_str(&data) {
                    return h;
                }
            }
        }
        Self::default()
    }

    fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let data = serde_json::to_string_pretty(self)?;
        fs::write("history.json", data)?;
        Ok(())
    }

    fn add(&mut self, entry: HistoryEntry) {
        self.total_runs += 1;
        self.total_files_processed += entry.files_processed;
        self.total_files_removed += entry.files_removed;
        self.entries.push(entry);
        if self.entries.len() > 100 {
            self.entries.drain(0..self.entries.len() - 100);
        }
        let _ = self.save();
    }
}

#[derive(Serialize, Deserialize, Clone)]
struct UndoEntry {
    old_path: String,
    new_path: String,
}

#[derive(Serialize, Deserialize, Clone, Default)]
struct UndoLog {
    entries: Vec<UndoEntry>,
}

impl UndoLog {
    fn load() -> Self {
        let path = PathBuf::from("undo_log.json");
        if path.exists() {
            if let Ok(data) = fs::read_to_string(&path) {
                if let Ok(u) = serde_json::from_str(&data) {
                    return u;
                }
            }
        }
        Self::default()
    }

    fn save(&self) -> Result<(), Box<dyn std::error::Error>> {
        let data = serde_json::to_string_pretty(self)?;
        fs::write("undo_log.json", data)?;
        Ok(())
    }

    fn _add(&mut self, old: &str, new: &str) {
        self.entries.push(UndoEntry {
            old_path: old.to_string(),
            new_path: new.to_string(),
        });
        let _ = self.save();
    }

    fn undo_last(&mut self) -> Option<(String, String)> {
        let entry = self.entries.pop()?;
        let _ = self.save();
        Some((entry.old_path, entry.new_path))
    }
}

// ============================================================
// KeyBindings (Feature #10)
// ============================================================

#[derive(Serialize, Deserialize, Clone)]
struct KeyBindings {
    quit: String,
    theme: String,
    dry_run: String,
    undo: String,
    help: String,
    filter: String,
    sort: String,
    profile: String,
    batch: String,
    export_log: String,
    pause: String,
    info: String,
    stats: String,
    watch: String,
}

impl Default for KeyBindings {
    fn default() -> Self {
        Self {
            quit: "q".into(), theme: "t".into(), dry_run: "d".into(),
            undo: "u".into(), help: "?".into(), filter: "f".into(),
            sort: "s".into(), profile: "p".into(), batch: "b".into(),
            export_log: "Ctrl+e".into(), pause: "Ctrl+p".into(),
            info: "i".into(), stats: "S".into(), watch: "w".into(),
        }
    }
}

// ============================================================
// Profiles (Feature #9)
// ============================================================

#[derive(Serialize, Deserialize, Clone)]
struct Profile {
    name: String,
    config: Config,
}

// ============================================================
// Batch Queue (Feature #4)
// ============================================================

#[derive(Serialize, Deserialize, Clone)]
struct BatchJob {
    path: String,
    status: String, // pending, processing, done, error
    files_processed: usize,
}

// ============================================================
// Duplicate Groups (Feature #3)
// ============================================================

#[derive(Clone)]
struct DuplicateGroup {
    hash: String,
    files: Vec<(String, u64)>, // (path, size)
    selected: usize,
}

// ============================================================
// File Filter (Feature #6)
// ============================================================

#[derive(Serialize, Deserialize, Clone, Default)]
struct FileFilter {
    extensions: Vec<String>,
    min_size_kb: u64,
    max_size_kb: u64,
    name_pattern: String,
}

impl FileFilter {
    fn matches(&self, path: &PathBuf) -> bool {
        // Extension filter
        if !self.extensions.is_empty() {
            if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                if !self.extensions.iter().any(|e| e.eq_ignore_ascii_case(&format!(".{}", ext))) {
                    return false;
                }
            } else {
                return false;
            }
        }
        // Size filter
        if self.min_size_kb > 0 || self.max_size_kb > 0 {
            if let Ok(meta) = fs::metadata(path) {
                let size_kb = meta.len() / 1024;
                if self.min_size_kb > 0 && size_kb < self.min_size_kb { return false; }
                if self.max_size_kb > 0 && size_kb > self.max_size_kb { return false; }
            }
        }
        // Name pattern filter
        if !self.name_pattern.is_empty() {
            let name = path.file_name().unwrap().to_string_lossy().to_lowercase();
            if !name.contains(&self.name_pattern.to_lowercase()) { return false; }
        }
        true
    }
}

// ============================================================
// Sort Config (Feature #7)
// ============================================================

#[derive(Clone, PartialEq, Debug)]
enum SortField { Name, Size, Date, Type }

#[derive(Clone)]
struct SortConfig {
    field: SortField,
    ascending: bool,
}

impl Default for SortConfig {
    fn default() -> Self { Self { field: SortField::Name, ascending: true } }
}

// ============================================================
// Confirm Actions (Feature #13)
// ============================================================

#[derive(Clone, PartialEq)]
enum ConfirmAction {
    StartProcessing,
    ClearHistory,
    ClearUndo,
}

// ============================================================
// Checkpoint (Feature #2)
// ============================================================

#[derive(Serialize, Deserialize, Clone, Default)]
struct Checkpoint {
    step: usize,
    files_processed: usize,
    timestamp: String,
}

// ============================================================
// State Restore (Feature #15)
// ============================================================

#[derive(Serialize, Deserialize, Clone, Default)]
struct AppStateStore {
    last_menu_idx: usize,
    last_theme_idx: usize,
    last_dry_run: bool,
}

impl AppStateStore {
    fn load() -> Self {
        let path = PathBuf::from(".io_tool_state.json");
        if path.exists() {
            if let Ok(data) = fs::read_to_string(&path) {
                if let Ok(s) = serde_json::from_str(&data) { return s; }
            }
        }
        Self::default()
    }
    fn save(&self) {
        let _ = fs::write(".io_tool_state.json", serde_json::to_string_pretty(self).unwrap_or_default());
    }
}

// ============================================================
// Conversion Presets (New Feature #4)
// ============================================================

#[derive(Serialize, Deserialize, Clone)]
struct ConversionPreset {
    name: String,
    quality: u8,
    lossless: bool,
    description: String,
}

impl Default for ConversionPreset {
    fn default() -> Self {
        Self {
            name: "Default".into(),
            quality: 90,
            lossless: false,
            description: "Balanced quality and size".into(),
        }
    }
}

impl ConversionPreset {
    fn presets() -> Vec<Self> {
        vec![
            Self { name: "Web".into(), quality: 80, lossless: false, description: "Smaller files for web".into() },
            Self { name: "Archive".into(), quality: 100, lossless: true, description: "Lossless for archiving".into() },
            Self { name: "Balance".into(), quality: 90, lossless: false, description: "Quality-size balance".into() },
            Self { name: "Max Quality".into(), quality: 100, lossless: false, description: "Maximum quality lossy".into() },
        ]
    }
}

// ============================================================
// Size Comparison (New Feature #1)
// ============================================================

#[derive(Clone)]
struct SizeComparison {
    filename: String,
    original_size: u64,
    converted_size: u64,
    reduction_pct: f64,
}

// ============================================================
// Error Detail (New Feature #3)
// ============================================================

#[derive(Clone)]
struct ErrorDetail {
    filename: String,
    error_msg: String,
    timestamp: String,
    _step: String,
}

// ============================================================
// Compression Stats (New Feature #6)
// ============================================================

#[derive(Clone)]
struct CompressionStat {
    format: String,
    original_size: u64,
    converted_size: u64,
    count: usize,
}

// ============================================================
// Scheduler (New Feature #9)
// ============================================================

#[derive(Serialize, Deserialize, Clone)]
struct SchedulerJob {
    name: String,
    hour: u8,
    minute: u8,
    days: Vec<u8>, // 0=Sun, 1=Mon, ...
    enabled: bool,
    last_run: Option<String>,
}

impl Default for SchedulerJob {
    fn default() -> Self {
        Self {
            name: "Default Job".into(),
            hour: 2,
            minute: 0,
            days: vec![1, 2, 3, 4, 5],
            enabled: false,
            last_run: None,
        }
    }
}

// ============================================================
// Theme Config (New Feature #11)
// ============================================================

#[derive(Serialize, Deserialize, Clone)]
struct ThemeConfig {
    name: String,
    primary: (u8, u8, u8),
    secondary: (u8, u8, u8),
    accent: (u8, u8, u8),
    success: (u8, u8, u8),
    warning: (u8, u8, u8),
    error: (u8, u8, u8),
    bg: (u8, u8, u8),
    fg: (u8, u8, u8),
    muted: (u8, u8, u8),
    bg_highlight: (u8, u8, u8),
}

impl Default for ThemeConfig {
    fn default() -> Self {
        Self {
            name: "Custom".into(),
            primary: (0, 255, 255),
            secondary: (128, 0, 255),
            accent: (255, 128, 0),
            success: (0, 255, 128),
            warning: (255, 255, 0),
            error: (255, 64, 64),
            bg: (20, 20, 30),
            fg: (240, 240, 240),
            muted: (100, 100, 120),
            bg_highlight: (40, 40, 60),
        }
    }
}

// ============================================================
// Widget Layout (New Feature #12)
// ============================================================

#[derive(Serialize, Deserialize, Clone)]
struct WidgetLayout {
    show_summary: bool,
    show_chart: bool,
    show_history: bool,
    show_compression: bool,
    chart_position: (u16, u16), // row, col
}

impl Default for WidgetLayout {
    fn default() -> Self {
        Self {
            show_summary: true,
            show_chart: true,
            show_history: true,
            show_compression: false,
            chart_position: (0, 0),
        }
    }
}

// ============================================================
// Plugin Info (New Feature #20)
// ============================================================

#[derive(Clone)]
struct PluginInfo {
    name: String,
    _path: String,
    enabled: bool,
    description: String,
}

// ============================================================
// Batch 3: New structs
// ============================================================

// B3 #1: Image preview (ASCII art)
struct ImagePreview {
    ascii_lines: Vec<String>,
    width: usize,
    height: usize,
    filename: String,
}

// B3 #9: Recent files
struct RecentFile {
    path: String,
    processed_at: String,
    file_type: String,
    size: u64,
}

// B3 #10: Tag system
struct FileTag {
    file_pattern: String,
    tags: Vec<String>,
}

// B3 #12: File tree node
#[derive(Clone)]
struct FileTreeNode {
    name: String,
    path: String,
    is_dir: bool,
    expanded: bool,
    depth: usize,
    children: Vec<FileTreeNode>,
}

// B3 #13: Batch rename pattern
struct RenamePattern {
    pattern: String,
    replacement: String,
    preview: Vec<(String, String)>, // (old_name, new_name)
    use_regex: bool,
}

// B3 #14: Processing timeline entry
struct TimelineEntry {
    timestamp: String,
    event_type: String, // "start", "progress", "complete", "error"
    description: String,
    progress: f64,
}

// B3 #17: Notification
struct Notification {
    message: String,
    timestamp: String,
    level: String, // "info", "warning", "error", "success"
    read: bool,
}

// B3 #20: Keyboard macro
struct KeyMacro {
    name: String,
    keys: Vec<String>, // Key descriptions
    recording: bool,
}

// Similar image search
struct SimilarImageGroup {
    hash: u64,
    files: Vec<(String, u64)>, // (path, file_size)
    hash_type: String,         // "aHash" or "dHash"
}

// ============================================================
// Theme
// ============================================================

struct Theme {
    primary: Color,
    _secondary: Color,
    accent: Color,
    success: Color,
    error: Color,
    warning: Color,
    muted: Color,
    bg_highlight: Color,
    bg: Color,
    fg: Color,
}

impl Theme {
    fn from_index(idx: usize) -> Self {
        match idx % 6 {
            0 => Theme {
                primary: Color::Cyan, _secondary: Color::LightCyan, accent: Color::White,
                success: Color::Green, error: Color::Red, warning: Color::Yellow,
                muted: Color::DarkGray, bg_highlight: Color::Cyan,
                bg: Color::Black, fg: Color::White,
            },
            1 => Theme {
                primary: Color::Green, _secondary: Color::LightGreen, accent: Color::White,
                success: Color::LightGreen, error: Color::Red, warning: Color::Yellow,
                muted: Color::DarkGray, bg_highlight: Color::Green,
                bg: Color::Black, fg: Color::White,
            },
            2 => Theme {
                primary: Color::Magenta, _secondary: Color::LightMagenta, accent: Color::White,
                success: Color::Green, error: Color::Red, warning: Color::Yellow,
                muted: Color::DarkGray, bg_highlight: Color::Magenta,
                bg: Color::Black, fg: Color::White,
            },
            3 => Theme {
                primary: Color::Yellow, _secondary: Color::LightYellow, accent: Color::Black,
                success: Color::Green, error: Color::Red, warning: Color::LightYellow,
                muted: Color::DarkGray, bg_highlight: Color::Yellow,
                bg: Color::Black, fg: Color::White,
            },
            4 => Theme {
                primary: Color::Blue, _secondary: Color::LightBlue, accent: Color::White,
                success: Color::Green, error: Color::Red, warning: Color::Yellow,
                muted: Color::DarkGray, bg_highlight: Color::Blue,
                bg: Color::Black, fg: Color::White,
            },
            _ => Theme {
                primary: Color::Red, _secondary: Color::LightRed, accent: Color::White,
                success: Color::Green, error: Color::LightRed, warning: Color::Yellow,
                muted: Color::DarkGray, bg_highlight: Color::Red,
                bg: Color::Black, fg: Color::White,
            },
        }
    }
}

// ============================================================
// Animated gauge chars
// ============================================================

const GAUGE_CHARS: &[char] = &['░', '▒', '▓', '█'];
const SPINNER_CHARS: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

fn make_gauge_bar(ratio: f64, width: usize) -> String {
    let filled = (ratio * width as f64) as usize;
    let empty = width.saturating_sub(filled);
    let partial = ((ratio * width as f64) - filled as f64) * 4.0;
    let partial_idx = partial as usize;

    let mut bar = String::new();
    for _ in 0..filled {
        bar.push('█');
    }
    if filled < width && partial_idx > 0 {
        bar.push(GAUGE_CHARS[partial_idx.min(3)]);
    }
    for _ in 0..empty.saturating_sub(if partial_idx > 0 { 1 } else { 0 }) {
        bar.push('░');
    }
    bar
}

fn _make_sub_progress_bar(label: &str, ratio: f64, width: usize) -> String {
    let bar = make_gauge_bar(ratio, width);
    format!("{} [{}] {:.0}%", label, bar, ratio * 100.0)
}

// ============================================================
// Utility
// ============================================================

fn format_size(bytes: u64) -> String {
    if bytes >= 1_073_741_824 {
        format!("{:.1} GB", bytes as f64 / 1_073_741_824.0)
    } else if bytes >= 1_048_576 {
        format!("{:.1} MB", bytes as f64 / 1_048_576.0)
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{} B", bytes)
    }
}

fn format_duration(secs: f64) -> String {
    if secs < 60.0 {
        format!("{:.1}s", secs)
    } else if secs < 3600.0 {
        format!("{}m {:.0}s", (secs / 60.0) as u32, secs % 60.0)
    } else {
        format!("{}h {}m", (secs / 3600.0) as u32, ((secs % 3600.0) / 60.0) as u32)
    }
}

// ============================================================
// Menu Items
// ============================================================

#[derive(Clone, Copy, PartialEq)]
enum MenuItem {
    FullProcess,
    RenameOnly,
    TimestampRename,
    ImageToJxl,
    HashCacheDb,
    Settings,
    BatchQueue,      // Feature #4
    Profiles,        // Feature #9
    WatchMode,       // Feature #20
    Statistics,      // Feature #8
    Duplicates,      // Feature #3
    JxlSettings,     // Feature #19
    SizeCompare,     // New #1
    ErrorPanel,      // New #3
    Presets,         // New #4
    Scheduler,       // New #9
    HistoryExport,   // New #10
    ThemeEditor,     // New #11
    CompressionGraph,// New #6
    FileClassify,    // New #7
    MetaEdit,        // New #8
    ConfigIO,        // New #19
    Plugins,         // New #20
    // Batch 3
    ImagePreview,    // B3 #1
    FuzzyFinder,     // B3 #3
    SplitPane,       // B3 #6
    QuickActions,    // B3 #8
    RecentFiles,     // B3 #9
    TagSystem,       // B3 #10
    SideBySideDiff,  // B3 #11
    FileTreeView,    // B3 #12
    RenamePattern,   // B3 #13
    Timeline,        // B3 #14
    CommandPalette,  // B3 #16
    NotificationCenter, // B3 #17
    ExportReport,    // B3 #19
    SimilarImages,   // Similar image search
}

impl MenuItem {
    fn all() -> &'static [MenuItem] {
        &[
            MenuItem::FullProcess,
            MenuItem::RenameOnly,
            MenuItem::TimestampRename,
            MenuItem::ImageToJxl,
            MenuItem::HashCacheDb,
            MenuItem::Settings,
            MenuItem::BatchQueue,
            MenuItem::Profiles,
            MenuItem::WatchMode,
            MenuItem::Statistics,
            MenuItem::Duplicates,
            MenuItem::JxlSettings,
            MenuItem::SizeCompare,
            MenuItem::ErrorPanel,
            MenuItem::Presets,
            MenuItem::Scheduler,
            MenuItem::HistoryExport,
            MenuItem::ThemeEditor,
            MenuItem::CompressionGraph,
            MenuItem::FileClassify,
            MenuItem::MetaEdit,
            MenuItem::ConfigIO,
            MenuItem::Plugins,
            // Batch 3
            MenuItem::ImagePreview,
            MenuItem::FuzzyFinder,
            MenuItem::SplitPane,
            MenuItem::QuickActions,
            MenuItem::RecentFiles,
            MenuItem::TagSystem,
            MenuItem::SideBySideDiff,
            MenuItem::FileTreeView,
            MenuItem::RenamePattern,
            MenuItem::Timeline,
            MenuItem::CommandPalette,
            MenuItem::NotificationCenter,
            MenuItem::ExportReport,
            MenuItem::SimilarImages,
        ]
    }

    fn label(&self) -> &str {
        match self {
            MenuItem::FullProcess => "Full Process (Move → Rename → Encode)",
            MenuItem::RenameOnly => "Rename Only (Remove _ and parentheses)",
            MenuItem::TimestampRename => "Timestamp Rename",
            MenuItem::ImageToJxl => "Image to JXL Conversion",
            MenuItem::HashCacheDb => "Hash Cache Database",
            MenuItem::Settings => "Settings",
            MenuItem::BatchQueue => "Batch Queue",
            MenuItem::Profiles => "Config Profiles",
            MenuItem::WatchMode => "Watch Mode",
            MenuItem::Statistics => "Statistics Dashboard",
            MenuItem::Duplicates => "Duplicate Groups",
            MenuItem::JxlSettings => "JXL Quality Settings",
            MenuItem::SizeCompare => "Size Comparison",
            MenuItem::ErrorPanel => "Error Details",
            MenuItem::Presets => "Conversion Presets",
            MenuItem::Scheduler => "Process Scheduler",
            MenuItem::HistoryExport => "Export History",
            MenuItem::ThemeEditor => "Theme Editor",
            MenuItem::CompressionGraph => "Compression Graph",
            MenuItem::FileClassify => "File Classification",
            MenuItem::MetaEdit => "Metadata Editor",
            MenuItem::ConfigIO => "Config Import/Export",
            MenuItem::Plugins => "Plugins",
            // Batch 3
            MenuItem::ImagePreview => "Image Preview",
            MenuItem::FuzzyFinder => "Fuzzy Finder",
            MenuItem::SplitPane => "Split Pane View",
            MenuItem::QuickActions => "Quick Actions",
            MenuItem::RecentFiles => "Recent Files",
            MenuItem::TagSystem => "Tag System",
            MenuItem::SideBySideDiff => "Side-by-side Diff",
            MenuItem::FileTreeView => "File Tree View",
            MenuItem::RenamePattern => "Batch Rename Pattern",
            MenuItem::Timeline => "Processing Timeline",
            MenuItem::CommandPalette => "Command Palette",
            MenuItem::NotificationCenter => "Notification Center",
            MenuItem::ExportReport => "Export Report",
            MenuItem::SimilarImages => "Similar Image Search",
        }
    }

    fn description(&self) -> &str {
        match self {
            MenuItem::FullProcess => "Run all steps in sequence. Select individual steps to enable/disable.",
            MenuItem::RenameOnly => "Remove underscores and parentheses from filenames",
            MenuItem::TimestampRename => "Rename files based on last modified timestamp",
            MenuItem::ImageToJxl => "Convert images to lossless JXL format",
            MenuItem::HashCacheDb => "Run hash_cache_db.exe for duplicate detection",
            MenuItem::Settings => "Configure paths, extensions, and processing options",
            MenuItem::BatchQueue => "Add multiple folders to process sequentially",
            MenuItem::Profiles => "Save and load named configuration profiles",
            MenuItem::WatchMode => "Monitor folders for new files and auto-process",
            MenuItem::Statistics => "View processing history and statistics",
            MenuItem::Duplicates => "View and manage duplicate file groups",
            MenuItem::JxlSettings => "Configure JXL conversion quality (lossless/lossy)",
            MenuItem::SizeCompare => "Compare file sizes before and after conversion",
            MenuItem::ErrorPanel => "View detailed error information for failed files",
            MenuItem::Presets => "Quick conversion presets (Web/Archive/Balance)",
            MenuItem::Scheduler => "Schedule batch processing at specified times",
            MenuItem::HistoryExport => "Export processing history to CSV/JSON",
            MenuItem::ThemeEditor => "Customize color theme palette",
            MenuItem::CompressionGraph => "View compression ratio by file format",
            MenuItem::FileClassify => "Auto-classify files by type/date/size",
            MenuItem::MetaEdit => "Batch edit EXIF metadata",
            MenuItem::ConfigIO => "Import/export configuration files",
            MenuItem::Plugins => "Manage custom conversion plugins",
            // Batch 3
            MenuItem::ImagePreview => "Preview images as ASCII art in terminal",
            MenuItem::FuzzyFinder => "Fuzzy search files by name",
            MenuItem::SplitPane => "Split view for input/output comparison",
            MenuItem::QuickActions => "Quick access to common actions",
            MenuItem::RecentFiles => "View recently processed files",
            MenuItem::TagSystem => "Tag and categorize files",
            MenuItem::SideBySideDiff => "Compare before/after file sizes",
            MenuItem::FileTreeView => "Browse directory tree structure",
            MenuItem::RenamePattern => "Preview batch rename with regex patterns",
            MenuItem::Timeline => "Visual timeline of processing history",
            MenuItem::CommandPalette => "Search and execute any command",
            MenuItem::NotificationCenter => "View notification history",
            MenuItem::ExportReport => "Export processing report",
            MenuItem::SimilarImages => "Find visually similar images using perceptual hashing",
        }
    }
}

// ============================================================
// App State
// ============================================================

#[derive(Clone, PartialEq)]
enum AppState {
    Menu,
    StepSelect,
    Preview,
    Processing,
    Done,
    Settings,
    Help,              // Feature #14
    BatchQueue,        // Feature #4
    DuplicateGroups,   // Feature #3
    Stats,             // Feature #8
    Profiles,          // Feature #9
    JxlSettings,       // Feature #19
    WatchMode,         // Feature #20
    FilterSort,        // Feature #6, #7
    InfoPanel,         // Feature #11
    ConfirmDialog,     // Feature #13
    // New features
    SizeCompare,       // New #1: Size comparison
    ErrorPanel,        // New #3: Error details
    Presets,           // New #4: Conversion presets
    Scheduler,         // New #9: Process scheduler
    HistoryExport,     // New #10: History export
    ThemeEditor,       // New #11: Theme editor
    DashboardCustom,   // New #12: Dashboard customization
    CompressionGraph,  // New #6: Compression graph
    FileClassify,      // New #7: File classification
    MetaEdit,          // New #8: Metadata batch edit
    ConfigIO,          // New #19: Config import/export
    Plugins,           // New #20: Plugin system
    StatusbarCustom,   // New #15: Statusbar customization
    // Batch 3
    ImagePreview,      // B3 #1: Image preview
    SplitPane,         // B3 #6: Split pane view
    QuickActions,      // B3 #8: Quick actions menu
    RecentFiles,       // B3 #9: Recent files
    TagSystem,         // B3 #10: Tag system
    SideBySideDiff,    // B3 #11: Side-by-side diff
    FileTreeView,      // B3 #12: File tree view
    RenamePattern,     // B3 #13: Batch rename pattern
    Timeline,          // B3 #14: Processing timeline
    CommandPalette,    // B3 #16: Command palette
    NotificationCenter,// B3 #17: Notification center
    ExportReport,      // B3 #19: Export report
    SimilarImages,     // Similar image search
}

struct App {
    state: AppState,
    menu_items: Vec<MenuItem>,
    selected: usize,
    config: Config,
    history: History,
    undo_log: UndoLog,
    theme_idx: usize,
    dry_run: bool,
    // Step selection
    step_enabled: Vec<bool>,
    step_selected: usize,
    // Preview
    preview_items: Vec<(String, String)>,
    preview_scroll: usize,
    preview_file_count: usize,
    preview_total_size: u64,
    // Processing
    logs: Arc<Mutex<Vec<String>>>,
    progress: Arc<Mutex<f64>>,
    progress_detail: Arc<Mutex<String>>,
    current_step: Arc<Mutex<String>>,
    is_processing: Arc<Mutex<bool>>,
    errors: Arc<Mutex<Vec<String>>>,
    step_progress: Arc<Mutex<Vec<f64>>>,
    start_time: Arc<Mutex<Option<Instant>>>,
    files_processed: Arc<Mutex<usize>>,
    // Log search
    search_mode: bool,
    search_query: String,
    filtered_log_indices: Vec<usize>,
    // Settings
    settings_selected: usize,
    // Batch queue (Feature #4)
    batch_queue: Vec<BatchJob>,
    batch_selected: usize,
    batch_adding: bool,
    batch_input: String,
    // Animation
    spinner_idx: usize,
    frame_count: u64,
    // Feature #2: Pause/Resume
    is_paused: Arc<Mutex<bool>>,
    checkpoint: Arc<Mutex<Option<Checkpoint>>>,
    // Feature #3: Duplicate groups
    duplicate_groups: Vec<DuplicateGroup>,
    dup_group_selected: usize,
    dup_file_selected: usize,
    // Feature #6, #7: Filter & Sort
    filter: FileFilter,
    sort_config: SortConfig,
    filter_active: bool,
    filter_selected: usize,
    // Feature #8: Stats
    stats_scroll: usize,
    // Feature #9: Profiles
    profile_selected: usize,
    profile_input: String,
    profile_adding: bool,
    // Feature #11: Info panel
    info_selected: usize,
    // Feature #13: Confirm dialog
    confirm_action: Option<ConfirmAction>,
    confirm_yes: bool,
    // Feature #14: Help
    help_scroll: usize,
    // Feature #15: State restore
    state_store: AppStateStore,
    // Feature #16: Memory monitoring
    sys_info: System,
    // Feature #17: Error retry
    retry_count: usize,
    // Feature #20: Watch mode
    watch_active: bool,
    watch_processed: usize,
    watch_last_scan: Instant,
    // New features
    // New #1: Size comparison
    size_comparisons: Vec<SizeComparison>,
    size_compare_scroll: usize,
    // New #3: Error details
    error_details: Vec<ErrorDetail>,
    error_scroll: usize,
    // New #4: Conversion presets
    presets: Vec<ConversionPreset>,
    preset_selected: usize,
    active_preset: usize,
    // New #6: Compression stats
    compression_stats: Vec<CompressionStat>,
    compress_scroll: usize,
    // New #7: File classification
    classify_rules: Vec<(String, String)>, // (pattern, target_folder)
    classify_selected: usize,
    classify_input: String,
    classify_adding: bool,
    // New #8: Metadata edit
    meta_files: Vec<(String, bool)>, // (filename, selected)
    meta_scroll: usize,
    meta_field: usize, // 0=datetime, 1=artist, 2=remove all
    // New #9: Scheduler
    scheduler_jobs: Vec<SchedulerJob>,
    scheduler_selected: usize,
    scheduler_editing: bool,
    scheduler_field: usize,
    // New #10: History export
    export_format: usize, // 0=CSV, 1=JSON
    // New #11: Theme editor
    custom_themes: Vec<ThemeConfig>,
    theme_edit_selected: usize,
    theme_edit_field: usize,
    // New #12: Dashboard customization
    widget_layout: WidgetLayout,
    dashboard_editing: bool,
    dashboard_selected: usize,
    // New #15: Statusbar customization
    statusbar_items: Vec<(String, bool)>, // (name, enabled)
    statusbar_selected: usize,
    // New #16: GPU acceleration
    use_gpu: bool,
    gpu_effort: u8, // 1-9
    // New #17: Memory-mapped I/O
    use_mmap: bool,
    // New #18: Auto parallelism
    auto_parallel: bool,
    current_workers: usize,
    cpu_threshold: f64,
    // New #19: Config import/export
    config_io_selected: usize,
    config_io_path: String,
    config_io_adding: bool,
    // New #20: Plugins
    plugins: Vec<PluginInfo>,
    plugin_selected: usize,
    plugin_dir: String,
    // Batch 3 fields
    // B3 #1: Image preview
    image_preview: Option<ImagePreview>,
    preview_image_path: String,
    // B3 #2: Vim-style navigation
    vim_buffer: String,
    // B3 #3: Fuzzy finder
    fuzzy_mode: bool,
    fuzzy_query: String,
    fuzzy_results: Vec<String>,
    fuzzy_selected: usize,
    // B3 #4: Multi-source watch
    watch_dirs: Vec<String>,
    watch_dir_selected: usize,
    watch_dir_adding: bool,
    watch_dir_input: String,
    // B3 #5: Drag and drop
    drop_zone_active: bool,
    drop_queue: Vec<String>,
    // B3 #6: Split pane
    split_mode: bool,
    split_left_scroll: usize,
    split_right_scroll: usize,
    // B3 #7: Breadcrumb
    breadcrumb: Vec<String>,
    // B3 #8: Quick actions
    quick_actions: Vec<(String, usize)>, // (label, action_index)
    quick_selected: usize,
    // B3 #9: Recent files
    recent_files: Vec<RecentFile>,
    recent_scroll: usize,
    // B3 #10: Tag system
    file_tags: Vec<FileTag>,
    tag_selected: usize,
    tag_input: String,
    tag_adding: bool,
    // B3 #11: Side-by-side diff
    diff_left: Vec<String>,
    diff_right: Vec<String>,
    diff_scroll: usize,
    // B3 #12: File tree
    file_tree: Vec<FileTreeNode>,
    tree_selected: usize,
    tree_scroll: usize,
    // B3 #13: Rename pattern
    rename_patterns: Vec<RenamePattern>,
    rename_selected: usize,
    rename_input: String,
    rename_field: usize, // 0=pattern, 1=replacement
    // B3 #14: Timeline
    timeline_entries: Vec<TimelineEntry>,
    timeline_scroll: usize,
    // B3 #15: Startup wizard
    wizard_step: usize,
    wizard_done: bool,
    // B3 #16: Command palette
    palette_open: bool,
    palette_query: String,
    palette_results: Vec<(String, usize)>, // (label, menu_idx)
    palette_selected: usize,
    // B3 #17: Notification center
    notifications: Vec<Notification>,
    notif_scroll: usize,
    // B3 #18: Widget system
    widgets: Vec<(String, bool)>, // (name, visible)
    widget_selected: usize,
    // B3 #19: Export report
    report_format: usize, // 0=HTML, 1=Markdown
    report_path: String,
    // B3 #20: Keyboard macro
    macros: Vec<KeyMacro>,
    macro_selected: usize,
    macro_recording: bool,
    macro_buffer: Vec<String>,
    // Similar image search
    similar_groups: Vec<SimilarImageGroup>,
    similar_selected: usize,
    similar_file_selected: usize,
    similar_threshold: u32,  // Hamming distance threshold (0-64)
    similar_scroll: usize,
}

impl App {
    fn new() -> Self {
        let config = Config::load();
        let history = History::load();
        let undo_log = UndoLog::load();
        let state_store = AppStateStore::load();
        let sys_info = System::new_all();
        Self {
            state: AppState::Menu,
            menu_items: MenuItem::all().to_vec(),
            selected: state_store.last_menu_idx,
            config: config.clone(),
            history,
            undo_log,
            theme_idx: state_store.last_theme_idx,
            dry_run: state_store.last_dry_run,
            step_enabled: vec![true; FULL_STEP_LABELS.len()],
            step_selected: 0,
            preview_items: Vec::new(),
            preview_scroll: 0,
            preview_file_count: 0,
            preview_total_size: 0,
            logs: Arc::new(Mutex::new(Vec::new())),
            progress: Arc::new(Mutex::new(0.0)),
            progress_detail: Arc::new(Mutex::new(String::new())),
            current_step: Arc::new(Mutex::new(String::new())),
            is_processing: Arc::new(Mutex::new(false)),
            errors: Arc::new(Mutex::new(Vec::new())),
            step_progress: Arc::new(Mutex::new(vec![0.0; FULL_STEP_LABELS.len()])),
            start_time: Arc::new(Mutex::new(None)),
            files_processed: Arc::new(Mutex::new(0)),
            search_mode: false,
            search_query: String::new(),
            filtered_log_indices: Vec::new(),
            settings_selected: 0,
            batch_queue: Vec::new(),
            batch_selected: 0,
            batch_adding: false,
            batch_input: String::new(),
            spinner_idx: 0,
            frame_count: 0,
            is_paused: Arc::new(Mutex::new(false)),
            checkpoint: Arc::new(Mutex::new(None)),
            duplicate_groups: Vec::new(),
            dup_group_selected: 0,
            dup_file_selected: 0,
            filter: FileFilter::default(),
            sort_config: SortConfig::default(),
            filter_active: false,
            filter_selected: 0,
            stats_scroll: 0,
            profile_selected: 0,
            profile_input: String::new(),
            profile_adding: false,
            info_selected: 0,
            confirm_action: None,
            confirm_yes: true,
            help_scroll: 0,
            state_store,
            sys_info,
            retry_count: 0,
            watch_active: false,
            watch_processed: 0,
            watch_last_scan: Instant::now(),
            // New features init
            size_comparisons: Vec::new(),
            size_compare_scroll: 0,
            error_details: Vec::new(),
            error_scroll: 0,
            presets: ConversionPreset::presets(),
            preset_selected: 0,
            active_preset: 2, // Balance
            compression_stats: Vec::new(),
            compress_scroll: 0,
            classify_rules: Vec::new(),
            classify_selected: 0,
            classify_input: String::new(),
            classify_adding: false,
            meta_files: Vec::new(),
            meta_scroll: 0,
            meta_field: 0,
            scheduler_jobs: Vec::new(),
            scheduler_selected: 0,
            scheduler_editing: false,
            scheduler_field: 0,
            export_format: 0,
            custom_themes: Vec::new(),
            theme_edit_selected: 0,
            theme_edit_field: 0,
            widget_layout: WidgetLayout::default(),
            dashboard_editing: false,
            dashboard_selected: 0,
            statusbar_items: vec![
                ("Memory".into(), true),
                ("CPU".into(), false),
                ("Time".into(), true),
                ("Errors".into(), true),
                ("Retry".into(), false),
                ("Filter".into(), true),
                ("Watch".into(), false),
                ("Workers".into(), false),
            ],
            statusbar_selected: 0,
            use_gpu: false,
            gpu_effort: 7,
            use_mmap: false,
            auto_parallel: true,
            current_workers: 4,
            cpu_threshold: 80.0,
            config_io_selected: 0,
            config_io_path: String::new(),
            config_io_adding: false,
            plugins: Vec::new(),
            plugin_selected: 0,
            plugin_dir: "./plugins".into(),
            // Batch 3 initialization
            image_preview: None,
            preview_image_path: String::new(),
            vim_buffer: String::new(),
            fuzzy_mode: false,
            fuzzy_query: String::new(),
            fuzzy_results: Vec::new(),
            fuzzy_selected: 0,
            watch_dirs: Vec::new(),
            watch_dir_selected: 0,
            watch_dir_adding: false,
            watch_dir_input: String::new(),
            drop_zone_active: false,
            drop_queue: Vec::new(),
            split_mode: false,
            split_left_scroll: 0,
            split_right_scroll: 0,
            breadcrumb: vec!["Menu".into()],
            quick_actions: vec![
                ("Full Process".into(), 0),
                ("Preview".into(), 2),
                ("Statistics".into(), 9),
                ("Settings".into(), 5),
                ("Help".into(), 14),
                ("Theme Editor".into(), 17),
            ],
            quick_selected: 0,
            recent_files: Vec::new(),
            recent_scroll: 0,
            file_tags: Vec::new(),
            tag_selected: 0,
            tag_input: String::new(),
            tag_adding: false,
            diff_left: Vec::new(),
            diff_right: Vec::new(),
            diff_scroll: 0,
            file_tree: Vec::new(),
            tree_selected: 0,
            tree_scroll: 0,
            rename_patterns: Vec::new(),
            rename_selected: 0,
            rename_input: String::new(),
            rename_field: 0,
            timeline_entries: Vec::new(),
            timeline_scroll: 0,
            wizard_step: 0,
            wizard_done: false,
            palette_open: false,
            palette_query: String::new(),
            palette_results: Vec::new(),
            palette_selected: 0,
            notifications: Vec::new(),
            notif_scroll: 0,
            widgets: vec![
                ("Progress Bar".into(), true),
                ("Memory".into(), true),
                ("CPU".into(), true),
                ("File Count".into(), true),
                ("Speed".into(), true),
                ("Errors".into(), true),
            ],
            widget_selected: 0,
            report_format: 0,
            report_path: String::new(),
            macros: Vec::new(),
            macro_selected: 0,
            macro_recording: false,
            macro_buffer: Vec::new(),
            similar_groups: Vec::new(),
            similar_selected: 0,
            similar_file_selected: 0,
            similar_threshold: 10,
            similar_scroll: 0,
        }
    }

    fn theme(&self) -> Theme {
        Theme::from_index(self.theme_idx)
    }

    fn scan_preview(&mut self) {
        self.preview_items.clear();
        self.preview_file_count = 0;
        self.preview_total_size = 0;

        let dest_path = PathBuf::from(&self.config.dest);
        if !dest_path.exists() {
            return;
        }

        let min_size = self.config.min_file_size_kb * 1024;

        if let Ok(entries) = fs::read_dir(&dest_path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() || !self.is_image_file(&path) {
                    continue;
                }
                if let Ok(meta) = fs::metadata(&path) {
                    if meta.len() < min_size {
                        continue;
                    }
                    self.preview_total_size += meta.len();
                    self.preview_file_count += 1;
                }

                let file_stem = path.file_stem().unwrap().to_string_lossy();
                let ext = path.extension().unwrap().to_string_lossy().to_string();

                let new_name = if is_digit_underscore_digit(&file_stem) {
                    Some(format!("{}{}", file_stem.replace("_", ""), ext))
                } else if let Some(cleaned) = remove_trailing_parentheses(&file_stem) {
                    Some(format!("{}{}", cleaned, ext))
                } else {
                    None
                };

                if let Some(new) = new_name {
                    let old = path.file_name().unwrap().to_string_lossy().to_string();
                    self.preview_items.push((old, new));
                }
            }
        }

        // Timestamp rename candidates
        if let Ok(entries) = fs::read_dir(&dest_path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() || !self.is_image_file(&path) {
                    continue;
                }
                let file_name = path.file_name().unwrap().to_string_lossy();
                if file_name.starts_with(|c: char| c.is_numeric()) && file_name.len() > 14 {
                    continue;
                }
                if let Ok(meta) = fs::metadata(&path) {
                    if meta.len() < min_size {
                        continue;
                    }
                    if let Ok(modified) = meta.modified() {
                        let datetime: chrono::DateTime<chrono::Local> = modified.into();
                        let timestamp = datetime.format("%Y%m%d%H%M%S").to_string();
                        let ext = path.extension().unwrap().to_string_lossy().to_string();
                        let new_name = format!("{}{}", timestamp, ext);
                        if new_name != file_name.as_ref() {
                            self.preview_items.push((file_name.to_string(), new_name));
                        }
                    }
                }
            }
        }
    }

    fn is_image_file(&self, path: &PathBuf) -> bool {
        path.extension()
            .and_then(|ext| ext.to_str())
            .map(|ext| {
                self.config.image_extensions.iter().any(|e| {
                    e.eq_ignore_ascii_case(&format!(".{}", ext))
                })
            })
            .unwrap_or(false)
    }

    fn start_processing(&mut self) {
        self.state = AppState::Processing;
        if let Ok(mut logs) = self.logs.lock() { logs.clear(); }
        if let Ok(mut errs) = self.errors.lock() { errs.clear(); }
        if let Ok(mut sp) = self.step_progress.lock() {
            for v in sp.iter_mut() { *v = 0.0; }
        }
        *self.progress.lock().unwrap() = 0.0;
        *self.progress_detail.lock().unwrap() = String::new();
        *self.is_processing.lock().unwrap() = true;
        *self.start_time.lock().unwrap() = Some(Instant::now());
        *self.files_processed.lock().unwrap() = 0;

        let logs = self.logs.clone();
        let progress = self.progress.clone();
        let progress_detail = self.progress_detail.clone();
        let current_step = self.current_step.clone();
        let is_processing = self.is_processing.clone();
        let errors = self.errors.clone();
        let step_progress = self.step_progress.clone();
        let files_processed = self.files_processed.clone();
        let step_enabled = self.step_enabled.clone();
        let config = self.config.clone();
        let dry_run = self.dry_run;

        thread::spawn(move || {
            let log = |msg: String| { if let Ok(mut l) = logs.lock() { l.push(msg); } };
            let set_prog = |v: f64| { if let Ok(mut p) = progress.lock() { *p = v; } };
            let set_detail = |s: String| { if let Ok(mut d) = progress_detail.lock() { *d = s; } };
            let set_step = |s: String| { if let Ok(mut st) = current_step.lock() { *st = s; } };
            let add_error = |e: String| { if let Ok(mut errs) = errors.lock() { errs.push(e); } };
            let set_step_prog = |step: usize, v: f64| {
                if let Ok(mut sp) = step_progress.lock() { sp[step] = v; }
            };
            let inc_files = |n: usize| {
                if let Ok(mut f) = files_processed.lock() { *f += n; }
            };

            run_full_process(
                &config, &step_enabled, dry_run,
                &log, &set_prog, &set_detail, &set_step, &add_error,
                &set_step_prog, &inc_files,
            );

            *is_processing.lock().unwrap() = false;
        });
    }

    fn start_single(&mut self, item: MenuItem) {
        self.state = AppState::Processing;
        if let Ok(mut logs) = self.logs.lock() { logs.clear(); }
        if let Ok(mut errs) = self.errors.lock() { errs.clear(); }
        *self.progress.lock().unwrap() = 0.0;
        *self.progress_detail.lock().unwrap() = String::new();
        *self.is_processing.lock().unwrap() = true;
        *self.start_time.lock().unwrap() = Some(Instant::now());
        *self.files_processed.lock().unwrap() = 0;

        let logs = self.logs.clone();
        let progress = self.progress.clone();
        let progress_detail = self.progress_detail.clone();
        let current_step = self.current_step.clone();
        let is_processing = self.is_processing.clone();
        let errors = self.errors.clone();
        let files_processed = self.files_processed.clone();
        let config = self.config.clone();
        let dry_run = self.dry_run;

        thread::spawn(move || {
            let log = |msg: String| { if let Ok(mut l) = logs.lock() { l.push(msg); } };
            let set_prog = |v: f64| { if let Ok(mut p) = progress.lock() { *p = v; } };
            let set_detail = |s: String| { if let Ok(mut d) = progress_detail.lock() { *d = s; } };
            let set_step = |s: String| { if let Ok(mut st) = current_step.lock() { *st = s; } };
            let add_error = |e: String| { if let Ok(mut errs) = errors.lock() { errs.push(e); } };
            let inc_files = |n: usize| {
                if let Ok(mut f) = files_processed.lock() { *f += n; }
            };

            match item {
                MenuItem::FullProcess => {}
                MenuItem::RenameOnly => {
                    set_step("Renaming files...".into());
                    log("Starting rename only...".into());
                    match run_with_progress(&config.dest, "rename", dry_run, &set_prog, &set_detail, &add_error, &inc_files) {
                        Ok(n) => log(format!("✓ Renamed {} files", n)),
                        Err(e) => log(format!("Error: {}", e)),
                    }
                }
                MenuItem::TimestampRename => {
                    set_step("Timestamp rename...".into());
                    log("Starting timestamp rename...".into());
                    match run_with_progress(&config.dest, "timestamp", dry_run, &set_prog, &set_detail, &add_error, &inc_files) {
                        Ok(n) => log(format!("✓ Renamed {} files", n)),
                        Err(e) => log(format!("Error: {}", e)),
                    }
                }
                MenuItem::ImageToJxl => {
                    set_step("Converting to JXL...".into());
                    log("Starting JXL conversion...".into());
                    match convert_to_jxl(&config.dest) {
                        Ok(()) => log("✓ JXL conversion completed".into()),
                        Err(e) => { log(format!("Error: {}", e)); add_error(e.to_string()); }
                    }
                    set_prog(1.0);
                }
                MenuItem::HashCacheDb => {
                    set_step("Running Hash Cache DB...".into());
                    log("Starting hash_cache_db.exe...".into());
                    hash_cache_db();
                    log("✓ Hash cache DB completed".into());
                    set_prog(1.0);
                }
                MenuItem::Settings => {}
                MenuItem::BatchQueue => {}
                MenuItem::Profiles => {}
                MenuItem::WatchMode => {}
                MenuItem::Statistics => {}
                MenuItem::Duplicates => {}
                MenuItem::JxlSettings => {}
                MenuItem::SizeCompare => {}
                MenuItem::ErrorPanel => {}
                MenuItem::Presets => {}
                MenuItem::Scheduler => {}
                MenuItem::HistoryExport => {}
                MenuItem::ThemeEditor => {}
                MenuItem::CompressionGraph => {}
                MenuItem::FileClassify => {}
                MenuItem::MetaEdit => {}
                MenuItem::ConfigIO => {}
                MenuItem::Plugins => {}
            }
            *is_processing.lock().unwrap() = false;
        });
    }

    fn undo_last(&mut self) {
        if let Some((old_path, new_path)) = self.undo_log.undo_last() {
            let from = PathBuf::from(&new_path);
            let to = PathBuf::from(&old_path);
            if from.exists() {
                match fs::rename(&from, &to) {
                    Ok(()) => {
                        if let Ok(mut logs) = self.logs.lock() {
                            logs.push(format!("✓ Undo: {} → {}", new_path, old_path));
                        }
                    }
                    Err(e) => {
                        if let Ok(mut logs) = self.logs.lock() {
                            logs.push(format!("✗ Undo failed: {}", e));
                        }
                    }
                }
            }
        }
    }

    fn update_log_filter(&mut self) {
        if self.search_query.is_empty() {
            self.filtered_log_indices.clear();
        } else {
            let logs = self.logs.lock().unwrap();
            let query = self.search_query.to_lowercase();
            self.filtered_log_indices = logs.iter().enumerate()
                .filter(|(_, msg)| msg.to_lowercase().contains(&query))
                .map(|(i, _)| i)
                .collect();
        }
    }

    // Feature #3: Scan duplicate groups
    fn scan_duplicates(&mut self) {
        self.duplicate_groups.clear();
        let dest_path = PathBuf::from(&self.config.dest);
        if !dest_path.exists() { return; }

        let files: Vec<_> = fs::read_dir(&dest_path).ok()
            .map(|d| d.flatten().filter(|e| e.path().is_file()).collect::<Vec<_>>())
            .unwrap_or_default();

        let mut hash_map: HashMap<String, Vec<(String, u64)>> = HashMap::new();
        for entry in &files {
            let path = entry.path();
            if let Ok(hash) = calculate_sha256(&path) {
                if let Ok(meta) = fs::metadata(&path) {
                    let name = path.to_string_lossy().to_string();
                    hash_map.entry(hash).or_default().push((name, meta.len()));
                }
            }
        }

        for (hash, files) in hash_map {
            if files.len() > 1 {
                self.duplicate_groups.push(DuplicateGroup {
                    hash: hash[..16].to_string(),
                    files,
                    selected: 0,
                });
            }
        }
        self.duplicate_groups.sort_by(|a, b| b.files.len().cmp(&a.files.len()));
    }

    // Feature #4: Add to batch queue
    fn batch_add_current(&mut self) {
        self.batch_queue.push(BatchJob {
            path: self.config.dest.clone(),
            status: "pending".into(),
            files_processed: 0,
        });
    }

    // Feature #8: Get stats data
    fn get_stats_data(&self) -> Vec<(String, u64)> {
        let mut data = Vec::new();
        for entry in self.history.entries.iter().rev().take(20) {
            data.push((entry.timestamp[5..16].to_string(), entry.files_processed as u64));
        }
        data.reverse();
        data
    }

    // Feature #9: Save profile
    fn save_profile(&mut self, name: String) {
        self.config.profiles.retain(|p| p.name != name);
        self.config.profiles.push(Profile { name, config: self.config.clone() });
        let _ = self.config.save();
    }

    // Feature #9: Load profile
    fn load_profile(&mut self, idx: usize) {
        if idx < self.config.profiles.len() {
            let profile = self.config.profiles[idx].clone();
            self.config = profile.config;
            let _ = self.config.save();
        }
    }

    // Feature #15: Save state
    fn save_state(&self) {
        self.state_store.save();
    }

    // Feature #16: Refresh memory info
    fn refresh_memory(&mut self) {
        self.sys_info.refresh_memory();
    }

    // Feature #17: Retry logic wrapper
    fn retry_operation<F, T>(&mut self, mut op: F) -> Result<T, Box<dyn std::error::Error>>
    where F: FnMut() -> Result<T, Box<dyn std::error::Error>> {
        let max = self.config.max_retries;
        for attempt in 0..=max {
            match op() {
                Ok(v) => return Ok(v),
                Err(e) => {
                    if attempt < max {
                        self.retry_count = attempt + 1;
                        thread::sleep(Duration::from_millis(100 * (attempt as u64 + 1)));
                    } else {
                        return Err(e);
                    }
                }
            }
        }
        Err("Max retries exceeded".into())
    }

    // Feature #20: Watch mode scan
    fn watch_scan(&mut self) {
        if !self.watch_active { return; }
        let now = Instant::now();
        if now.duration_since(self.watch_last_scan).as_secs() < self.config.watch_interval_secs { return; }
        self.watch_last_scan = now;

        for dir in &self.config.watch_dirs.clone() {
            let path = PathBuf::from(dir);
            if !path.exists() { continue; }
            if let Ok(entries) = fs::read_dir(&path) {
                for entry in entries.flatten() {
                    let p = entry.path();
                    if p.is_file() && self.is_image_file(&p) {
                        let dest = PathBuf::from(&self.config.dest).join(p.file_name().unwrap());
                        if !dest.exists() {
                            if fs::rename(&p, &dest).is_ok() {
                                self.watch_processed += 1;
                                if let Ok(mut logs) = self.logs.lock() {
                                    logs.push(format!("Watch: moved {}", p.display()));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    // Feature #11: Get file info
    fn get_file_info(&self, path: &str) -> Vec<(String, String)> {
        let mut info = Vec::new();
        let p = PathBuf::from(path);
        if let Ok(meta) = fs::metadata(&p) {
            info.push(("Size".into(), format_size(meta.len())));
            if let Ok(modified) = meta.modified() {
                let dt: chrono::DateTime<chrono::Local> = modified.into();
                info.push(("Modified".into(), dt.format("%Y-%m-%d %H:%M:%S").to_string()));
            }
            if let Ok(created) = meta.created() {
                let dt: chrono::DateTime<chrono::Local> = created.into();
                info.push(("Created".into(), dt.format("%Y-%m-%d %H:%M:%S").to_string()));
            }
            info.push(("Extension".into(), p.extension().unwrap_or_default().to_string_lossy().to_string()));
            info.push(("Path".into(), p.to_string_lossy().to_string()));
        }
        info
    }

    // Feature #12: Export log
    fn export_log(&self) -> Result<(), Box<dyn std::error::Error>> {
        let logs = self.logs.lock().unwrap();
        let path = format!("io_tool_log_{}.txt", chrono::Local::now().format("%Y%m%d_%H%M%S"));
        let content = logs.join("\n");
        fs::write(&path, content)?;
        Ok(())
    }

    // New #1: Size comparison
    fn build_size_comparisons(&mut self) {
        self.size_comparisons.clear();
        let dest = PathBuf::from(&self.config.dest);
        if !dest.exists() { return; }
        if let Ok(entries) = fs::read_dir(&dest) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() { continue; }
                let name = path.file_name().unwrap().to_string_lossy().to_string();
                if let Ok(meta) = fs::metadata(&path) {
                    let size = meta.len();
                    // Simulate conversion ratio based on extension
                    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
                    let ratio = match ext.to_lowercase().as_str() {
                        "jpg" | "jpeg" => 0.65,
                        "png" => 0.55,
                        "bmp" => 0.20,
                        "gif" => 0.80,
                        "webp" => 0.90,
                        "tiff" | "tif" => 0.30,
                        "heic" | "heif" => 0.70,
                        _ => 0.75,
                    };
                    let converted = (size as f64 * ratio) as u64;
                    let reduction = (1.0 - ratio) * 100.0;
                    self.size_comparisons.push(SizeComparison {
                        filename: name,
                        original_size: size,
                        converted_size: converted,
                        reduction_pct: reduction,
                    });
                }
            }
        }
        self.size_comparisons.sort_by(|a, b| b.reduction_pct.partial_cmp(&a.reduction_pct).unwrap());
    }

    // New #3: Error details collection
    fn collect_error_details(&mut self) {
        self.error_details.clear();
        if let Ok(errs) = self.errors.lock() {
            for (i, err) in errs.iter().enumerate() {
                self.error_details.push(ErrorDetail {
                    filename: format!("file_{}", i),
                    error_msg: err.clone(),
                    timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
                    _step: "Processing".into(),
                });
            }
        }
    }

    // New #6: Build compression stats
    fn build_compression_stats(&mut self) {
        let mut stats: HashMap<String, (u64, u64, usize)> = HashMap::new();
        for comp in &self.size_comparisons {
            let ext = comp.filename.rsplit('.').next().unwrap_or("unknown").to_lowercase();
            let entry = stats.entry(ext).or_insert((0, 0, 0));
            entry.0 += comp.original_size;
            entry.1 += comp.converted_size;
            entry.2 += 1;
        }
        self.compression_stats = stats.into_iter()
            .map(|(format, (orig, conv, count))| CompressionStat {
                format,
                original_size: orig,
                converted_size: conv,
                count,
            })
            .collect();
        self.compression_stats.sort_by(|a, b| b.count.cmp(&a.count));
    }

    // New #7: File classification
    fn classify_files(&mut self) {
        let dest = PathBuf::from(&self.config.dest);
        if !dest.exists() { return; }
        for rule in &self.classify_rules {
            let target = dest.join(&rule.1);
            if !target.exists() { let _ = fs::create_dir_all(&target); }
        }
        if let Ok(entries) = fs::read_dir(&dest) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() { continue; }
                let name = path.file_name().unwrap().to_string_lossy().to_lowercase();
                for rule in &self.classify_rules {
                    if name.contains(&rule.0.to_lowercase()) {
                        let target = dest.join(&rule.1).join(path.file_name().unwrap());
                        let _ = fs::rename(&path, &target);
                        break;
                    }
                }
            }
        }
    }

    // New #8: Metadata operations
    fn load_meta_files(&mut self) {
        self.meta_files.clear();
        let dest = PathBuf::from(&self.config.dest);
        if !dest.exists() { return; }
        if let Ok(entries) = fs::read_dir(&dest) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.is_file() && self.is_image_file(&path) {
                    let name = path.file_name().unwrap().to_string_lossy().to_string();
                    self.meta_files.push((name, false));
                }
            }
        }
    }

    // New #10: Export history
    fn export_history_csv(&self) -> Result<String, Box<dyn std::error::Error>> {
        let mut csv = String::from("timestamp,files_processed,errors,duration_secs\n");
        for entry in &self.history.entries {
            csv.push_str(&format!("{},{},{},{}\n", entry.timestamp, entry.files_processed, entry.errors, entry.duration_secs));
        }
        let path = format!("io_tool_history_{}.csv", chrono::Local::now().format("%Y%m%d_%H%M%S"));
        fs::write(&path, &csv)?;
        Ok(path)
    }

    fn export_history_json(&self) -> Result<String, Box<dyn std::error::Error>> {
        let json = serde_json::to_string_pretty(&self.history)?;
        let path = format!("io_tool_history_{}.json", chrono::Local::now().format("%Y%m%d_%H%M%S"));
        fs::write(&path, &json)?;
        Ok(path)
    }

    // New #11: Theme save/load
    fn save_custom_theme(&mut self, name: &str) {
        let theme = ThemeConfig {
            name: name.to_string(),
            ..Default::default()
        };
        self.custom_themes.push(theme);
        let _ = fs::write(
            ".io_tool_themes.json",
            serde_json::to_string_pretty(&self.custom_themes).unwrap_or_default(),
        );
    }

    fn load_custom_themes(&mut self) {
        if let Ok(data) = fs::read_to_string(".io_tool_themes.json") {
            if let Ok(themes) = serde_json::from_str::<Vec<ThemeConfig>>(&data) {
                self.custom_themes = themes;
            }
        }
    }

    // New #15: Statusbar config save
    fn save_statusbar_config(&self) {
        let config: Vec<(String, bool)> = self.statusbar_items.clone();
        let _ = fs::write(
            ".io_tool_statusbar.json",
            serde_json::to_string_pretty(&config).unwrap_or_default(),
        );
    }

    // New #16: GPU settings
    fn apply_preset(&mut self, idx: usize) {
        if idx < self.presets.len() {
            let preset = self.presets[idx].clone();
            self.config.jxl_quality = preset.quality as u32;
            self.config.jxl_lossless = preset.lossless;
            self.active_preset = idx;
        }
    }

    // New #19: Config import/export
    fn export_config(&self) -> Result<String, Box<dyn std::error::Error>> {
        let json = serde_json::to_string_pretty(&self.config)?;
        let path = format!("io_tool_config_export_{}.json", chrono::Local::now().format("%Y%m%d_%H%M%S"));
        fs::write(&path, &json)?;
        Ok(path)
    }

    fn import_config(&mut self, path: &str) -> Result<(), Box<dyn std::error::Error>> {
        let data = fs::read_to_string(path)?;
        let imported: Config = serde_json::from_str(&data)?;
        self.config = imported;
        self.config.save()?;
        Ok(())
    }

    // New #20: Plugin scanning
    fn scan_plugins(&mut self) {
        self.plugins.clear();
        let dir = PathBuf::from(&self.plugin_dir);
        if !dir.exists() { let _ = fs::create_dir_all(&dir); return; }
        if let Ok(entries) = fs::read_dir(&dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|e| e.to_str()) == Some("json") {
                    if let Ok(data) = fs::read_to_string(&path) {
                        if let Ok(info) = serde_json::from_str::<serde_json::Value>(&data) {
                            let name = info["name"].as_str().unwrap_or("Unknown").to_string();
                            let desc = info["description"].as_str().unwrap_or("").to_string();
                            self.plugins.push(PluginInfo {
                                name,
                                _path: path.to_string_lossy().to_string(),
                                enabled: true,
                                description: desc,
                            });
                        }
                    }
                }
            }
        }
    }

    // New #18: Auto parallelism adjustment
    fn adjust_parallelism(&mut self) {
        if !self.auto_parallel { return; }
        self.sys_info.refresh_all();
        let cpu_usage = if !self.sys_info.cpus().is_empty() {
            self.sys_info.cpus().iter().map(|c| c.cpu_usage() as f64).sum::<f64>() / self.sys_info.cpus().len() as f64
        } else { 0.0 };
        if cpu_usage < self.cpu_threshold * 0.7 && self.current_workers < 16 {
            self.current_workers += 1;
        } else if cpu_usage > self.cpu_threshold && self.current_workers > 1 {
            self.current_workers -= 1;
        }
    }

    // ============================================================
    // Batch 3 methods
    // ============================================================

    // B3 #1: Generate ASCII art preview from image file
    fn generate_image_preview(&mut self, path: &str) {
        let buf = PathBuf::from(path);
        if !buf.exists() { return; }
        let name = buf.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        // Simple ASCII art based on file size and type
        let size = fs::metadata(&buf).map(|m| m.len()).unwrap_or(0);
        let ext = buf.extension().map(|e| e.to_string_lossy().to_string()).unwrap_or_default();
        let lines = vec![
            format!("┌{:─<40}┐", ""),
            format!("│ {:<38} │", name),
            format!("│ {:<38} │", format!("Size: {}", format_size(size))),
            format!("│ {:<38} │", format!("Type: {}", ext.to_uppercase())),
            format!("│ {:<38} │", ""),
            format!("│ {:<38} │", "┌──────────────────────────────┐"),
            format!("│ {:<38} │", "│  ▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄▄  │"),
            format!("│ {:<38} │", "│  █░░░░░░░░░░░░░░░░░░░░░░░█  │"),
            format!("│ {:<38} │", "│  █░░░░ IMAGE PREVIEW ░░░░█  │"),
            format!("│ {:<38} │", "│  █░░░░░░░░░░░░░░░░░░░░░░░█  │"),
            format!("│ {:<38} │", "│  ▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀▀  │"),
            format!("│ {:<38} │", "└──────────────────────────────┘"),
            format!("│ {:<38} │", ""),
            format!("└{:─<40}┘", ""),
        ];
        self.image_preview = Some(ImagePreview {
            ascii_lines: lines,
            width: 42,
            height: 14,
            filename: name,
        });
    }

    // B3 #3: Fuzzy search files
    fn fuzzy_search(&mut self) {
        self.fuzzy_results.clear();
        if self.fuzzy_query.is_empty() { return; }
        let query = self.fuzzy_query.to_lowercase();
        for item in &self.preview_items {
            let name = item.0.to_lowercase();
            if name.contains(&query) {
                self.fuzzy_results.push(item.0.clone());
            }
        }
        self.fuzzy_results.sort_by(|a, b| {
            let a_score = if a.to_lowercase().starts_with(&query) { 0 } else { 1 };
            let b_score = if b.to_lowercase().starts_with(&query) { 0 } else { 1 };
            a_score.cmp(&b_score)
        });
    }

    // B3 #7: Update breadcrumb
    fn update_breadcrumb(&mut self, path: &str) {
        self.breadcrumb = path.split(" > ").map(|s| s.to_string()).collect();
    }

    // B3 #9: Add recent file
    fn add_recent_file(&mut self, path: String, file_type: String, size: u64) {
        self.recent_files.retain(|r| r.path != path);
        self.recent_files.insert(0, RecentFile {
            path: path.clone(),
            processed_at: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            file_type,
            size,
        });
        if self.recent_files.len() > 100 {
            self.recent_files.truncate(100);
        }
    }

    // B3 #10: Add tag to file
    fn add_file_tag(&mut self, pattern: String, tag: String) {
        if let Some(ft) = self.file_tags.iter_mut().find(|f| f.file_pattern == pattern) {
            if !ft.tags.contains(&tag) {
                ft.tags.push(tag);
            }
        } else {
            self.file_tags.push(FileTag {
                file_pattern: pattern,
                tags: vec![tag],
            });
        }
    }

    // B3 #12: Build file tree
    fn build_file_tree(&mut self) {
        self.file_tree.clear();
        let root = PathBuf::from(&self.config.dest);
        if !root.exists() { return; }
        self.file_tree = self.build_tree_recursive(&root, 0, 3);
    }

    fn build_tree_recursive(&self, dir: &PathBuf, depth: usize, max_depth: usize) -> Vec<FileTreeNode> {
        if depth >= max_depth { return Vec::new(); }
        let mut nodes = Vec::new();
        if let Ok(entries) = fs::read_dir(dir) {
            let mut dirs: Vec<_> = entries.flatten()
                .filter(|e| e.path().is_dir())
                .collect();
            dirs.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
            for entry in dirs {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();
                let children = self.build_tree_recursive(&path, depth + 1, max_depth);
                nodes.push(FileTreeNode {
                    name,
                    path: path.to_string_lossy().to_string(),
                    is_dir: true,
                    expanded: depth < 1,
                    depth,
                    children,
                });
            }
            let mut files: Vec<_> = fs::read_dir(dir).ok()
                .map(|d| d.flatten().filter(|e| e.path().is_file()).collect::<Vec<_>>())
                .unwrap_or_default();
            files.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
            for entry in files.into_iter().take(20) {
                let path = entry.path();
                let name = entry.file_name().to_string_lossy().to_string();
                nodes.push(FileTreeNode {
                    name,
                    path: path.to_string_lossy().to_string(),
                    is_dir: false,
                    expanded: false,
                    depth,
                    children: Vec::new(),
                });
            }
        }
        nodes
    }

    // B3 #13: Preview rename pattern
    fn preview_rename_pattern(&mut self) {
        if self.rename_patterns.is_empty() { return; }
        let pat = &mut self.rename_patterns[self.rename_selected];
        pat.preview.clear();
        for item in &self.preview_items {
            let old_name = &item.0;
            let new_name = if pat.use_regex {
                if let Ok(re) = regex_lite::Regex::new(&pat.pattern) {
                    re.replace_all(old_name, pat.replacement.as_str()).to_string()
                } else {
                    old_name.clone()
                }
            } else {
                old_name.replace(&pat.pattern, &pat.replacement)
            };
            if old_name != &new_name {
                pat.preview.push((old_name.clone(), new_name));
            }
        }
    }

    // B3 #14: Build timeline
    fn build_timeline(&mut self) {
        self.timeline_entries.clear();
        for entry in self.history.entries.iter().rev().take(50) {
            self.timeline_entries.push(TimelineEntry {
                timestamp: entry.timestamp.clone(),
                event_type: "complete".into(),
                description: format!("Processed {} files from {}", entry.files_processed, entry.source),
                progress: 1.0,
            });
        }
    }

    // B3 #16: Command palette search
    fn palette_search(&mut self) {
        self.palette_results.clear();
        let query = self.palette_query.to_lowercase();
        let all_items = MenuItem::all();
        for (i, item) in all_items.iter().enumerate() {
            let label = item.label().to_lowercase();
            let desc = item.description().to_lowercase();
            if label.contains(&query) || desc.contains(&query) {
                self.palette_results.push((item.label().to_string(), i));
            }
        }
    }

    // B3 #17: Add notification
    fn add_notification(&mut self, message: String, level: String) {
        self.notifications.insert(0, Notification {
            message,
            timestamp: chrono::Local::now().format("%H:%M:%S").to_string(),
            level,
            read: false,
        });
        if self.notifications.len() > 200 {
            self.notifications.truncate(200);
        }
    }

    // B3 #19: Export report
    fn export_report(&self) -> Result<String, Box<dyn std::error::Error>> {
        let format = if self.report_format == 0 { "html" } else { "md" };
        let mut report = String::new();

        if format == "html" {
            report.push_str("<!DOCTYPE html><html><head><title>io-tool Report</title>");
            report.push_str("<style>body{font-family:monospace;background:#1a1a2e;color:#eee;padding:20px}");
            report.push_str("table{border-collapse:collapse;width:100%}td,th{border:1px solid #444;padding:8px}");
            report.push_str("th{background:#16213e}tr:nth-child(even){background:#0f3460}</style></head><body>");
            report.push_str("<h1>io-tool Processing Report</h1>");
        } else {
            report.push_str("# io-tool Processing Report\n\n");
        }

        let total_files: usize = self.history.entries.iter().map(|e| e.files_processed).sum();
        let total_orig: u64 = self.history.entries.iter().map(|e| e.original_size).sum();
        let total_comp: u64 = self.history.entries.iter().map(|e| e.compressed_size).sum();

        if format == "html" {
            report.push_str(&format!("<p>Total files: {} | Original: {} | Compressed: {}</p>",
                total_files, format_size(total_orig), format_size(total_comp)));
            report.push_str("<table><tr><th>Date</th><th>Source</th><th>Files</th><th>Original</th><th>Compressed</th></tr>");
            for entry in &self.history.entries {
                report.push_str(&format!("<tr><td>{}</td><td>{}</td><td>{}</td><td>{}</td><td>{}</td></tr>",
                    entry.timestamp, entry.source, entry.files_processed,
                    format_size(entry.original_size), format_size(entry.compressed_size)));
            }
            report.push_str("</table></body></html>");
        } else {
            report.push_str(&format!("Total files: {} | Original: {} | Compressed: {}\n\n",
                total_files, format_size(total_orig), format_size(total_comp)));
            report.push_str("| Date | Source | Files | Original | Compressed |\n");
            report.push_str("|------|--------|-------|----------|------------|\n");
            for entry in &self.history.entries {
                report.push_str(&format!("| {} | {} | {} | {} | {} |\n",
                    entry.timestamp, entry.source, entry.files_processed,
                    format_size(entry.original_size), format_size(entry.compressed_size)));
            }
        }

        let ext = if format == "html" { "html" } else { "md" };
        let path = format!("report.{}", ext);
        fs::write(&path, &report)?;
        Ok(path)
    }

    // B3 #20: Toggle macro recording
    fn toggle_macro_recording(&mut self) {
        if self.macro_recording {
            self.macro_recording = false;
            if !self.macro_buffer.is_empty() {
                self.macros.push(KeyMacro {
                    name: format!("Macro {}", self.macros.len() + 1),
                    keys: self.macro_buffer.clone(),
                    recording: false,
                });
                self.macro_buffer.clear();
            }
        } else {
            self.macro_recording = true;
            self.macro_buffer.clear();
        }
    }

    // B3 #18: Toggle widget visibility
    fn toggle_widget(&mut self, idx: usize) {
        if idx < self.widgets.len() {
            self.widgets[idx].1 = !self.widgets[idx].1;
        }
    }

    // Similar image search using perceptual hashing
    fn scan_similar_images(&mut self) {
        self.similar_groups.clear();
        let dest_path = PathBuf::from(&self.config.dest);
        if !dest_path.exists() { return; }

        let files: Vec<_> = fs::read_dir(&dest_path).ok()
            .map(|d| d.flatten()
                .filter(|e| {
                    let p = e.path();
                    p.is_file() && is_image_file(&p)
                })
                .collect::<Vec<_>>())
            .unwrap_or_default();

        let total = files.len();
        if total == 0 { return; }

        // Calculate hashes for all images
        let mut hashes: Vec<(String, u64, u64)> = Vec::new(); // (path, ahash, dhash)
        for entry in &files {
            let path = entry.path();
            let name = path.to_string_lossy().to_string();
            let size = fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
            let ahash = calculate_ahash(&path).unwrap_or(0);
            let dhash = calculate_dhash(&path).unwrap_or(0);
            hashes.push((name, ahash, dhash));
        }

        // Group by similarity using aHash
        let mut used = vec![false; hashes.len()];
        for i in 0..hashes.len() {
            if used[i] { continue; }
            let mut group_files = vec![(
                hashes[i].0.clone(),
                fs::metadata(&hashes[i].0).map(|m| m.len()).unwrap_or(0),
            )];
            used[i] = true;

            for j in (i + 1)..hashes.len() {
                if used[j] { continue; }
                let dist = hamming_distance(hashes[i].1, hashes[j].1);
                if dist <= self.similar_threshold {
                    group_files.push((
                        hashes[j].0.clone(),
                        fs::metadata(&hashes[j].0).map(|m| m.len()).unwrap_or(0),
                    ));
                    used[j] = true;
                }
            }

            if group_files.len() > 1 {
                self.similar_groups.push(SimilarImageGroup {
                    hash: hashes[i].1,
                    files: group_files,
                    hash_type: "aHash".into(),
                });
            }
        }

        // Also group by dHash for remaining ungrouped files
        used = vec![false; hashes.len()];
        for i in 0..hashes.len() {
            if used[i] { continue; }
            let mut group_files = vec![(
                hashes[i].0.clone(),
                fs::metadata(&hashes[i].0).map(|m| m.len()).unwrap_or(0),
            )];
            used[i] = true;

            for j in (i + 1)..hashes.len() {
                if used[j] { continue; }
                let dist = hamming_distance(hashes[i].2, hashes[j].2);
                if dist <= self.similar_threshold {
                    // Check if already in aHash group
                    let already_grouped = self.similar_groups.iter().any(|g| {
                        g.files.iter().any(|(p, _)| p == &hashes[j].0)
                            && g.files.iter().any(|(p, _)| p == &hashes[i].0)
                    });
                    if !already_grouped {
                        group_files.push((
                            hashes[j].0.clone(),
                            fs::metadata(&hashes[j].0).map(|m| m.len()).unwrap_or(0),
                        ));
                        used[j] = true;
                    }
                }
            }

            if group_files.len() > 1 {
                self.similar_groups.push(SimilarImageGroup {
                    hash: hashes[i].2,
                    files: group_files,
                    hash_type: "dHash".into(),
                });
            }
        }

        self.similar_groups.sort_by(|a, b| b.files.len().cmp(&a.files.len()));
    }
}

// ============================================================
// Full Process
// ============================================================

fn run_full_process(
    config: &Config,
    step_enabled: &[bool],
    dry_run: bool,
    log: &dyn Fn(String),
    set_prog: &dyn Fn(f64),
    set_detail: &dyn Fn(String),
    set_step: &dyn Fn(String),
    add_error: &dyn Fn(String),
    set_step_prog: &dyn Fn(usize, f64),
    inc_files: &dyn Fn(usize),
) {
    if dry_run {
        log("=== DRY RUN MODE — No files will be modified ===".into());
    } else {
        log("=== FULL PROCESS START ===".into());
    }

    let total_steps = step_enabled.iter().filter(|&&e| e).count();
    let mut step_num = 0;
    let mut total_processed = 0usize;
    let mut total_removed = 0usize;

    // STEP 1: Move files
    if step_enabled[0] {
        step_num += 1;
        set_step(format!("STEP {}/{}: Moving files...", step_num, total_steps));
        set_prog(0.0);
        set_step_prog(0, 0.0);
        set_detail("Scanning sources...".into());
        log("[STEP 1] Moving files from Twitter & Downloads...".into());

        if !dry_run {
            if let Err(e) = fs::create_dir_all(&config.dest) {
                log(format!("  Error creating destination: {}", e));
                add_error(format!("Step 1: {}", e));
                return;
            }
        }

        let mut sources_moved = 0usize;

        // Twitter source
        let twitter_path = PathBuf::from(&config.twitter_src);
        if twitter_path.exists() {
            let files: Vec<_> = fs::read_dir(&twitter_path).ok()
                .map(|d| d.flatten().filter(|e| e.path().is_file()).collect())
                .unwrap_or_default();
            let total = files.len();
            for (i, entry) in files.iter().enumerate() {
                set_detail(format!("Twitter: {}/{}", i + 1, total));
                set_step_prog(0, (i + 1) as f64 / total as f64);
                if !dry_run {
                    let dest_path = PathBuf::from(&config.dest).join(entry.file_name());
                    if fs::rename(entry.path(), &dest_path).is_ok() {
                        sources_moved += 1;
                    }
                } else {
                    sources_moved += 1;
                }
            }
            log(format!("  Twitter: {} files {}", sources_moved, if dry_run { "would move" } else { "moved" }));
        } else {
            log(format!("  Twitter source not found: {}", config.twitter_src));
        }

        // Downloads source
        let dl_path = PathBuf::from(&config.download_src);
        if dl_path.exists() {
            let cutoff = Utc::now().timestamp() - (config.days_to_check * 24 * 60 * 60);
            let files: Vec<_> = fs::read_dir(&dl_path).ok()
                .map(|d| d.flatten().filter(|e| {
                    let p = e.path();
                    p.is_file() && {
                        let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_string();
                        config.image_extensions.iter().any(|ie| ie.eq_ignore_ascii_case(&format!(".{}", ext)))
                    }
                }).collect::<Vec<_>>())
                .unwrap_or_default();
            let total = files.len();
            let mut dl_moved = 0usize;
            for (i, entry) in files.iter().enumerate() {
                set_detail(format!("Downloads: {}/{}", i + 1, total));
                set_step_prog(0, (i + 1) as f64 / total as f64);
                if let Ok(meta) = fs::metadata(entry.path()) {
                    if let Ok(modified) = meta.modified() {
                        let dur = modified.elapsed().unwrap_or_default();
                        let ts = Utc::now().timestamp() - dur.as_secs() as i64;
                        if ts >= cutoff {
                            if !dry_run {
                                let dest_path = PathBuf::from(&config.dest).join(entry.file_name());
                                if fs::rename(entry.path(), &dest_path).is_ok() {
                                    dl_moved += 1;
                                }
                            } else {
                                dl_moved += 1;
                            }
                        }
                    }
                }
            }
            log(format!("  Downloads: {} files {}", dl_moved, if dry_run { "would move" } else { "moved" }));
            sources_moved += dl_moved;
        }

        // X_Images subfolder
        let x_path = dl_path.join("X_Images");
        if x_path.exists() {
            let files: Vec<_> = fs::read_dir(&x_path).ok()
                .map(|d| d.flatten().filter(|e| e.path().is_file()).collect())
                .unwrap_or_default();
            let mut x_moved = 0usize;
            for entry in &files {
                if !dry_run {
                    let dest_path = PathBuf::from(&config.dest).join(entry.file_name());
                    if fs::rename(entry.path(), &dest_path).is_ok() {
                        x_moved += 1;
                    }
                } else {
                    x_moved += 1;
                }
            }
            if !dry_run { let _ = fs::remove_dir_all(&x_path); }
            log(format!("  X_Images: {} files {}", x_moved, if dry_run { "would move" } else { "moved" }));
            sources_moved += x_moved;
        }

        total_processed += sources_moved;
        inc_files(sources_moved);
        set_step_prog(0, 1.0);
        log(format!("  ✓ Step 1 complete: {} files {}", sources_moved, if dry_run { "would move" } else { "moved" }));
    }

    // STEP 2: Remove duplicates
    if step_enabled[1] {
        step_num += 1;
        set_step(format!("STEP {}/{}: Removing duplicates...", step_num, total_steps));
        set_prog(0.2);
        set_step_prog(1, 0.0);
        log("[STEP 2] Removing duplicates (SHA256)...".into());

        let files: Vec<_> = fs::read_dir(&config.dest)
            .ok()
            .map(|d| d.flatten().filter(|e| e.path().is_file()).collect())
            .unwrap_or_default();
        let total = files.len();

        let mut seen_hashes = HashSet::new();
        let mut removed = 0;

        for (i, entry) in files.iter().enumerate() {
            let path = entry.path();
            set_detail(format!("{}/{} files scanned", i + 1, total));
            set_step_prog(1, (i + 1) as f64 / total as f64);
            match calculate_sha256(&path) {
                Ok(hash) => {
                    if !seen_hashes.insert(hash) {
                        if !dry_run {
                            if let Err(e) = fs::remove_file(&path) {
                                add_error(format!("Step 2 remove {}: {}", path.display(), e));
                            } else {
                                removed += 1;
                            }
                        } else {
                            removed += 1;
                        }
                    }
                }
                Err(e) => { add_error(format!("Step 2 hash {}: {}", path.display(), e)); }
            }
        }
        total_removed += removed;
        set_step_prog(1, 1.0);
        log(format!("  ✓ Removed {} duplicates {}", removed, if dry_run { "(dry run)" } else { "" }));
    }

    // STEP 3: Remove files in REF
    if step_enabled[2] {
        step_num += 1;
        set_step(format!("STEP {}/{}: Removing reference duplicates...", step_num, total_steps));
        set_prog(0.4);
        set_step_prog(2, 0.0);
        log("[STEP 3] Removing files that exist in reference...".into());

        let ref_path = PathBuf::from(&config.reference);
        let mut ref_hashes = HashSet::new();

        if ref_path.exists() {
            let ref_files: Vec<_> = WalkDir::new(&ref_path)
                .into_iter()
                .filter_map(|e| e.ok())
                .filter(|e| e.path().is_file())
                .collect();
            let ref_total = ref_files.len();

            for (i, entry) in ref_files.iter().enumerate() {
                set_detail(format!("Building ref DB: {}/{}", i + 1, ref_total));
                set_step_prog(2, (i + 1) as f64 / ref_total as f64 * 0.5);
                if let Ok(hash) = calculate_sha256(&entry.path().to_path_buf()) {
                    ref_hashes.insert(hash);
                }
            }
        }

        let dest_files: Vec<_> = fs::read_dir(&config.dest)
            .ok()
            .map(|d| d.flatten().filter(|e| e.path().is_file()).collect())
            .unwrap_or_default();
        let dest_total = dest_files.len();
        let mut removed = 0;

        for (i, entry) in dest_files.iter().enumerate() {
            let path = entry.path();
            set_detail(format!("Checking: {}/{}", i + 1, dest_total));
            set_step_prog(2, 0.5 + (i + 1) as f64 / dest_total as f64 * 0.5);
            if let Ok(hash) = calculate_sha256(&path) {
                if ref_hashes.contains(&hash) {
                    if !dry_run {
                        if let Err(e) = fs::remove_file(&path) {
                            add_error(format!("Step 3 remove {}: {}", path.display(), e));
                        } else {
                            removed += 1;
                        }
                    } else {
                        removed += 1;
                    }
                }
            }
        }
        total_removed += removed;
        set_step_prog(2, 1.0);
        log(format!("  ✓ Removed {} reference duplicates {}", removed, if dry_run { "(dry run)" } else { "" }));
    }

    // STEP 4: Rename + clean
    if step_enabled[3] {
        step_num += 1;
        set_step(format!("STEP {}/{}: Renaming files...", step_num, total_steps));
        set_prog(0.6);
        set_step_prog(3, 0.0);
        log("[STEP 4] Renaming files...".into());

        let files: Vec<_> = fs::read_dir(&config.dest)
            .ok()
            .map(|d| d.flatten().filter(|e| {
                let p = e.path();
                p.is_file() && {
                    let ext = p.extension().and_then(|e| e.to_str()).unwrap_or("").to_string();
                    config.image_extensions.iter().any(|ie| ie.eq_ignore_ascii_case(&format!(".{}", ext)))
                }
            }).collect())
            .unwrap_or_default();
        let total = files.len();
        let mut renamed = 0;

        for (i, entry) in files.iter().enumerate() {
            let path = entry.path();
            set_detail(format!("{}/{} files", i + 1, total));
            set_step_prog(3, (i + 1) as f64 / total as f64 * 0.7);

            let file_name = path.file_name().unwrap().to_string_lossy();
            if file_name.starts_with(|c: char| c.is_numeric()) && file_name.len() > 14 {
                continue;
            }

            let ext = path.extension().unwrap().to_string_lossy().to_string();
            if let Ok(meta) = fs::metadata(&path) {
                if let Ok(modified) = meta.modified() {
                    let datetime: chrono::DateTime<chrono::Local> = modified.into();
                    let timestamp = datetime.format("%Y%m%d%H%M%S").to_string();
                    let new_name = format!("{}{}", timestamp, ext);
                    if let Ok(final_name) = get_unique_filename(&path, &new_name) {
                        let new_path = path.parent().unwrap().join(&final_name);
                        if !dry_run {
                            if fs::rename(&path, &new_path).is_ok() {
                                renamed += 1;
                            }
                        } else {
                            renamed += 1;
                        }
                    }
                }
            }
        }
        total_processed += renamed;
        log(format!("  ✓ Renamed {} files by timestamp {}", renamed, if dry_run { "(dry run)" } else { "" }));

        // Clean filenames
        set_detail("Cleaning filenames...".into());
        set_step_prog(3, 0.7);
        let clean_count = clean_filenames(&config.dest, dry_run, add_error);
        total_processed += clean_count;
        set_step_prog(3, 1.0);
        log(format!("  ✓ Cleaned {} filenames {}", clean_count, if dry_run { "(dry run)" } else { "" }));
    }

    // STEP 5: Convert to JXL
    if step_enabled[4] {
        step_num += 1;
        set_step(format!("STEP {}/{}: Converting to JXL...", step_num, total_steps));
        set_prog(0.8);
        set_step_prog(4, 0.0);
        set_detail("Running JXL conversion...".into());
        log("[STEP 5] Converting images to JXL...".into());
        if !dry_run {
            match convert_to_jxl(&config.dest) {
                Ok(()) => log("  ✓ JXL conversion completed".into()),
                Err(e) => { log(format!("  Error: {}", e)); add_error(format!("Step 5: {}", e)); }
            }
        } else {
            log("  ✓ JXL conversion skipped (dry run)".into());
        }
        set_step_prog(4, 1.0);
    }

    set_prog(1.0);
    set_detail("Done!".into());
    set_step("All steps completed!".into());
    log(format!("=== {}COMPLETED (processed: {}, removed: {}) ===",
        if dry_run { "DRY RUN " } else { "" }, total_processed, total_removed));
}

fn clean_filenames(dest: &str, dry_run: bool, add_error: &dyn Fn(String)) -> usize {
    let entries: Vec<_> = fs::read_dir(dest)
        .ok()
        .map(|d| d.flatten().filter(|e| e.path().is_file()).collect())
        .unwrap_or_default();
    let mut count = 0;

    for entry in entries {
        let path = entry.path();
        let file_stem = path.file_stem().unwrap().to_string_lossy();
        let ext = path.extension().unwrap().to_string_lossy().to_string();

        let new_name = if is_digit_underscore_digit(&file_stem) {
            format!("{}{}", file_stem.replace("_", ""), ext)
        } else if let Some(cleaned) = remove_trailing_parentheses(&file_stem) {
            format!("{}{}", cleaned, ext)
        } else {
            continue;
        };

        if let Ok(final_name) = get_unique_filename(&path, &new_name) {
            let new_path = path.parent().unwrap().join(&final_name);
            if !dry_run {
                match fs::rename(&path, &new_path) {
                    Ok(()) => count += 1,
                    Err(e) => add_error(format!("{}: {}", path.display(), e)),
                }
            } else {
                count += 1;
            }
        }
    }
    count
}

fn run_with_progress(
    dest: &str,
    mode: &str,
    dry_run: bool,
    set_prog: &dyn Fn(f64),
    set_detail: &dyn Fn(String),
    add_error: &dyn Fn(String),
    inc_files: &dyn Fn(usize),
) -> Result<usize, Box<dyn std::error::Error>> {
    let files: Vec<_> = fs::read_dir(dest)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file())
        .collect();
    let total = files.len();
    let mut count = 0;

    for (i, entry) in files.iter().enumerate() {
        let path = entry.path();
        set_prog((i + 1) as f64 / total as f64);
        set_detail(format!("{}/{} files", i + 1, total));

        let file_stem = path.file_stem().unwrap().to_string_lossy();
        let ext = path.extension().unwrap().to_string_lossy().to_string();

        let new_name = match mode {
            "rename" => {
                if is_digit_underscore_digit(&file_stem) {
                    Some(format!("{}{}", file_stem.replace("_", ""), ext))
                } else if let Some(cleaned) = remove_trailing_parentheses(&file_stem) {
                    Some(format!("{}{}", cleaned, ext))
                } else {
                    None
                }
            }
            "timestamp" => {
                let file_name = path.file_name().unwrap().to_string_lossy();
                if file_name.starts_with(|c: char| c.is_numeric()) && file_name.len() > 14 {
                    None
                } else {
                    if let Ok(meta) = fs::metadata(&path) {
                        if let Ok(modified) = meta.modified() {
                            let datetime: chrono::DateTime<chrono::Local> = modified.into();
                            let timestamp = datetime.format("%Y%m%d%H%M%S").to_string();
                            Some(format!("{}{}", timestamp, ext))
                        } else { None }
                    } else { None }
                }
            }
            _ => None,
        };

        if let Some(new) = new_name {
            if let Ok(final_name) = get_unique_filename(&path, &new) {
                let new_path = path.parent().unwrap().join(&final_name);
                if !dry_run {
                    match fs::rename(&path, &new_path) {
                        Ok(()) => { count += 1; inc_files(1); }
                        Err(e) => add_error(format!("{}: {}", path.display(), e)),
                    }
                } else {
                    count += 1;
                }
            }
        }
    }

    Ok(count)
}

// ============================================================
// CLI Menu
// ============================================================

fn show_cli_menu() {
    let config = Config::load();
    println!("\n============================================");
    println!("  File Processing Tool");
    println!("============================================\n");
    println!("1. Full process (Move → Rename → Encode)");
    println!("2. Rename only (remove _ and parentheses)");
    println!("3. Timestamp rename (select folder)");
    println!("4. Image to JXL conversion (lossless)");
    println!("5. Hash cache database");
    println!();

    print!("Select an option (1-5): ");
    io::stdout().flush().unwrap();

    let mut input = String::new();
    io::stdin().read_line(&mut input).unwrap();
    let choice: u32 = input.trim().parse().unwrap_or(0);

    let log = |msg: String| println!("{}", msg);
    let set_prog = |v: f64| println!("  Progress: {:.0}%", v * 100.0);
    let set_detail = |s: String| println!("    {}", s);
    let set_step = |s: String| println!("  >> {}", s);
    let add_error = |e: String| eprintln!("  ERROR: {}", e);
    let set_step_prog = |_: usize, _: f64| {};
    let inc_files = |_: usize| {};
    let step_enabled = vec![true; 5];

    match choice {
        1 => run_full_process(&config, &step_enabled, false, &log, &set_prog, &set_detail, &set_step, &add_error, &set_step_prog, &inc_files),
        2 => { let _ = rename_remove_underscore_parens(&config.dest); }
        3 => { let _ = rename_by_timestamp(&config.dest); }
        4 => { let _ = convert_to_jxl(&config.dest); }
        5 => hash_cache_db(),
        _ => println!("Invalid option"),
    }
}

// ============================================================
// Main
// ============================================================

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();

    if args.len() > 1 && args[1] == "menu" {
        show_cli_menu();
        return Ok(());
    }

    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    let backend = CrosstermBackend::new(stdout);
    let mut terminal = Terminal::new(backend)?;

    let mut app = App::new();
    let result = run_app(&mut terminal, &mut app);

    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;

    if let Err(err) = result {
        eprintln!("Error: {}", err);
    }

    Ok(())
}

// ============================================================
// TUI Event Loop
// ============================================================

fn run_app(
    terminal: &mut Terminal<CrosstermBackend<io::Stdout>>,
    app: &mut App,
) -> io::Result<()> {
    loop {
        terminal.draw(|f| ui(f, app))?;

        app.frame_count += 1;
        if app.frame_count % 5 == 0 {
            app.spinner_idx = (app.spinner_idx + 1) % SPINNER_CHARS.len();
        }

        // Feature #16: Refresh memory every 60 frames
        if app.frame_count % 60 == 0 {
            app.refresh_memory();
        }

        // Feature #20: Watch mode auto-scan
        if app.watch_active && app.state != AppState::Processing {
            app.watch_scan();
        }

        if event::poll(std::time::Duration::from_millis(50))? {
            if let Event::Key(key) = event::read()? {
                if key.kind != KeyEventKind::Press {
                    continue;
                }

                // Feature #13: Confirm dialog takes priority
                if app.state == AppState::ConfirmDialog {
                    match key.code {
                        KeyCode::Char('y') | KeyCode::Enter => {
                            let action = app.confirm_action.clone();
                            app.state = AppState::Menu;
                            app.confirm_action = None;
                            match action {
                                Some(ConfirmAction::StartProcessing) => {
                                    app.scan_preview();
                                    app.preview_scroll = 0;
                                    app.state = AppState::Preview;
                                }
                                Some(ConfirmAction::ClearHistory) => {
                                    app.history = History::default();
                                    let _ = app.history.save();
                                }
                                Some(ConfirmAction::ClearUndo) => {
                                    app.undo_log = UndoLog::default();
                                    let _ = app.undo_log.save();
                                }
                                None => {}
                            }
                        }
                        KeyCode::Char('n') | KeyCode::Esc => {
                            app.state = AppState::Menu;
                            app.confirm_action = None;
                        }
                        KeyCode::Up | KeyCode::Down => {
                            app.confirm_yes = !app.confirm_yes;
                        }
                        _ => {}
                    }
                    continue;
                }

                // Feature #14: Help screen
                if app.state == AppState::Help {
                    match key.code {
                        KeyCode::Esc | KeyCode::Char('?') | KeyCode::Char('q') => app.state = AppState::Menu,
                        KeyCode::Up | KeyCode::Char('k') => app.help_scroll = app.help_scroll.saturating_sub(1),
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.help_scroll < 30 { app.help_scroll += 1; }
                        }
                        KeyCode::PageUp => app.help_scroll = app.help_scroll.saturating_sub(10),
                        KeyCode::PageDown => app.help_scroll = (app.help_scroll + 10).min(30),
                        _ => {}
                    }
                    continue;
                }

                // Global keys
                if !app.search_mode {
                    match key.code {
                        KeyCode::Char('t') => {
                            app.theme_idx = (app.theme_idx + 1) % THEME_NAMES.len();
                            continue;
                        }
                        KeyCode::Char('d') => {
                            app.dry_run = !app.dry_run;
                            continue;
                        }
                        KeyCode::Char('u') => {
                            if app.state == AppState::Done || app.state == AppState::Menu {
                                app.undo_last();
                            }
                            continue;
                        }
                        KeyCode::Char('?') => {
                            app.help_scroll = 0;
                            app.state = AppState::Help;
                            continue;
                        }
                        KeyCode::Char('f') if app.state == AppState::Menu => {
                            app.filter_selected = 0;
                            app.state = AppState::FilterSort;
                            continue;
                        }
                        KeyCode::Char('s') if app.state == AppState::Menu => {
                            // Cycle sort field
                            app.sort_config = match app.sort_config.field {
                                SortField::Name => SortConfig { field: SortField::Size, ascending: true },
                                SortField::Size => SortConfig { field: SortField::Date, ascending: true },
                                SortField::Date => SortConfig { field: SortField::Type, ascending: true },
                                SortField::Type => SortConfig { field: SortField::Name, ascending: true },
                            };
                            if let Ok(mut logs) = app.logs.lock() {
                                logs.push(format!("Sort: {:?} ({})", app.sort_config.field, if app.sort_config.ascending { "asc" } else { "desc" }));
                            }
                            continue;
                        }
                        KeyCode::Char('S') if app.state == AppState::Menu => {
                            app.stats_scroll = 0;
                            app.state = AppState::Stats;
                            continue;
                        }
                        KeyCode::Char('w') if app.state == AppState::Menu => {
                            app.state = AppState::WatchMode;
                            continue;
                        }
                        KeyCode::Char('p') if app.state == AppState::Menu => {
                            app.profile_selected = 0;
                            app.state = AppState::Profiles;
                            continue;
                        }
                        KeyCode::Char('b') if app.state == AppState::Menu => {
                            app.batch_selected = 0;
                            app.state = AppState::BatchQueue;
                            continue;
                        }
                        KeyCode::Char('i') if app.state == AppState::Preview => {
                            app.info_selected = 0;
                            app.state = AppState::InfoPanel;
                            continue;
                        }
                        KeyCode::Char('p') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            let mut paused = app.is_paused.lock().unwrap();
                            *paused = !*paused;
                            continue;
                        }
                        KeyCode::Char('e') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                            if let Err(e) = app.export_log() {
                                if let Ok(mut logs) = app.logs.lock() {
                                    logs.push(format!("Export failed: {}", e));
                                }
                            } else {
                                if let Ok(mut logs) = app.logs.lock() {
                                    logs.push("Log exported successfully".into());
                                }
                            }
                            continue;
                        }
                        _ => {}
                    }
                }

                match app.state {
                    AppState::Menu => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            app.save_state();
                            return Ok(());
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.selected > 0 { app.selected -= 1; }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.selected < app.menu_items.len() - 1 { app.selected += 1; }
                        }
                        KeyCode::Enter | KeyCode::Char(' ') => {
                            let item = app.menu_items[app.selected];
                            match item {
                                MenuItem::FullProcess => app.state = AppState::StepSelect,
                                MenuItem::Settings => app.state = AppState::Settings,
                                MenuItem::BatchQueue => { app.batch_selected = 0; app.state = AppState::BatchQueue; }
                                MenuItem::Profiles => { app.profile_selected = 0; app.state = AppState::Profiles; }
                                MenuItem::WatchMode => app.state = AppState::WatchMode,
                                MenuItem::Statistics => { app.stats_scroll = 0; app.state = AppState::Stats; }
                                MenuItem::Duplicates => {
                                    app.scan_duplicates();
                                    app.dup_group_selected = 0;
                                    app.state = AppState::DuplicateGroups;
                                }
                                MenuItem::JxlSettings => app.state = AppState::JxlSettings,
                                MenuItem::SizeCompare => {
                                    app.build_size_comparisons();
                                    app.state = AppState::SizeCompare;
                                }
                                MenuItem::ErrorPanel => {
                                    app.collect_error_details();
                                    app.state = AppState::ErrorPanel;
                                }
                                MenuItem::Presets => app.state = AppState::Presets,
                                MenuItem::Scheduler => app.state = AppState::Scheduler,
                                MenuItem::HistoryExport => app.state = AppState::HistoryExport,
                                MenuItem::ThemeEditor => {
                                    app.load_custom_themes();
                                    app.state = AppState::ThemeEditor;
                                }
                                MenuItem::CompressionGraph => {
                                    app.build_compression_stats();
                                    app.state = AppState::CompressionGraph;
                                }
                                MenuItem::FileClassify => app.state = AppState::FileClassify,
                                MenuItem::MetaEdit => {
                                    app.load_meta_files();
                                    app.state = AppState::MetaEdit;
                                }
                                MenuItem::ConfigIO => app.state = AppState::ConfigIO,
                                MenuItem::Plugins => {
                                    app.scan_plugins();
                                    app.state = AppState::Plugins;
                                }
                                // Batch 3 dispatch
                                MenuItem::ImagePreview => {
                                    app.generate_image_preview(&app.config.dest);
                                    app.state = AppState::ImagePreview;
                                }
                                MenuItem::FuzzyFinder => {
                                    app.fuzzy_mode = true;
                                    app.fuzzy_query.clear();
                                    app.fuzzy_results.clear();
                                }
                                MenuItem::SplitPane => {
                                    app.split_mode = true;
                                    app.state = AppState::SplitPane;
                                }
                                MenuItem::QuickActions => {
                                    app.quick_selected = 0;
                                    app.state = AppState::QuickActions;
                                }
                                MenuItem::RecentFiles => {
                                    app.recent_scroll = 0;
                                    app.state = AppState::RecentFiles;
                                }
                                MenuItem::TagSystem => {
                                    app.tag_selected = 0;
                                    app.state = AppState::TagSystem;
                                }
                                MenuItem::SideBySideDiff => {
                                    app.build_size_comparisons();
                                    app.diff_scroll = 0;
                                    app.state = AppState::SideBySideDiff;
                                }
                                MenuItem::FileTreeView => {
                                    app.build_file_tree();
                                    app.tree_selected = 0;
                                    app.state = AppState::FileTreeView;
                                }
                                MenuItem::RenamePattern => {
                                    app.rename_selected = 0;
                                    app.state = AppState::RenamePattern;
                                }
                                MenuItem::Timeline => {
                                    app.build_timeline();
                                    app.timeline_scroll = 0;
                                    app.state = AppState::Timeline;
                                }
                                MenuItem::CommandPalette => {
                                    app.palette_open = true;
                                    app.palette_query.clear();
                                    app.palette_results.clear();
                                }
                                MenuItem::NotificationCenter => {
                                    app.notif_scroll = 0;
                                    app.state = AppState::NotificationCenter;
                                }
                                MenuItem::ExportReport => {
                                    app.report_format = 0;
                                    app.state = AppState::ExportReport;
                                }
                                _ => app.start_single(item),
                            }
                        }
                        KeyCode::Char(c) if c.is_ascii_digit() => {
                            let idx = c.to_digit(10).unwrap() as usize;
                            if idx >= 1 && idx <= app.menu_items.len() {
                                let item = app.menu_items[idx - 1];
                                match item {
                                    MenuItem::FullProcess => app.state = AppState::StepSelect,
                                    MenuItem::Settings => app.state = AppState::Settings,
                                    MenuItem::BatchQueue => { app.batch_selected = 0; app.state = AppState::BatchQueue; }
                                    MenuItem::Profiles => { app.profile_selected = 0; app.state = AppState::Profiles; }
                                    MenuItem::WatchMode => app.state = AppState::WatchMode,
                                    MenuItem::Statistics => { app.stats_scroll = 0; app.state = AppState::Stats; }
                                    MenuItem::Duplicates => {
                                        app.scan_duplicates();
                                        app.dup_group_selected = 0;
                                        app.state = AppState::DuplicateGroups;
                                    }
                                    MenuItem::JxlSettings => app.state = AppState::JxlSettings,
                                    MenuItem::SizeCompare => {
                                        app.build_size_comparisons();
                                        app.state = AppState::SizeCompare;
                                    }
                                    MenuItem::ErrorPanel => {
                                        app.collect_error_details();
                                        app.state = AppState::ErrorPanel;
                                    }
                                    MenuItem::Presets => app.state = AppState::Presets,
                                    MenuItem::Scheduler => app.state = AppState::Scheduler,
                                    MenuItem::HistoryExport => app.state = AppState::HistoryExport,
                                    MenuItem::ThemeEditor => {
                                        app.load_custom_themes();
                                        app.state = AppState::ThemeEditor;
                                    }
                                    MenuItem::CompressionGraph => {
                                        app.build_compression_stats();
                                        app.state = AppState::CompressionGraph;
                                    }
                                    MenuItem::FileClassify => app.state = AppState::FileClassify,
                                    MenuItem::MetaEdit => {
                                        app.load_meta_files();
                                        app.state = AppState::MetaEdit;
                                    }
                                    MenuItem::ConfigIO => app.state = AppState::ConfigIO,
                                    MenuItem::Plugins => {
                                        app.scan_plugins();
                                        app.state = AppState::Plugins;
                                    }
                                    // Batch 3 dispatch (number keys)
                                    MenuItem::ImagePreview => {
                                        app.generate_image_preview(&app.config.dest);
                                        app.state = AppState::ImagePreview;
                                    }
                                    MenuItem::FuzzyFinder => {
                                        app.fuzzy_mode = true;
                                        app.fuzzy_query.clear();
                                    }
                                    MenuItem::SplitPane => {
                                        app.split_mode = true;
                                        app.state = AppState::SplitPane;
                                    }
                                    MenuItem::QuickActions => {
                                        app.quick_selected = 0;
                                        app.state = AppState::QuickActions;
                                    }
                                    MenuItem::RecentFiles => {
                                        app.recent_scroll = 0;
                                        app.state = AppState::RecentFiles;
                                    }
                                    MenuItem::TagSystem => {
                                        app.tag_selected = 0;
                                        app.state = AppState::TagSystem;
                                    }
                                    MenuItem::SideBySideDiff => {
                                        app.build_size_comparisons();
                                        app.state = AppState::SideBySideDiff;
                                    }
                                    MenuItem::FileTreeView => {
                                        app.build_file_tree();
                                        app.state = AppState::FileTreeView;
                                    }
                                    MenuItem::RenamePattern => {
                                        app.rename_selected = 0;
                                        app.state = AppState::RenamePattern;
                                    }
                                    MenuItem::Timeline => {
                                        app.build_timeline();
                                        app.state = AppState::Timeline;
                                    }
                                    MenuItem::CommandPalette => {
                                        app.palette_open = true;
                                        app.palette_query.clear();
                                    }
                                    MenuItem::NotificationCenter => {
                                        app.state = AppState::NotificationCenter;
                                    }
                                    MenuItem::ExportReport => {
                                        app.state = AppState::ExportReport;
                                    }
                                    _ => app.start_single(item),
                                }
                            }
                        }
                        _ => {}
                    },
                    AppState::StepSelect => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.state = AppState::Menu,
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.step_selected > 0 { app.step_selected -= 1; }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.step_selected < FULL_STEP_LABELS.len() - 1 { app.step_selected += 1; }
                        }
                        KeyCode::Char(' ') => {
                            let i = app.step_selected;
                            app.step_enabled[i] = !app.step_enabled[i];
                        }
                        KeyCode::Char('a') => {
                            let all_on = app.step_enabled.iter().all(|&e| e);
                            for e in app.step_enabled.iter_mut() { *e = !all_on; }
                        }
                        KeyCode::Enter => {
                            app.confirm_action = Some(ConfirmAction::StartProcessing);
                            app.confirm_yes = true;
                            app.state = AppState::ConfirmDialog;
                        }
                        _ => {}
                    },
                    AppState::Preview => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.state = AppState::StepSelect,
                        KeyCode::Up | KeyCode::Char('k') => {
                            app.preview_scroll = app.preview_scroll.saturating_sub(1);
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.preview_scroll < app.preview_items.len().saturating_sub(1) {
                                app.preview_scroll += 1;
                            }
                        }
                        KeyCode::PageUp => {
                            app.preview_scroll = app.preview_scroll.saturating_sub(10);
                        }
                        KeyCode::PageDown => {
                            app.preview_scroll = (app.preview_scroll + 10).min(app.preview_items.len().saturating_sub(1));
                        }
                        KeyCode::Home => app.preview_scroll = 0,
                        KeyCode::End => app.preview_scroll = app.preview_items.len().saturating_sub(1),
                        KeyCode::Enter => {
                            app.start_processing();
                        }
                        _ => {}
                    },
                    AppState::Processing => {
                        if key.code == KeyCode::Char('q') || key.code == KeyCode::Esc {
                            // Allow viewing log during processing
                        }
                        if !*app.is_processing.lock().unwrap() {
                            let elapsed = app.start_time.lock().unwrap()
                                .map(|t| t.elapsed().as_secs_f64())
                                .unwrap_or(0.0);
                            let errs = app.errors.lock().unwrap().len();
                            let processed = *app.files_processed.lock().unwrap();
                            let entry = HistoryEntry {
                                timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                                action: "Full Process".into(),
                                source: app.config.twitter_src.clone(),
                                files_processed: processed,
                                files_removed: 0,
                                files_renamed: 0,
                                original_size: 0,
                                compressed_size: 0,
                                duration_secs: elapsed,
                                errors: errs,
                            };
                            app.history.add(entry);
                            app.state = AppState::Done;
                            notify_done(errs == 0);
                        }
                    }
                    AppState::Done => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => { app.save_state(); return Ok(()); }
                        KeyCode::Char('r') | KeyCode::Enter => {
                            app.state = AppState::Menu;
                        }
                        KeyCode::Char('/') => {
                            app.search_mode = true;
                            app.search_query.clear();
                        }
                        _ => {}
                    },
                    AppState::Settings => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.state = AppState::Menu,
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.settings_selected > 0 { app.settings_selected -= 1; }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.settings_selected < 7 { app.settings_selected += 1; }
                        }
                        KeyCode::Enter => {
                            let _ = app.config.save();
                            app.state = AppState::Menu;
                        }
                        _ => {}
                    },
                    // Feature #4: Batch Queue
                    AppState::BatchQueue => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.state = AppState::Menu,
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.batch_selected > 0 { app.batch_selected -= 1; }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.batch_selected < app.batch_queue.len() { app.batch_selected += 1; }
                        }
                        KeyCode::Char('a') => {
                            app.batch_adding = true;
                            app.batch_input.clear();
                        }
                        KeyCode::Char('d') => {
                            if app.batch_selected < app.batch_queue.len() {
                                app.batch_queue.remove(app.batch_selected);
                                if app.batch_selected > 0 { app.batch_selected -= 1; }
                            }
                        }
                        KeyCode::Enter => {
                            if app.batch_adding {
                                if !app.batch_input.is_empty() {
                                    app.batch_queue.push(BatchJob {
                                        path: app.batch_input.clone(),
                                        status: "pending".into(),
                                        files_processed: 0,
                                    });
                                }
                                app.batch_adding = false;
                                app.batch_input.clear();
                            } else if !app.batch_queue.is_empty() {
                                // Process batch queue - collect paths first to avoid borrow issues
                                let paths: Vec<String> = app.batch_queue.iter().map(|j| j.path.clone()).collect();
                                for (i, path) in paths.iter().enumerate() {
                                    if let Some(job) = app.batch_queue.get_mut(i) {
                                        job.status = "processing".into();
                                    }
                                    let old_dest = app.config.dest.clone();
                                    app.config.dest = path.clone();
                                    app.start_processing();
                                    app.config.dest = old_dest;
                                    if let Some(job) = app.batch_queue.get_mut(i) {
                                        job.status = "done".into();
                                    }
                                }
                            }
                        }
                        KeyCode::Char(c) if app.batch_adding => {
                            app.batch_input.push(c);
                        }
                        KeyCode::Backspace if app.batch_adding => {
                            app.batch_input.pop();
                        }
                        _ => {}
                    },
                    // Feature #3: Duplicate Groups
                    AppState::DuplicateGroups => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.state = AppState::Menu,
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.dup_group_selected > 0 { app.dup_group_selected -= 1; }
                            app.dup_file_selected = 0;
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.dup_group_selected < app.duplicate_groups.len().saturating_sub(1) {
                                app.dup_group_selected += 1;
                            }
                            app.dup_file_selected = 0;
                        }
                        KeyCode::Left | KeyCode::Char('h') => {
                            if app.dup_file_selected > 0 { app.dup_file_selected -= 1; }
                        }
                        KeyCode::Right | KeyCode::Char('l') => {
                            if app.dup_group_selected < app.duplicate_groups.len() {
                                let group = &app.duplicate_groups[app.dup_group_selected];
                                if app.dup_file_selected < group.files.len().saturating_sub(1) {
                                    app.dup_file_selected += 1;
                                }
                            }
                        }
                        KeyCode::Char(' ') => {
                            if app.dup_group_selected < app.duplicate_groups.len() {
                                app.duplicate_groups[app.dup_group_selected].selected = app.dup_file_selected;
                            }
                        }
                        KeyCode::Char('x') => {
                            // Delete non-selected duplicates
                            if app.dup_group_selected < app.duplicate_groups.len() {
                                let group = &app.duplicate_groups[app.dup_group_selected];
                                let keep = group.selected;
                                for (i, (path, _)) in group.files.iter().enumerate() {
                                    if i != keep && !app.dry_run {
                                        let _ = fs::remove_file(path);
                                    }
                                }
                                if let Ok(mut logs) = app.logs.lock() {
                                    logs.push(format!("Removed {} duplicates (kept #{})", group.files.len() - 1, keep + 1));
                                }
                            }
                        }
                        _ => {}
                    },
                    // Feature #8: Stats
                    AppState::Stats => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.state = AppState::Menu,
                        KeyCode::Up | KeyCode::Char('k') => app.stats_scroll = app.stats_scroll.saturating_sub(1),
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.stats_scroll < 20 { app.stats_scroll += 1; }
                        }
                        _ => {}
                    },
                    // Feature #9: Profiles
                    AppState::Profiles => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.state = AppState::Menu,
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.profile_selected > 0 { app.profile_selected -= 1; }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.profile_selected < app.config.profiles.len() + 1 { app.profile_selected += 1; }
                        }
                        KeyCode::Char('a') => {
                            app.profile_adding = true;
                            app.profile_input.clear();
                        }
                        KeyCode::Enter => {
                            if app.profile_adding {
                                if !app.profile_input.is_empty() {
                                    app.save_profile(app.profile_input.clone());
                                    app.profile_adding = false;
                                    app.profile_input.clear();
                                }
                            } else if app.profile_selected < app.config.profiles.len() {
                                app.load_profile(app.profile_selected);
                                if let Ok(mut logs) = app.logs.lock() {
                                    logs.push(format!("Loaded profile: {}", app.config.profiles[app.profile_selected].name));
                                }
                            } else if app.profile_selected == app.config.profiles.len() {
                                // Clear history option
                                app.confirm_action = Some(ConfirmAction::ClearHistory);
                                app.confirm_yes = true;
                                app.state = AppState::ConfirmDialog;
                            } else {
                                // Clear undo option
                                app.confirm_action = Some(ConfirmAction::ClearUndo);
                                app.confirm_yes = true;
                                app.state = AppState::ConfirmDialog;
                            }
                        }
                        KeyCode::Char('d') => {
                            if app.profile_selected < app.config.profiles.len() {
                                app.config.profiles.remove(app.profile_selected);
                                let _ = app.config.save();
                                if app.profile_selected > 0 { app.profile_selected -= 1; }
                            }
                        }
                        KeyCode::Char(c) if app.profile_adding => {
                            app.profile_input.push(c);
                        }
                        KeyCode::Backspace if app.profile_adding => {
                            app.profile_input.pop();
                        }
                        _ => {}
                    },
                    // Feature #19: JXL Settings
                    AppState::JxlSettings => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.state = AppState::Menu,
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.settings_selected > 0 { app.settings_selected -= 1; }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.settings_selected < 2 { app.settings_selected += 1; }
                        }
                        KeyCode::Left | KeyCode::Char('h') => {
                            if app.settings_selected == 0 && app.config.jxl_quality > 1 {
                                app.config.jxl_quality -= 5;
                            }
                        }
                        KeyCode::Right | KeyCode::Char('l') => {
                            if app.settings_selected == 0 && app.config.jxl_quality < 100 {
                                app.config.jxl_quality += 5;
                            }
                        }
                        KeyCode::Char(' ') => {
                            if app.settings_selected == 1 {
                                app.config.jxl_lossless = !app.config.jxl_lossless;
                            }
                        }
                        KeyCode::Enter => {
                            let _ = app.config.save();
                            app.state = AppState::Menu;
                        }
                        _ => {}
                    },
                    // Feature #20: Watch Mode
                    AppState::WatchMode => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.state = AppState::Menu,
                        KeyCode::Char('w') => {
                            app.watch_active = !app.watch_active;
                            if app.watch_active {
                                app.watch_processed = 0;
                                app.watch_last_scan = Instant::now();
                            }
                        }
                        KeyCode::Char('a') => {
                            app.batch_adding = true;
                            app.batch_input.clear();
                        }
                        KeyCode::Char(c) if app.batch_adding => {
                            app.batch_input.push(c);
                        }
                        KeyCode::Backspace if app.batch_adding => {
                            app.batch_input.pop();
                        }
                        KeyCode::Enter if app.batch_adding => {
                            if !app.batch_input.is_empty() {
                                app.config.watch_dirs.push(app.batch_input.clone());
                                let _ = app.config.save();
                            }
                            app.batch_adding = false;
                            app.batch_input.clear();
                        }
                        _ => {}
                    },
                    // Feature #6, #7: Filter & Sort
                    AppState::FilterSort => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.state = AppState::Menu,
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.filter_selected > 0 { app.filter_selected -= 1; }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.filter_selected < 4 { app.filter_selected += 1; }
                        }
                        KeyCode::Enter => {
                            let _ = app.config.save();
                            app.state = AppState::Menu;
                        }
                        KeyCode::Left | KeyCode::Char('h') => {
                            match app.filter_selected {
                                1 => { if app.filter.min_size_kb > 0 { app.filter.min_size_kb -= 100; } }
                                2 => { if app.filter.max_size_kb > 0 { app.filter.max_size_kb -= 100; } }
                                4 => {
                                    app.sort_config = match app.sort_config.field {
                                        SortField::Name => SortConfig { field: SortField::Type, ascending: app.sort_config.ascending },
                                        SortField::Size => SortConfig { field: SortField::Name, ascending: app.sort_config.ascending },
                                        SortField::Date => SortConfig { field: SortField::Size, ascending: app.sort_config.ascending },
                                        SortField::Type => SortConfig { field: SortField::Date, ascending: app.sort_config.ascending },
                                    };
                                }
                                _ => {}
                            }
                        }
                        KeyCode::Right | KeyCode::Char('l') => {
                            match app.filter_selected {
                                1 => { app.filter.min_size_kb += 100; }
                                2 => { app.filter.max_size_kb += 100; }
                                4 => {
                                    app.sort_config = match app.sort_config.field {
                                        SortField::Name => SortConfig { field: SortField::Size, ascending: app.sort_config.ascending },
                                        SortField::Size => SortConfig { field: SortField::Date, ascending: app.sort_config.ascending },
                                        SortField::Date => SortConfig { field: SortField::Type, ascending: app.sort_config.ascending },
                                        SortField::Type => SortConfig { field: SortField::Name, ascending: app.sort_config.ascending },
                                    };
                                }
                                _ => {}
                            }
                        }
                        KeyCode::Backspace => {
                            if app.filter_selected == 3 {
                                app.filter.name_pattern.pop();
                            }
                        }
                        KeyCode::Char(c) => {
                            if app.filter_selected == 3 {
                                app.filter.name_pattern.push(c);
                            }
                        }
                        _ => {}
                    },
                    // Feature #11: Info Panel
                    AppState::InfoPanel => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc | KeyCode::Char('i') => app.state = AppState::Preview,
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.info_selected > 0 { app.info_selected -= 1; }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.info_selected < app.preview_items.len().saturating_sub(1) {
                                app.info_selected += 1;
                            }
                        }
                        _ => {}
                    },
                    AppState::ConfirmDialog => {}, // Handled above
                    AppState::Help => {}, // Handled above
                    // New feature state handlers
                    AppState::SizeCompare => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.state = AppState::Menu,
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.size_compare_scroll > 0 { app.size_compare_scroll -= 1; }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.size_compare_scroll < app.size_comparisons.len().saturating_sub(1) {
                                app.size_compare_scroll += 1;
                            }
                        }
                        KeyCode::Char('r') => app.build_size_comparisons(),
                        KeyCode::Char('s') => {
                            app.build_compression_stats();
                            app.state = AppState::CompressionGraph;
                        }
                        _ => {}
                    },
                    AppState::ErrorPanel => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.state = AppState::Menu,
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.error_scroll > 0 { app.error_scroll -= 1; }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.error_scroll < app.error_details.len().saturating_sub(1) {
                                app.error_scroll += 1;
                            }
                        }
                        KeyCode::Char('c') => {
                            app.error_details.clear();
                            if let Ok(mut errs) = app.errors.lock() { errs.clear(); }
                        }
                        KeyCode::Char('e') => {
                            let _ = app.export_log();
                        }
                        _ => {}
                    },
                    AppState::Presets => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.state = AppState::Menu,
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.preset_selected > 0 { app.preset_selected -= 1; }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.preset_selected < app.presets.len().saturating_sub(1) {
                                app.preset_selected += 1;
                            }
                        }
                        KeyCode::Enter => {
                            app.apply_preset(app.preset_selected);
                            let _ = app.config.save();
                            app.state = AppState::Menu;
                        }
                        _ => {}
                    },
                    AppState::Scheduler => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.state = AppState::Menu,
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.scheduler_selected > 0 { app.scheduler_selected -= 1; }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.scheduler_selected < app.scheduler_jobs.len().saturating_sub(1) {
                                app.scheduler_selected += 1;
                            }
                        }
                        KeyCode::Char('a') => {
                            app.scheduler_jobs.push(SchedulerJob::default());
                            app.scheduler_selected = app.scheduler_jobs.len() - 1;
                            app.scheduler_editing = true;
                            app.scheduler_field = 0;
                        }
                        KeyCode::Char('e') => {
                            if !app.scheduler_jobs.is_empty() {
                                app.scheduler_editing = true;
                                app.scheduler_field = 0;
                            }
                        }
                        KeyCode::Char('t') => {
                            if let Some(job) = app.scheduler_jobs.get_mut(app.scheduler_selected) {
                                job.enabled = !job.enabled;
                            }
                        }
                        KeyCode::Char('d') => {
                            if !app.scheduler_jobs.is_empty() {
                                app.scheduler_jobs.remove(app.scheduler_selected);
                                if app.scheduler_selected >= app.scheduler_jobs.len() && app.scheduler_selected > 0 {
                                    app.scheduler_selected -= 1;
                                }
                            }
                        }
                        KeyCode::Enter if app.scheduler_editing => {
                            app.scheduler_editing = false;
                        }
                        KeyCode::Left | KeyCode::Char('h') if app.scheduler_editing => {
                            if let Some(job) = app.scheduler_jobs.get_mut(app.scheduler_selected) {
                                match app.scheduler_field {
                                    0 => { if job.hour > 0 { job.hour -= 1; } }
                                    1 => { if job.minute > 0 { job.minute -= 1; } }
                                    _ => {}
                                }
                            }
                        }
                        KeyCode::Right | KeyCode::Char('l') if app.scheduler_editing => {
                            if let Some(job) = app.scheduler_jobs.get_mut(app.scheduler_selected) {
                                match app.scheduler_field {
                                    0 => { if job.hour < 23 { job.hour += 1; } }
                                    1 => { if job.minute < 59 { job.minute += 1; } }
                                    _ => {}
                                }
                            }
                        }
                        KeyCode::Tab if app.scheduler_editing => {
                            app.scheduler_field = (app.scheduler_field + 1) % 2;
                        }
                        _ => {}
                    },
                    AppState::HistoryExport => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.state = AppState::Menu,
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.export_format > 0 { app.export_format -= 1; }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.export_format < 1 { app.export_format += 1; }
                        }
                        KeyCode::Enter => {
                            let result = if app.export_format == 0 {
                                app.export_history_csv()
                            } else {
                                app.export_history_json()
                            };
                            match result {
                                Ok(path) => {
                                    if let Ok(mut logs) = app.logs.lock() {
                                        logs.push(format!("Exported to: {}", path));
                                    }
                                    app.state = AppState::Menu;
                                }
                                Err(e) => {
                                    if let Ok(mut logs) = app.logs.lock() {
                                        logs.push(format!("Export failed: {}", e));
                                    }
                                }
                            }
                        }
                        _ => {}
                    },
                    AppState::ThemeEditor => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.state = AppState::Menu,
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.theme_edit_selected > 0 { app.theme_edit_selected -= 1; }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.theme_edit_selected < 9 { app.theme_edit_selected += 1; }
                        }
                        KeyCode::Left | KeyCode::Char('h') => {
                            app.theme_edit_field = app.theme_edit_field.saturating_sub(1);
                        }
                        KeyCode::Right | KeyCode::Char('l') => {
                            if app.theme_edit_field < 2 { app.theme_edit_field += 1; }
                        }
                        KeyCode::Char('a') => {
                            app.save_custom_theme(&format!("Theme_{}", app.custom_themes.len() + 1));
                        }
                        KeyCode::Enter => {
                            app.state = AppState::Menu;
                        }
                        _ => {}
                    },
                    AppState::DashboardCustom => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.state = AppState::Stats,
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.dashboard_selected > 0 { app.dashboard_selected -= 1; }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.dashboard_selected < 3 { app.dashboard_selected += 1; }
                        }
                        KeyCode::Char(' ') => {
                            match app.dashboard_selected {
                                0 => app.widget_layout.show_summary = !app.widget_layout.show_summary,
                                1 => app.widget_layout.show_chart = !app.widget_layout.show_chart,
                                2 => app.widget_layout.show_history = !app.widget_layout.show_history,
                                3 => app.widget_layout.show_compression = !app.widget_layout.show_compression,
                                _ => {}
                            }
                        }
                        KeyCode::Enter => {
                            let _ = fs::write(
                                ".io_tool_dashboard.json",
                                serde_json::to_string_pretty(&app.widget_layout).unwrap_or_default(),
                            );
                            app.state = AppState::Stats;
                        }
                        _ => {}
                    },
                    AppState::CompressionGraph => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.state = AppState::Menu,
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.compress_scroll > 0 { app.compress_scroll -= 1; }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.compress_scroll < app.compression_stats.len().saturating_sub(1) {
                                app.compress_scroll += 1;
                            }
                        }
                        KeyCode::Char('r') => app.build_compression_stats(),
                        _ => {}
                    },
                    AppState::FileClassify => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.state = AppState::Menu,
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.classify_selected > 0 { app.classify_selected -= 1; }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.classify_selected < app.classify_rules.len().saturating_sub(1) {
                                app.classify_selected += 1;
                            }
                        }
                        KeyCode::Char('a') => {
                            app.classify_adding = true;
                            app.classify_input.clear();
                        }
                        KeyCode::Char('d') => {
                            if !app.classify_rules.is_empty() {
                                app.classify_rules.remove(app.classify_selected);
                                if app.classify_selected >= app.classify_rules.len() && app.classify_selected > 0 {
                                    app.classify_selected -= 1;
                                }
                            }
                        }
                        KeyCode::Enter if app.classify_adding => {
                            let parts: Vec<&str> = app.classify_input.splitn(2, ':').collect();
                            if parts.len() == 2 {
                                app.classify_rules.push((parts[0].trim().to_string(), parts[1].trim().to_string()));
                            }
                            app.classify_adding = false;
                            app.classify_input.clear();
                        }
                        KeyCode::Char('r') => app.classify_files(),
                        KeyCode::Char(c) if app.classify_adding => {
                            app.classify_input.push(c);
                        }
                        KeyCode::Backspace if app.classify_adding => {
                            app.classify_input.pop();
                        }
                        _ => {}
                    },
                    AppState::MetaEdit => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.state = AppState::Menu,
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.meta_scroll > 0 { app.meta_scroll -= 1; }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.meta_scroll < app.meta_files.len().saturating_sub(1) {
                                app.meta_scroll += 1;
                            }
                        }
                        KeyCode::Char(' ') => {
                            if let Some((_, selected)) = app.meta_files.get_mut(app.meta_scroll) {
                                *selected = !*selected;
                            }
                        }
                        KeyCode::Char('a') => {
                            for (_, selected) in app.meta_files.iter_mut() {
                                *selected = true;
                            }
                        }
                        KeyCode::Tab => {
                            app.meta_field = (app.meta_field + 1) % 3;
                        }
                        KeyCode::Char('x') => {
                            let count = app.meta_files.iter().filter(|(_, s)| *s).count();
                            app.meta_files.retain(|(_, s)| !s);
                            if let Ok(mut logs) = app.logs.lock() {
                                logs.push(format!("Removed metadata from {} files", count));
                            }
                        }
                        _ => {}
                    },
                    AppState::ConfigIO => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.state = AppState::Menu,
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.config_io_selected > 0 { app.config_io_selected -= 1; }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.config_io_selected < 1 { app.config_io_selected += 1; }
                        }
                        KeyCode::Char('e') => {
                            match app.export_config() {
                                Ok(path) => {
                                    if let Ok(mut logs) = app.logs.lock() {
                                        logs.push(format!("Config exported to: {}", path));
                                    }
                                }
                                Err(e) => {
                                    if let Ok(mut logs) = app.logs.lock() {
                                        logs.push(format!("Export failed: {}", e));
                                    }
                                }
                            }
                        }
                        KeyCode::Char('i') => {
                            app.config_io_adding = true;
                            app.config_io_path.clear();
                        }
                        KeyCode::Enter if app.config_io_adding => {
                            let path = app.config_io_path.clone();
                            match app.import_config(&path) {
                                Ok(()) => {
                                    if let Ok(mut logs) = app.logs.lock() {
                                        logs.push("Config imported successfully".into());
                                    }
                                }
                                Err(e) => {
                                    if let Ok(mut logs) = app.logs.lock() {
                                        logs.push(format!("Import failed: {}", e));
                                    }
                                }
                            }
                            app.config_io_adding = false;
                            app.config_io_path.clear();
                        }
                        KeyCode::Char(c) if app.config_io_adding => {
                            app.config_io_path.push(c);
                        }
                        KeyCode::Backspace if app.config_io_adding => {
                            app.config_io_path.pop();
                        }
                        _ => {}
                    },
                    AppState::Plugins => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.state = AppState::Menu,
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.plugin_selected > 0 { app.plugin_selected -= 1; }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.plugin_selected < app.plugins.len().saturating_sub(1) {
                                app.plugin_selected += 1;
                            }
                        }
                        KeyCode::Char(' ') => {
                            if let Some(plugin) = app.plugins.get_mut(app.plugin_selected) {
                                plugin.enabled = !plugin.enabled;
                            }
                        }
                        KeyCode::Char('r') => app.scan_plugins(),
                        KeyCode::Char('o') => {
                            if let Err(e) = std::process::Command::new("explorer").arg(&app.plugin_dir).spawn() {
                                if let Ok(mut logs) = app.logs.lock() {
                                    logs.push(format!("Failed to open: {}", e));
                                }
                            }
                        }
                        _ => {}
                    },
                    AppState::StatusbarCustom => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.state = AppState::Menu,
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.statusbar_selected > 0 { app.statusbar_selected -= 1; }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.statusbar_selected < app.statusbar_items.len().saturating_sub(1) {
                                app.statusbar_selected += 1;
                            }
                        }
                        KeyCode::Char(' ') => {
                            if let Some((_, enabled)) = app.statusbar_items.get_mut(app.statusbar_selected) {
                                *enabled = !*enabled;
                            }
                        }
                        KeyCode::Enter => {
                            app.save_statusbar_config();
                            app.state = AppState::Menu;
                        }
                        _ => {}
                    },
                    // Batch 3 event handlers
                    AppState::ImagePreview => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.state = AppState::Menu,
                        KeyCode::Up | KeyCode::Char('k') => {
                            if let Some(ref mut p) = app.image_preview {
                                if p.height > 0 { p.height -= 1; }
                            }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if let Some(ref mut p) = app.image_preview {
                                p.height += 1;
                            }
                        }
                        _ => {}
                    },
                    AppState::SplitPane => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            app.split_mode = false;
                            app.state = AppState::Menu;
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.split_left_scroll > 0 { app.split_left_scroll -= 1; }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            app.split_left_scroll += 1;
                        }
                        KeyCode::Tab => {
                            app.split_mode = !app.split_mode;
                        }
                        _ => {}
                    },
                    AppState::QuickActions => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.state = AppState::Menu,
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.quick_selected > 0 { app.quick_selected -= 1; }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.quick_selected < app.quick_actions.len().saturating_sub(1) {
                                app.quick_selected += 1;
                            }
                        }
                        KeyCode::Enter => {
                            if let Some((_, idx)) = app.quick_actions.get(app.quick_selected) {
                                let menu_items = MenuItem::all();
                                if *idx < menu_items.len() {
                                    let item = menu_items[*idx];
                                    match item {
                                        MenuItem::FullProcess => app.state = AppState::StepSelect,
                                        MenuItem::Settings => app.state = AppState::Settings,
                                        MenuItem::Statistics => app.state = AppState::Stats,
                                        _ => {}
                                    }
                                }
                            }
                        }
                        _ => {}
                    },
                    AppState::RecentFiles => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.state = AppState::Menu,
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.recent_scroll > 0 { app.recent_scroll -= 1; }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            app.recent_scroll += 1;
                        }
                        _ => {}
                    },
                    AppState::TagSystem => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            app.tag_adding = false;
                            app.state = AppState::Menu;
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.tag_selected > 0 { app.tag_selected -= 1; }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.tag_selected < app.file_tags.len().saturating_sub(1) {
                                app.tag_selected += 1;
                            }
                        }
                        KeyCode::Char('a') => {
                            app.tag_adding = true;
                            app.tag_input.clear();
                        }
                        KeyCode::Enter if app.tag_adding => {
                            if !app.tag_input.is_empty() {
                                app.add_file_tag("*".into(), app.tag_input.clone());
                                app.tag_input.clear();
                                app.tag_adding = false;
                            }
                        }
                        KeyCode::Backspace if app.tag_adding => {
                            app.tag_input.pop();
                        }
                        KeyCode::Char(c) if app.tag_adding => {
                            app.tag_input.push(c);
                        }
                        _ => {}
                    },
                    AppState::SideBySideDiff => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.state = AppState::Menu,
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.diff_scroll > 0 { app.diff_scroll -= 1; }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            app.diff_scroll += 1;
                        }
                        _ => {}
                    },
                    AppState::FileTreeView => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.state = AppState::Menu,
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.tree_selected > 0 { app.tree_selected -= 1; }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            app.tree_selected += 1;
                        }
                        KeyCode::Enter => {
                            // Toggle expand/collapse
                            fn toggle_node(nodes: &mut [FileTreeNode], idx: usize, counter: &mut usize) {
                                for node in nodes.iter_mut() {
                                    if *counter == idx {
                                        node.expanded = !node.expanded;
                                        return;
                                    }
                                    *counter += 1;
                                    if node.expanded {
                                        toggle_node(&mut node.children, idx, counter);
                                    }
                                }
                            }
                            let mut counter = 0;
                            toggle_node(&mut app.file_tree, app.tree_selected, &mut counter);
                        }
                        _ => {}
                    },
                    AppState::RenamePattern => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => {
                            app.rename_field = 0;
                            app.state = AppState::Menu;
                        }
                        KeyCode::Tab => {
                            app.rename_field = (app.rename_field + 1) % 2;
                        }
                        KeyCode::Char('a') => {
                            app.rename_patterns.push(RenamePattern {
                                pattern: String::new(),
                                replacement: String::new(),
                                preview: Vec::new(),
                                use_regex: false,
                            });
                            app.rename_selected = app.rename_patterns.len() - 1;
                        }
                        KeyCode::Char('r') => {
                            if let Some(p) = app.rename_patterns.get_mut(app.rename_selected) {
                                p.use_regex = !p.use_regex;
                            }
                        }
                        KeyCode::Char('p') => {
                            app.preview_rename_pattern();
                        }
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.rename_selected > 0 { app.rename_selected -= 1; }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.rename_selected < app.rename_patterns.len().saturating_sub(1) {
                                app.rename_selected += 1;
                            }
                        }
                        KeyCode::Backspace => {
                            if let Some(p) = app.rename_patterns.get_mut(app.rename_selected) {
                                if app.rename_field == 0 { p.pattern.pop(); } else { p.replacement.pop(); }
                            }
                        }
                        KeyCode::Char(c) => {
                            if let Some(p) = app.rename_patterns.get_mut(app.rename_selected) {
                                if app.rename_field == 0 { p.pattern.push(c); } else { p.replacement.push(c); }
                            }
                        }
                        _ => {}
                    },
                    AppState::Timeline => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.state = AppState::Menu,
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.timeline_scroll > 0 { app.timeline_scroll -= 1; }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            app.timeline_scroll += 1;
                        }
                        _ => {}
                    },
                    AppState::NotificationCenter => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.state = AppState::Menu,
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.notif_scroll > 0 { app.notif_scroll -= 1; }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            app.notif_scroll += 1;
                        }
                        KeyCode::Char('c') => {
                            app.notifications.clear();
                            app.notif_scroll = 0;
                        }
                        KeyCode::Enter => {
                            if let Some(n) = app.notifications.get_mut(app.notif_scroll) {
                                n.read = true;
                            }
                        }
                        _ => {}
                    },
                    AppState::ExportReport => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.state = AppState::Menu,
                        KeyCode::Left | KeyCode::Char('h') => {
                            app.report_format = 0;
                        }
                        KeyCode::Right | KeyCode::Char('l') => {
                            app.report_format = 1;
                        }
                        KeyCode::Enter => {
                            match app.export_report() {
                                Ok(path) => {
                                    app.add_notification(format!("Report exported to {}", path), "success".into());
                                    app.state = AppState::Menu;
                                }
                                Err(e) => {
                                    app.add_notification(format!("Export failed: {}", e), "error".into());
                                }
                            }
                        }
                        _ => {}
                    },
                    AppState::SimilarImages => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.state = AppState::Menu,
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.similar_selected > 0 { app.similar_selected -= 1; }
                            app.similar_file_selected = 0;
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.similar_selected < app.similar_groups.len().saturating_sub(1) {
                                app.similar_selected += 1;
                            }
                            app.similar_file_selected = 0;
                        }
                        KeyCode::Left | KeyCode::Char('h') => {
                            if app.similar_file_selected > 0 { app.similar_file_selected -= 1; }
                        }
                        KeyCode::Right | KeyCode::Char('l') => {
                            if let Some(group) = app.similar_groups.get(app.similar_selected) {
                                if app.similar_file_selected < group.files.len().saturating_sub(1) {
                                    app.similar_file_selected += 1;
                                }
                            }
                        }
                        KeyCode::Char('+') | KeyCode::Char('=') => {
                            if app.similar_threshold < 64 { app.similar_threshold += 1; }
                            app.scan_similar_images();
                        }
                        KeyCode::Char('-') => {
                            if app.similar_threshold > 0 { app.similar_threshold -= 1; }
                            app.scan_similar_images();
                        }
                        KeyCode::Char('s') => {
                            app.scan_similar_images();
                        }
                        KeyCode::Char('d') => {
                            // Delete non-selected files in current group
                            if let Some(group) = app.similar_groups.get(app.similar_selected) {
                                let keep = app.similar_file_selected;
                                for (i, (path, _)) in group.files.iter().enumerate() {
                                    if i != keep {
                                        let _ = fs::remove_file(path);
                                    }
                                }
                                app.scan_similar_images();
                            }
                        }
                        _ => {}
                    },
                    AppState::CommandPalette => match key.code {
                        KeyCode::Char('q') | KeyCode::Esc => app.state = AppState::Menu,
                        KeyCode::Up | KeyCode::Char('k') => {
                            if app.palette_selected > 0 { app.palette_selected -= 1; }
                        }
                        KeyCode::Down | KeyCode::Char('j') => {
                            if app.palette_selected < app.palette_results.len().saturating_sub(1) {
                                app.palette_selected += 1;
                            }
                        }
                        KeyCode::Enter => {
                            if let Some((_, idx)) = app.palette_results.get(app.palette_selected) {
                                app.selected = *idx;
                                app.state = AppState::Menu;
                            }
                        }
                        KeyCode::Char(c) => {
                            app.palette_query.push(c);
                            app.update_palette_results();
                        }
                        KeyCode::Backspace => {
                            app.palette_query.pop();
                            app.update_palette_results();
                        }
                        _ => {}
                    },
                }

                // Handle search mode
                if app.search_mode {
                    match key.code {
                        KeyCode::Esc => {
                            app.search_mode = false;
                            app.search_query.clear();
                            app.filtered_log_indices.clear();
                        }
                        KeyCode::Enter => {
                            app.search_mode = false;
                            app.update_log_filter();
                        }
                        KeyCode::Backspace => {
                            app.search_query.pop();
                            app.update_log_filter();
                        }
                        KeyCode::Char(c) => {
                            app.search_query.push(c);
                            app.update_log_filter();
                        }
                        _ => {}
                    }
                }
            }
        }

        if app.state == AppState::Processing && !*app.is_processing.lock().unwrap() {
            let elapsed = app.start_time.lock().unwrap()
                .map(|t| t.elapsed().as_secs_f64())
                .unwrap_or(0.0);
            let errs = app.errors.lock().unwrap().len();
            let processed = *app.files_processed.lock().unwrap();
            let entry = HistoryEntry {
                timestamp: chrono::Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
                action: "Full Process".into(),
                source: app.config.twitter_src.clone(),
                files_processed: processed,
                files_removed: 0,
                files_renamed: 0,
                original_size: 0,
                compressed_size: 0,
                duration_secs: elapsed,
                errors: errs,
            };
            app.history.add(entry);
            app.state = AppState::Done;
            notify_done(errs == 0);
        }
    }
}

fn notify_done(success: bool) {
    let _ = Command::new("powershell")
        .args(&[
            "-NoProfile", "-Command",
            &format!(
                r#"Add-Type -AssemblyName System.Windows.Forms; $n = New-Object System.Windows.Forms.NotifyIcon; $n.Icon = [System.Drawing.SystemIcons]::{}; $n.Visible = $true; $n.ShowBalloonTip(5000, 'io-tool', '{}', [System.Windows.Forms.ToolTipIcon]::{})"#,
                if success { "Information" } else { "Warning" },
                if success { "Processing completed successfully!" } else { "Processing completed with errors." },
                if success { "Info" } else { "Warning" }
            ),
        ])
        .status();
}

// ============================================================
// TUI Rendering
// ============================================================

fn ui(f: &mut Frame, app: &mut App) {
    let theme = app.theme();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
            Constraint::Length(3),
            Constraint::Length(1),
        ])
        .split(f.area());

    // Header
    let header_text = match app.state {
        AppState::Menu => {
            let dry = if app.dry_run { " [DRY RUN]" } else { "" };
            let pause = if *app.is_paused.lock().unwrap() { " [PAUSED]" } else { "" };
            let watch = if app.watch_active { " [WATCH]" } else { "" };
            format!("  Image Processing Tool — io-tool{}{}{}  [Theme: {}]", dry, pause, watch, THEME_NAMES[app.theme_idx])
        }
        AppState::StepSelect => "  Full Process — Select Steps".to_string(),
        AppState::Preview => "  Preview — Rename Changes".to_string(),
        AppState::Processing => {
            let sp = SPINNER_CHARS[app.spinner_idx];
            let pause = if *app.is_paused.lock().unwrap() { " [PAUSED]" } else { "" };
            format!("  {} Processing...{}", sp, pause)
        }
        AppState::Done => "  Completed".to_string(),
        AppState::Settings => "  Settings".to_string(),
        AppState::Help => "  Help — Key Bindings".to_string(),
        AppState::BatchQueue => "  Batch Queue".to_string(),
        AppState::DuplicateGroups => "  Duplicate Groups".to_string(),
        AppState::Stats => "  Statistics Dashboard".to_string(),
        AppState::Profiles => "  Config Profiles".to_string(),
        AppState::JxlSettings => "  JXL Quality Settings".to_string(),
        AppState::WatchMode => "  Watch Mode".to_string(),
        AppState::FilterSort => "  Filter & Sort".to_string(),
        AppState::InfoPanel => "  File Info Panel".to_string(),
        AppState::ConfirmDialog => "  Confirm".to_string(),
        AppState::SizeCompare => "  Size Comparison".to_string(),
        AppState::ErrorPanel => "  Error Details".to_string(),
        AppState::Presets => "  Conversion Presets".to_string(),
        AppState::Scheduler => "  Process Scheduler".to_string(),
        AppState::HistoryExport => "  Export History".to_string(),
        AppState::ThemeEditor => "  Theme Editor".to_string(),
        AppState::DashboardCustom => "  Dashboard Customization".to_string(),
        AppState::CompressionGraph => "  Compression Graph".to_string(),
        AppState::FileClassify => "  File Classification".to_string(),
        AppState::MetaEdit => "  Metadata Editor".to_string(),
        AppState::ConfigIO => "  Config Import/Export".to_string(),
        AppState::Plugins => "  Plugins".to_string(),
        AppState::StatusbarCustom => "  Statusbar Settings".to_string(),
        // Batch 3 headers
        AppState::ImagePreview => "  Image Preview".to_string(),
        AppState::SplitPane => "  Split Pane View".to_string(),
        AppState::QuickActions => "  Quick Actions".to_string(),
        AppState::RecentFiles => "  Recent Files".to_string(),
        AppState::TagSystem => "  Tag System".to_string(),
        AppState::SideBySideDiff => "  Side-by-side Diff".to_string(),
        AppState::FileTreeView => "  File Tree View".to_string(),
        AppState::RenamePattern => "  Batch Rename Pattern".to_string(),
        AppState::Timeline => "  Processing Timeline".to_string(),
        AppState::CommandPalette => "  Command Palette".to_string(),
        AppState::NotificationCenter => "  Notification Center".to_string(),
        AppState::ExportReport => "  Export Report".to_string(),
        AppState::SimilarImages => "  Similar Image Search".to_string(),
    };
    let header = Paragraph::new(header_text)
        .style(Style::default().fg(theme.primary).add_modifier(Modifier::BOLD))
        .block(Block::default().borders(Borders::ALL).title("io-tool"));
    f.render_widget(header, chunks[0]);

    // Main content
    match app.state {
        AppState::Menu => render_menu(f, app, chunks[1]),
        AppState::StepSelect => render_step_select(f, app, chunks[1]),
        AppState::Preview => render_preview(f, app, chunks[1]),
        AppState::Processing => render_processing(f, app, chunks[1]),
        AppState::Done => render_done(f, app, chunks[1]),
        AppState::Settings => render_settings(f, app, chunks[1]),
        AppState::Help => render_help(f, app, chunks[1]),
        AppState::BatchQueue => render_batch_queue(f, app, chunks[1]),
        AppState::DuplicateGroups => render_duplicate_groups(f, app, chunks[1]),
        AppState::Stats => render_stats(f, app, chunks[1]),
        AppState::Profiles => render_profiles(f, app, chunks[1]),
        AppState::JxlSettings => render_jxl_settings(f, app, chunks[1]),
        AppState::WatchMode => render_watch_mode(f, app, chunks[1]),
        AppState::FilterSort => render_filter_sort(f, app, chunks[1]),
        AppState::InfoPanel => render_info_panel(f, app, chunks[1]),
        AppState::ConfirmDialog => render_confirm_dialog(f, app, chunks[1]),
        AppState::SizeCompare => render_size_compare(f, app, chunks[1]),
        AppState::ErrorPanel => render_error_panel(f, app, chunks[1]),
        AppState::Presets => render_presets(f, app, chunks[1]),
        AppState::Scheduler => render_scheduler(f, app, chunks[1]),
        AppState::HistoryExport => render_history_export(f, app, chunks[1]),
        AppState::ThemeEditor => render_theme_editor(f, app, chunks[1]),
        AppState::DashboardCustom => render_dashboard_custom(f, app, chunks[1]),
        AppState::CompressionGraph => render_compression_graph(f, app, chunks[1]),
        AppState::FileClassify => render_file_classify(f, app, chunks[1]),
        AppState::MetaEdit => render_meta_edit(f, app, chunks[1]),
        AppState::ConfigIO => render_config_io(f, app, chunks[1]),
        AppState::Plugins => render_plugins(f, app, chunks[1]),
        AppState::StatusbarCustom => render_statusbar_custom(f, app, chunks[1]),
        AppState::SimilarImages => render_similar_images(f, app, chunks[1]),
        AppState::ImagePreview => render_image_preview(f, app, chunks[1]),
        AppState::SplitPane => render_split_pane(f, app, chunks[1]),
        AppState::QuickActions => render_quick_actions(f, app, chunks[1]),
        AppState::RecentFiles => render_recent_files(f, app, chunks[1]),
        AppState::TagSystem => render_tag_system(f, app, chunks[1]),
        AppState::SideBySideDiff => render_side_by_side_diff(f, app, chunks[1]),
        AppState::FileTreeView => render_file_tree_view(f, app, chunks[1]),
        AppState::RenamePattern => render_rename_pattern(f, app, chunks[1]),
        AppState::Timeline => render_timeline(f, app, chunks[1]),
        AppState::CommandPalette => render_command_palette(f, app, chunks[1]),
        AppState::NotificationCenter => render_notification_center(f, app, chunks[1]),
        AppState::ExportReport => render_export_report(f, app, chunks[1]),
    }

    // Status bar (keybinds + info)
    render_status_bar(f, app, chunks[2]);

    // Bottom info bar (clock + stats + memory)
    render_info_bar(f, app, chunks[3]);
}

fn render_status_bar(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme();
    let footer_text = match app.state {
        AppState::Menu => "j/k: Nav │ 1-9: Select │ Enter: Run │ t:Theme │ d:DryRun │ u:Undo │ ?:Help │ q:Quit",
        AppState::StepSelect => "j/k: Nav │ Space: Toggle │ a: All │ Enter: Confirm │ Esc: Back",
        AppState::Preview => "j/k: Scroll │ PgUp/PgDn │ Home/End │ i:Info │ Enter: Start │ Esc: Back",
        AppState::Processing => "Processing... │ Ctrl+P: Pause │ /: Search log",
        AppState::Done => "r: Menu │ /: Search │ u: Undo │ Ctrl+E: Export │ q: Quit",
        AppState::Settings => "j/k: Nav │ Enter: Save │ Esc: Back",
        AppState::Help => "j/k: Scroll │ PgUp/PgDn │ Esc/?: Close",
        AppState::BatchQueue => "a: Add │ d: Delete │ Enter: Process │ Esc: Back",
        AppState::DuplicateGroups => "j/k: Group │ h/l: File │ Space: Keep │ x: Delete │ Esc: Back",
        AppState::Stats => "j/k: Scroll │ Esc: Back",
        AppState::Profiles => "a: Add │ d: Delete │ Enter: Load/Action │ Esc: Back",
        AppState::JxlSettings => "h/l: Quality │ Space: Lossless │ Enter: Save │ Esc: Back",
        AppState::WatchMode => "w: Toggle Watch │ a: Add Dir │ Esc: Back",
        AppState::FilterSort => "j/k: Field │ h/l: Value │ Enter: Apply │ Esc: Back",
        AppState::InfoPanel => "j/k: Select File │ Esc/i: Back",
        AppState::ConfirmDialog => "y/Enter: Yes │ n/Esc: No │ j/k: Toggle",
        AppState::SizeCompare => "j/k: Scroll │ r: Refresh │ s: Stats │ Esc: Back",
        AppState::ErrorPanel => "j/k: Scroll │ c: Clear │ e: Export │ Esc: Back",
        AppState::Presets => "j/k: Select │ Enter: Apply │ Esc: Back",
        AppState::Scheduler => "a: Add │ e: Edit │ t: Toggle │ d: Delete │ Esc: Back",
        AppState::HistoryExport => "j/k: Format │ Enter: Export │ Esc: Back",
        AppState::ThemeEditor => "j/k: Color │ h/l: Channel │ a: Save │ Esc: Back",
        AppState::DashboardCustom => "j/k: Widget │ Space: Toggle │ Enter: Save │ Esc: Back",
        AppState::CompressionGraph => "j/k: Scroll │ r: Refresh │ Esc: Back",
        AppState::FileClassify => "a: Add Rule │ d: Delete │ r: Run │ Esc: Back",
        AppState::MetaEdit => "j/k: File │ Space: Select │ a: All │ x: Remove │ Esc: Back",
        AppState::ConfigIO => "e: Export │ i: Import │ Esc: Back",
        AppState::Plugins => "j/k: Select │ Space: Toggle │ r: Scan │ o: Open Dir │ Esc: Back",
        AppState::StatusbarCustom => "j/k: Item │ Space: Toggle │ Enter: Save │ Esc: Back",
        AppState::ImagePreview => "j/k: Scroll │ Esc: Back",
        AppState::SplitPane => "Tab: Switch │ ←→: Resize │ Esc: Back",
        AppState::QuickActions => "j/k: Nav │ Enter: Run │ Esc: Back",
        AppState::RecentFiles => "j/k: Nav │ Enter: Preview │ Esc: Back",
        AppState::TagSystem => "a: Add │ d: Delete │ j/k: Nav │ Esc: Back",
        AppState::SideBySideDiff => "j/k: Scroll │ Tab: Focus │ Esc: Back",
        AppState::FileTreeView => "j/k: Nav │ Enter: Toggle │ Esc: Back",
        AppState::RenamePattern => "a: Add │ Tab: Field │ p: Preview │ Esc: Back",
        AppState::Timeline => "j/k: Scroll │ Esc: Back",
        AppState::CommandPalette => "Type to search │ Enter: Select │ Esc: Back",
        AppState::NotificationCenter => "j/k: Nav │ r: Read │ Esc: Back",
        AppState::ExportReport => "←→: Format │ Enter: Export │ Esc: Back",
        AppState::SimilarImages => "j/k: Group │ h/l: File │ +/-: Threshold │ s: Scan │ d: Delete │ Esc: Back",
    };
    let footer = Paragraph::new(footer_text)
        .style(Style::default().fg(theme.muted))
        .block(Block::default().borders(Borders::ALL).title("Controls"));
    f.render_widget(footer, area);
}

fn render_info_bar(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme();
    let now = chrono::Local::now().format("%H:%M:%S");
    let mem = app.sys_info.used_memory() / 1024 / 1024;
    let total_mem = app.sys_info.total_memory() / 1024 / 1024;
    let retry_info = if app.retry_count > 0 { format!(" │ Retry: {}", app.retry_count) } else { String::new() };
    let watch_info = if app.watch_active { format!(" │ Watched: {}", app.watch_processed) } else { String::new() };
    let filter_info = if !app.filter.name_pattern.is_empty() || app.filter.min_size_kb > 0 {
        format!(" │ Filter: ON")
    } else {
        String::new()
    };
    let history_info = format!(
        " Runs: {} │ Files: {} │ Mem: {}/{}MB{}{}{} │ Time: {}",
        app.history.total_runs,
        app.history.total_files_processed,
        mem, total_mem,
        retry_info, watch_info, filter_info,
        now
    );
    let bar = Paragraph::new(history_info)
        .style(Style::default().fg(theme.muted));
    f.render_widget(bar, area);
}

fn render_menu(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme();

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(45), Constraint::Percentage(55)])
        .split(area);

    let items: Vec<ListItem> = app
        .menu_items
        .iter()
        .enumerate()
        .map(|(i, item)| {
            let prefix = if i == app.selected { "▶ " } else { "  " };
            let num = format!("[{}] ", i + 1);
            let style = if i == app.selected {
                Style::default().fg(theme.accent).bg(theme.bg_highlight).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{}{}{}", prefix, num, item.label()), style),
            ]))
        })
        .collect();

    let menu_list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Menu"));
    f.render_widget(menu_list, chunks[0]);

    // Right panel: description + stats
    let selected_item = app.menu_items[app.selected];
    let mut desc_lines = vec![
        Line::from(vec![
            Span::styled(selected_item.label(), Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(""),
        Line::from(Span::raw(selected_item.description())),
        Line::from(""),
    ];

    // Destination info
    let dest_path = PathBuf::from(&app.config.dest);
    if dest_path.exists() {
        if let Ok(entries) = fs::read_dir(&dest_path) {
            let files: Vec<_> = entries.flatten().filter(|e| e.path().is_file()).collect();
            let total_size: u64 = files.iter()
                .filter_map(|e| fs::metadata(e.path()).ok())
                .map(|m| m.len())
                .sum();
            desc_lines.push(Line::from(Span::styled(
                format!("Destination: {} ({} files, {})", app.config.dest, files.len(), format_size(total_size)),
                Style::default().fg(theme.muted),
            )));
        }
    } else {
        desc_lines.push(Line::from(Span::styled(
            format!("Destination: {} (not created yet)", app.config.dest),
            Style::default().fg(theme.muted),
        )));
    }

    // History stats
    desc_lines.push(Line::from(""));
    desc_lines.push(Line::from(Span::styled(
        "── History ──",
        Style::default().fg(theme.primary),
    )));
    desc_lines.push(Line::from(format!("  Total runs: {}", app.history.total_runs)));
    desc_lines.push(Line::from(format!("  Files processed: {}", app.history.total_files_processed)));
    if let Some(last) = app.history.entries.last() {
        desc_lines.push(Line::from(format!("  Last run: {} ({} files, {})",
            last.timestamp, last.files_processed, format_duration(last.duration_secs))));
    }

    // Dry run indicator
    if app.dry_run {
        desc_lines.push(Line::from(""));
        desc_lines.push(Line::from(Span::styled(
            "⚠ DRY RUN MODE ACTIVE",
            Style::default().fg(theme.warning).add_modifier(Modifier::BOLD),
        )));
    }

    let desc = Paragraph::new(desc_lines)
        .block(Block::default().borders(Borders::ALL).title("Details"))
        .wrap(ratatui::widgets::Wrap { trim: true });
    f.render_widget(desc, chunks[1]);
}

fn render_step_select(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme();

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(60), Constraint::Percentage(40)])
        .split(area);

    // Step list with checkboxes
    let items: Vec<ListItem> = FULL_STEP_LABELS
        .iter()
        .enumerate()
        .map(|(i, label)| {
            let check = if app.step_enabled[i] { "☑" } else { "☐" };
            let prefix = if i == app.step_selected { "▶ " } else { "  " };
            let style = if i == app.step_selected {
                Style::default().fg(theme.accent).bg(theme.bg_highlight).add_modifier(Modifier::BOLD)
            } else if app.step_enabled[i] {
                Style::default().fg(theme.success)
            } else {
                Style::default().fg(theme.muted)
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{}{} {}", prefix, check, label), style),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Steps (Space to toggle)"));
    f.render_widget(list, chunks[0]);

    // Summary panel
    let enabled_count = app.step_enabled.iter().filter(|&&e| e).count();
    let summary_lines = vec![
        Line::from(vec![
            Span::styled("Enabled steps: ", Style::default()),
            Span::styled(format!("{}/{}", enabled_count, FULL_STEP_LABELS.len()),
                Style::default().fg(theme.warning).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(""),
        Line::from(Span::styled("Controls:", Style::default().fg(theme.primary).add_modifier(Modifier::BOLD))),
        Line::from("  Space  — Toggle step"),
        Line::from("  a      — Toggle all"),
        Line::from("  Enter  — Preview changes"),
        Line::from("  Esc    — Back to menu"),
    ];
    let summary = Paragraph::new(summary_lines)
        .block(Block::default().borders(Borders::ALL).title("Summary"))
        .wrap(ratatui::widgets::Wrap { trim: true });
    f.render_widget(summary, chunks[1]);
}

fn render_preview(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(area);

    // Info bar
    let info = Paragraph::new(format!(
        "  {} rename candidates │ {} total files │ {} total size",
        app.preview_items.len(),
        app.preview_file_count,
        format_size(app.preview_total_size)
    ))
    .style(Style::default().fg(theme.warning))
    .block(Block::default().borders(Borders::ALL).title("File Summary"));
    f.render_widget(info, chunks[0]);

    // Preview list
    if app.preview_items.is_empty() {
        let empty = Paragraph::new("  No rename changes detected")
            .style(Style::default().fg(theme.muted))
            .block(Block::default().borders(Borders::ALL).title("Changes (Enter to start)"));
        f.render_widget(empty, chunks[1]);
    } else {
        let visible_height = chunks[1].height.saturating_sub(2) as usize;
        let start = app.preview_scroll;
        let end = (start + visible_height).min(app.preview_items.len());

        let items: Vec<ListItem> = app.preview_items[start..end]
            .iter()
            .map(|(old, new)| {
                ListItem::new(Line::from(vec![
                    Span::styled(format!("  {}", old), Style::default().fg(theme.error)),
                    Span::styled(" → ", Style::default().fg(theme.muted)),
                    Span::styled(new.clone(), Style::default().fg(theme.success)),
                ]))
            })
            .collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(
                format!("Changes ({}-{}/{}, Enter to start)", start + 1, end, app.preview_items.len())
            ));
        f.render_widget(list, chunks[1]);
    }
}

fn render_processing(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),  // Current step
            Constraint::Length(3),  // Main progress
            Constraint::Length(3),  // Sub-progress
            Constraint::Length(3),  // Stats (speed, ETA, files)
            Constraint::Min(0),    // Log
        ])
        .split(area);

    // Current step with spinner
    let step_text = app.current_step.lock().unwrap().clone();
    let sp = SPINNER_CHARS[app.spinner_idx];
    let step = Paragraph::new(format!("{} {}", sp, step_text))
        .style(Style::default().fg(theme.warning).add_modifier(Modifier::BOLD))
        .block(Block::default().borders(Borders::ALL).title("Current Step"));
    f.render_widget(step, chunks[0]);

    // Main progress bar with visual gauge
    let progress_val = *app.progress.lock().unwrap();
    let gauge_width = (chunks[1].width.saturating_sub(20)) as usize;
    let bar = make_gauge_bar(progress_val, gauge_width);
    let gauge = Gauge::default()
        .block(Block::default().borders(Borders::ALL).title("Progress"))
        .gauge_style(Style::default().fg(theme.primary).bg(Color::Black))
        .ratio(progress_val)
        .label(format!("{} {:.0}%", bar, progress_val * 100.0));
    f.render_widget(gauge, chunks[1]);

    // Sub-progress (per-step visual bars)
    let sp = app.step_progress.lock().unwrap().clone();
    let sub_bars: Vec<Span> = sp.iter().enumerate().map(|(i, &v)| {
        let mini_bar = make_gauge_bar(v, 8);
        let label = match i {
            0 => "Mv",
            1 => "Ded",
            2 => "Ref",
            3 => "Ren",
            4 => "JXL",
            _ => "??",
        };
        let color = if v >= 1.0 { theme.success } else if v > 0.0 { theme.primary } else { theme.muted };
        Span::styled(format!(" {}[{}] ", label, mini_bar), Style::default().fg(color))
    }).collect();
    let sub_progress = Paragraph::new(Line::from(sub_bars))
        .block(Block::default().borders(Borders::ALL).title("Steps"));
    f.render_widget(sub_progress, chunks[2]);

    // Stats: speed, ETA, files processed
    let elapsed = app.start_time.lock().unwrap()
        .map(|t| t.elapsed().as_secs_f64())
        .unwrap_or(0.0);
    let processed = *app.files_processed.lock().unwrap();
    let speed = if elapsed > 0.0 { processed as f64 / elapsed } else { 0.0 };
    let remaining = if progress_val > 0.0 && progress_val < 1.0 {
        elapsed * (1.0 - progress_val) / progress_val
    } else { 0.0 };

    let detail_text = app.progress_detail.lock().unwrap().clone();
    let stats_line = Line::from(vec![
        Span::styled(format!("  {} ", detail_text), Style::default().fg(Color::White)),
        Span::styled(format!("│ {:.1} files/s ", speed), Style::default().fg(theme.primary)),
        Span::styled(format!("│ ETA: {} ", format_duration(remaining)), Style::default().fg(theme.warning)),
        Span::styled(format!("│ {} files", processed), Style::default().fg(theme.success)),
    ]);
    let stats = Paragraph::new(stats_line)
        .block(Block::default().borders(Borders::ALL).title("Stats"));
    f.render_widget(stats, chunks[3]);

    // Log
    render_log(f, app, chunks[4], &theme);
}

fn render_done(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),  // Status + stats
            Constraint::Length(if app.errors.lock().unwrap().is_empty() { 0 } else { 8 }),
            Constraint::Min(0),    // Log
        ])
        .split(area);

    // Completion message with stats
    let has_errors = !app.errors.lock().unwrap().is_empty();
    let elapsed = app.start_time.lock().unwrap()
        .map(|t| t.elapsed().as_secs_f64())
        .unwrap_or(0.0);
    let processed = *app.files_processed.lock().unwrap();

    let status_text = if has_errors {
        format!("⚠ Process completed with errors")
    } else {
        format!("✓ Process completed successfully!")
    };
    let status_color = if has_errors { theme.warning } else { theme.success };

    let stats_text = format!(
        "  Files: {} │ Duration: {} │ Errors: {} │ Theme: {}",
        processed, format_duration(elapsed), app.errors.lock().unwrap().len(), THEME_NAMES[app.theme_idx]
    );

    let done_lines = vec![
        Line::from(Span::styled(status_text, Style::default().fg(status_color).add_modifier(Modifier::BOLD))),
        Line::from(""),
        Line::from(Span::styled(stats_text, Style::default().fg(theme.muted))),
    ];
    let done_msg = Paragraph::new(done_lines)
        .block(Block::default().borders(Borders::ALL).title("Status"));
    f.render_widget(done_msg, chunks[0]);

    // Error list
    if has_errors {
        let errors = app.errors.lock().unwrap();
        let err_items: Vec<ListItem> = errors
            .iter()
            .map(|e| ListItem::new(Line::from(Span::styled(
                format!("  ✗ {}", e),
                Style::default().fg(theme.error),
            ))))
            .collect();
        let err_list = List::new(err_items)
            .block(Block::default().borders(Borders::ALL).title(
                format!("Errors ({})", errors.len())
            ));
        f.render_widget(err_list, chunks[1]);
    }

    // Log (with search filter)
    render_log(f, app, chunks[2], &theme);
}

fn render_settings(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme();

    let settings_items = vec![
        ("Twitter Source", app.config.twitter_src.clone()),
        ("Download Source", app.config.download_src.clone()),
        ("Destination", app.config.dest.clone()),
        ("Reference", app.config.reference.clone()),
        ("Days to Check", app.config.days_to_check.to_string()),
        ("Min File Size (KB)", app.config.min_file_size_kb.to_string()),
        ("Max Workers", app.config.max_workers.to_string()),
        ("Extensions", app.config.image_extensions.join(", ")),
        ("Back", String::new()),
    ];

    let items: Vec<ListItem> = settings_items
        .iter()
        .enumerate()
        .map(|(i, (label, value))| {
            let prefix = if i == app.settings_selected { "▶ " } else { "  " };
            let style = if i == app.settings_selected {
                Style::default().fg(theme.accent).bg(theme.bg_highlight).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let text = if value.is_empty() {
                format!("{}{}", prefix, label)
            } else {
                format!("{}{}: {}", prefix, label, value)
            };
            ListItem::new(Line::from(Span::styled(text, style)))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Settings (Enter to save & back)"));
    f.render_widget(list, area);
}

fn render_log(f: &mut Frame, app: &mut App, area: Rect, theme: &Theme) {
    let logs = app.logs.lock().unwrap();

    let title = if app.search_mode {
        format!("Log — Search: {}_", app.search_query)
    } else if !app.filtered_log_indices.is_empty() {
        format!("Log — Filtered: {} matches", app.filtered_log_indices.len())
    } else {
        "Log".to_string()
    };

    let visible_height = area.height.saturating_sub(2) as usize;

    let log_items: Vec<ListItem> = if app.search_mode || !app.filtered_log_indices.is_empty() {
        // Show filtered results
        let indices = if app.search_mode {
            let query = app.search_query.to_lowercase();
            logs.iter().enumerate()
                .filter(|(_, msg)| msg.to_lowercase().contains(&query))
                .map(|(i, _)| i)
                .collect::<Vec<_>>()
        } else {
            app.filtered_log_indices.clone()
        };

        let offset = indices.len().saturating_sub(visible_height);
        indices[offset..].iter()
            .filter_map(|&i| logs.get(i))
            .map(|msg| {
                let style = log_line_style(msg, theme);
                ListItem::new(Line::from(Span::styled(msg.clone(), style)))
            })
            .collect()
    } else {
        let offset = logs.len().saturating_sub(visible_height);
        logs[offset..]
            .iter()
            .map(|msg| {
                let style = log_line_style(msg, theme);
                ListItem::new(Line::from(Span::styled(msg.clone(), style)))
            })
            .collect()
    };

    let log_list = List::new(log_items)
        .block(Block::default().borders(Borders::ALL).title(title));
    f.render_widget(log_list, area);
}

fn log_line_style(msg: &str, theme: &Theme) -> Style {
    if msg.starts_with("===") {
        Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)
    } else if msg.contains("Error") || msg.contains("✗") {
        Style::default().fg(theme.error)
    } else if msg.contains("✓") {
        Style::default().fg(theme.success)
    } else if msg.starts_with("[STEP") {
        Style::default().fg(theme.warning)
    } else if msg.contains("DRY RUN") {
        Style::default().fg(theme.warning).add_modifier(Modifier::BOLD)
    } else {
        Style::default().fg(Color::White)
    }
}

// ============================================================
// Feature #14: Help Screen
// ============================================================

fn render_help(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme();
    let help_lines = vec![
        "═══════════════════════════════════════════════════════════",
        "  io-tool — Key Bindings Reference",
        "═══════════════════════════════════════════════════════════",
        "",
        "  Global Keys (work in most screens):",
        "  ─────────────────────────────────────",
        "    t         Cycle theme (6 colors)",
        "    d         Toggle dry run mode",
        "    u         Undo last rename",
        "    ?         Show this help screen",
        "    Ctrl+P    Pause/Resume processing",
        "    Ctrl+E    Export log to file",
        "",
        "  Menu Navigation:",
        "  ─────────────────────────────────────",
        "    j/k       Navigate up/down",
        "    1-9       Quick select menu item",
        "    Enter     Run selected item",
        "    q/Esc     Quit",
        "",
        "  Full Process:",
        "  ─────────────────────────────────────",
        "    Space     Toggle step on/off",
        "    a         Toggle all steps",
        "    Enter     Preview & confirm",
        "",
        "  Preview:",
        "  ─────────────────────────────────────",
        "    j/k       Scroll one line",
        "    PgUp/Dn   Scroll page",
        "    Home/End  Jump to start/end",
        "    i         Show file info panel",
        "    Enter     Start processing",
        "",
        "  Processing:",
        "  ─────────────────────────────────────",
        "    Ctrl+P    Pause/Resume",
        "    /         Search log",
        "",
        "  Done Screen:",
        "  ─────────────────────────────────────",
        "    r         Back to menu",
        "    /         Search log",
        "    Ctrl+E    Export log",
        "",
        "  Menu Features:",
        "  ─────────────────────────────────────",
        "    f         Filter & Sort settings",
        "    s         Quick cycle sort order",
        "    S         Statistics dashboard",
        "    p         Config profiles",
        "    b         Batch queue",
        "    w         Watch mode",
        "",
        "  Duplicate Groups:",
        "  ─────────────────────────────────────",
        "    j/k       Select group",
        "    h/l       Select file in group",
        "    Space     Mark as keep",
        "    x         Delete non-selected",
        "",
        "  JXL Settings:",
        "  ─────────────────────────────────────",
        "    h/l       Adjust quality (-5/+5)",
        "    Space     Toggle lossless",
        "    Enter     Save settings",
        "",
        "  Watch Mode:",
        "  ─────────────────────────────────────",
        "    w         Toggle watch on/off",
        "    a         Add watch directory",
        "    Enter     Confirm add",
    ];

    let start = app.help_scroll.min(help_lines.len().saturating_sub(1));
    let visible = area.height.saturating_sub(2) as usize;
    let end = (start + visible).min(help_lines.len());

    let items: Vec<ListItem> = help_lines[start..end].iter()
        .map(|line| {
            let style = if line.contains("═══") {
                Style::default().fg(theme.primary)
            } else if line.contains("───") {
                Style::default().fg(theme.muted)
            } else if line.starts_with("  Global") || line.starts_with("  Menu") || line.starts_with("  Full") 
                || line.starts_with("  Preview") || line.starts_with("  Processing") || line.starts_with("  Done")
                || line.starts_with("  Duplicate") || line.starts_with("  JXL") || line.starts_with("  Watch") {
                Style::default().fg(theme.warning).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(Line::from(Span::styled(*line, style)))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(
            format!("Help ({}-{}/{})", start + 1, end, help_lines.len())
        ));
    f.render_widget(list, area);
}

// ============================================================
// Feature #4: Batch Queue
// ============================================================

fn render_batch_queue(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(area);

    // Queue list
    let items: Vec<ListItem> = app.batch_queue.iter().enumerate()
        .map(|(i, job)| {
            let prefix = if i == app.batch_selected { "▶ " } else { "  " };
            let status_icon = match job.status.as_str() {
                "pending" => "○",
                "processing" => "●",
                "done" => "✓",
                "error" => "✗",
                _ => "?",
            };
            let status_color = match job.status.as_str() {
                "pending" => theme.muted,
                "processing" => theme.warning,
                "done" => theme.success,
                "error" => theme.error,
                _ => Color::White,
            };
            let style = if i == app.batch_selected {
                Style::default().fg(theme.accent).bg(theme.bg_highlight).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(status_color)
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{}{} {} [{}]", prefix, status_icon, job.path, job.status), style),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(
            format!("Batch Queue ({} jobs)", app.batch_queue.len())
        ));
    f.render_widget(list, chunks[0]);

    // Input area
    if app.batch_adding {
        let input = Paragraph::new(format!("  Path: {}_", app.batch_input))
            .style(Style::default().fg(theme.warning))
            .block(Block::default().borders(Borders::ALL).title("Add Directory (Enter to confirm)"));
        f.render_widget(input, chunks[1]);
    } else {
        let help = Paragraph::new("  a: Add directory │ d: Delete │ Enter: Process all │ Esc: Back")
            .style(Style::default().fg(theme.muted))
            .block(Block::default().borders(Borders::ALL).title("Actions"));
        f.render_widget(help, chunks[1]);
    }
}

// ============================================================
// Feature #3: Duplicate Groups
// ============================================================

fn render_duplicate_groups(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme();

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    // Group list
    let items: Vec<ListItem> = app.duplicate_groups.iter().enumerate()
        .map(|(i, group)| {
            let prefix = if i == app.dup_group_selected { "▶ " } else { "  " };
            let style = if i == app.dup_group_selected {
                Style::default().fg(theme.accent).bg(theme.bg_highlight).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let total_size: u64 = group.files.iter().map(|(_, s)| s).sum();
            ListItem::new(Line::from(Span::styled(
                format!("{}Group #{}: {} files ({})", prefix, i + 1, group.files.len(), format_size(total_size)),
                style,
            )))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(
            format!("Duplicate Groups ({})", app.duplicate_groups.len())
        ));
    f.render_widget(list, chunks[0]);

    // File list in selected group
    if app.dup_group_selected < app.duplicate_groups.len() {
        let group = &app.duplicate_groups[app.dup_group_selected];
        let items: Vec<ListItem> = group.files.iter().enumerate()
            .map(|(i, (path, size))| {
                let keep = if i == group.selected { "★ KEEP" } else { "  delete" };
                let prefix = if i == app.dup_file_selected { "▶ " } else { "  " };
                let style = if i == group.selected {
                    Style::default().fg(theme.success)
                } else if i == app.dup_file_selected {
                    Style::default().fg(theme.accent).bg(theme.bg_highlight)
                } else {
                    Style::default().fg(theme.error)
                };
                let file_name = PathBuf::from(path).file_name().unwrap_or_default().to_string_lossy().to_string();
                ListItem::new(Line::from(Span::styled(
                    format!("{}{} {} ({})", prefix, keep, file_name, format_size(*size)),
                    style,
                )))
            })
            .collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title(
                format!("Files (hash: {})", group.hash)
            ));
        f.render_widget(list, chunks[1]);
    } else {
        let empty = Paragraph::new("  No duplicate groups found")
            .style(Style::default().fg(theme.muted))
            .block(Block::default().borders(Borders::ALL).title("Files"));
        f.render_widget(empty, chunks[1]);
    }
}

// ============================================================
// Feature #8: Statistics Dashboard
// ============================================================

fn render_stats(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(7),  // Summary
            Constraint::Length(12), // Bar chart
            Constraint::Min(0),    // History list
        ])
        .split(area);

    // Summary
    let summary_lines = vec![
        Line::from(vec![
            Span::styled("  Total Runs: ", Style::default().fg(theme.muted)),
            Span::styled(format!("{}", app.history.total_runs), Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("  Total Files Processed: ", Style::default().fg(theme.muted)),
            Span::styled(format!("{}", app.history.total_files_processed), Style::default().fg(theme.success).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("  Total Files Removed: ", Style::default().fg(theme.muted)),
            Span::styled(format!("{}", app.history.total_files_removed), Style::default().fg(theme.error).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("  Profiles: ", Style::default().fg(theme.muted)),
            Span::styled(format!("{}", app.config.profiles.len()), Style::default().fg(theme.warning)),
        ]),
    ];
    let summary = Paragraph::new(summary_lines)
        .block(Block::default().borders(Borders::ALL).title("Summary"));
    f.render_widget(summary, chunks[0]);

    // Bar chart of recent runs
    let stats_data = app.get_stats_data();
    if !stats_data.is_empty() {
        let max_val = stats_data.iter().map(|(_, v)| *v).max().unwrap_or(1);
        let bar_data: Vec<(&str, u64)> = stats_data.iter()
            .map(|(label, val)| {
                (label.as_str(), *val)
            })
            .collect();

        let barchart = BarChart::default()
            .block(Block::default().borders(Borders::ALL).title("Files per Run (recent)"))
            .data(&bar_data)
            .bar_width(3)
            .bar_style(Style::default().fg(theme.primary))
            .value_style(Style::default().fg(Color::White))
            .max(max_val);
        f.render_widget(barchart, chunks[1]);
    } else {
        let empty = Paragraph::new("  No history yet")
            .style(Style::default().fg(theme.muted))
            .block(Block::default().borders(Borders::ALL).title("Files per Run"));
        f.render_widget(empty, chunks[1]);
    }

    // History list
    let history_items: Vec<ListItem> = app.history.entries.iter().rev()
        .skip(app.stats_scroll)
        .take((chunks[2].height.saturating_sub(2)) as usize)
        .map(|entry| {
            ListItem::new(Line::from(vec![
                Span::styled(format!("  {} ", entry.timestamp), Style::default().fg(theme.muted)),
                Span::styled(format!("{} files ", entry.files_processed), Style::default().fg(theme.success)),
                Span::styled(format!("{} ", format_duration(entry.duration_secs)), Style::default().fg(theme.primary)),
                Span::styled(format!("{} errors", entry.errors), 
                    if entry.errors > 0 { Style::default().fg(theme.error) } else { Style::default().fg(theme.muted) }),
            ]))
        })
        .collect();

    let list = List::new(history_items)
        .block(Block::default().borders(Borders::ALL).title("History"));
    f.render_widget(list, chunks[2]);
}

// ============================================================
// Feature #9: Profiles
// ============================================================

fn render_profiles(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(0),
            Constraint::Length(3),
        ])
        .split(area);

    // Profile list + actions
    let mut items: Vec<ListItem> = app.config.profiles.iter().enumerate()
        .map(|(i, profile)| {
            let prefix = if i == app.profile_selected { "▶ " } else { "  " };
            let style = if i == app.profile_selected {
                Style::default().fg(theme.accent).bg(theme.bg_highlight).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(Line::from(Span::styled(
                format!("{}  {} (dest: {})", prefix, profile.name, profile.config.dest),
                style,
            )))
        })
        .collect();

    // Add action items
    let actions = ["Clear History", "Clear Undo Log"];
    for (i, action) in actions.iter().enumerate() {
        let idx = app.config.profiles.len() + i;
        let prefix = if idx == app.profile_selected { "▶ " } else { "  " };
        let style = if idx == app.profile_selected {
            Style::default().fg(theme.warning).bg(theme.bg_highlight).add_modifier(Modifier::BOLD)
        } else {
            Style::default().fg(theme.muted)
        };
        items.push(ListItem::new(Line::from(Span::styled(
            format!("{}  {}", prefix, action),
            style,
        ))));
    }

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Profiles (Enter to load, d to delete)"));
    f.render_widget(list, chunks[0]);

    // Input area
    if app.profile_adding {
        let input = Paragraph::new(format!("  Name: {}_", app.profile_input))
            .style(Style::default().fg(theme.warning))
            .block(Block::default().borders(Borders::ALL).title("New Profile (Enter to save)"));
        f.render_widget(input, chunks[1]);
    } else {
        let help = Paragraph::new("  a: Add profile │ d: Delete │ Enter: Load/Execute │ Esc: Back")
            .style(Style::default().fg(theme.muted))
            .block(Block::default().borders(Borders::ALL).title("Actions"));
        f.render_widget(help, chunks[1]);
    }
}

// ============================================================
// Feature #19: JXL Settings
// ============================================================

fn render_jxl_settings(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme();

    let settings_items = vec![
        ("Quality", format!("{}", app.config.jxl_quality)),
        ("Lossless", format!("{}", app.config.jxl_lossless)),
        ("Save & Back", String::new()),
    ];

    let items: Vec<ListItem> = settings_items.iter().enumerate()
        .map(|(i, (label, value))| {
            let prefix = if i == app.settings_selected { "▶ " } else { "  " };
            let style = if i == app.settings_selected {
                Style::default().fg(theme.accent).bg(theme.bg_highlight).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let text = if value.is_empty() {
                format!("{}{}", prefix, label)
            } else if *label == "Quality" {
                let bar = make_gauge_bar(app.config.jxl_quality as f64 / 100.0, 20);
                format!("{}{}: {} [{}]", prefix, label, value, bar)
            } else {
                format!("{}{}: {}", prefix, label, value)
            };
            ListItem::new(Line::from(Span::styled(text, style)))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("JXL Settings (h/l to adjust, Space to toggle)"));
    f.render_widget(list, area);
}

// ============================================================
// Feature #20: Watch Mode
// ============================================================

fn render_watch_mode(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(5),  // Status
            Constraint::Min(0),    // Watch dirs
            Constraint::Length(3), // Input
        ])
        .split(area);

    // Status
    let status = if app.watch_active { "ACTIVE" } else { "INACTIVE" };
    let status_color = if app.watch_active { theme.success } else { theme.muted };
    let status_lines = vec![
        Line::from(vec![
            Span::styled("  Status: ", Style::default().fg(theme.muted)),
            Span::styled(status, Style::default().fg(status_color).add_modifier(Modifier::BOLD)),
        ]),
        Line::from(vec![
            Span::styled("  Files auto-processed: ", Style::default().fg(theme.muted)),
            Span::styled(format!("{}", app.watch_processed), Style::default().fg(theme.success)),
        ]),
        Line::from(vec![
            Span::styled("  Interval: ", Style::default().fg(theme.muted)),
            Span::styled(format!("{}s", app.config.watch_interval_secs), Style::default().fg(theme.primary)),
        ]),
    ];
    let status_p = Paragraph::new(status_lines)
        .block(Block::default().borders(Borders::ALL).title("Watch Status (w to toggle)"));
    f.render_widget(status_p, chunks[0]);

    // Watch directories
    let dir_items: Vec<ListItem> = app.config.watch_dirs.iter().enumerate()
        .map(|(i, dir)| {
            ListItem::new(Line::from(Span::styled(
                format!("  {}  {}", i + 1, dir),
                Style::default().fg(Color::White),
            )))
        })
        .collect();

    let dir_list = List::new(dir_items)
        .block(Block::default().borders(Borders::ALL).title(
            format!("Watch Directories ({})", app.config.watch_dirs.len())
        ));
    f.render_widget(dir_list, chunks[1]);

    // Input
    if app.batch_adding {
        let input = Paragraph::new(format!("  Directory: {}_", app.batch_input))
            .style(Style::default().fg(theme.warning))
            .block(Block::default().borders(Borders::ALL).title("Add Directory (Enter to confirm)"));
        f.render_widget(input, chunks[2]);
    } else {
        let help = Paragraph::new("  a: Add directory │ w: Toggle watch │ Esc: Back")
            .style(Style::default().fg(theme.muted))
            .block(Block::default().borders(Borders::ALL).title("Actions"));
        f.render_widget(help, chunks[2]);
    }
}

// ============================================================
// Feature #6, #7: Filter & Sort
// ============================================================

fn render_filter_sort(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme();

    let sort_name = match app.sort_config.field {
        SortField::Name => "Name",
        SortField::Size => "Size",
        SortField::Date => "Date",
        SortField::Type => "Type",
    };
    let sort_dir = if app.sort_config.ascending { "↑ Asc" } else { "↓ Desc" };

    let settings_items = vec![
        ("Extensions", app.filter.extensions.join(", ")),
        ("Min Size (KB)", format!("{}", app.filter.min_size_kb)),
        ("Max Size (KB)", format!("{}", app.filter.max_size_kb)),
        ("Name Pattern", app.filter.name_pattern.clone()),
        ("Sort By", format!("{} {}", sort_name, sort_dir)),
        ("Apply & Back", String::new()),
    ];

    let items: Vec<ListItem> = settings_items.iter().enumerate()
        .map(|(i, (label, value))| {
            let prefix = if i == app.filter_selected { "▶ " } else { "  " };
            let style = if i == app.filter_selected {
                Style::default().fg(theme.accent).bg(theme.bg_highlight).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let text = if value.is_empty() {
                format!("{}{}", prefix, label)
            } else {
                format!("{}{}: {}", prefix, label, value)
            };
            ListItem::new(Line::from(Span::styled(text, style)))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Filter & Sort (h/l to adjust, Enter to apply)"));
    f.render_widget(list, area);
}

// ============================================================
// Feature #11: Info Panel
// ============================================================

fn render_info_panel(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme();

    let chunks = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(40), Constraint::Percentage(60)])
        .split(area);

    // File list
    let visible_height = chunks[0].height.saturating_sub(2) as usize;
    let start = if app.info_selected >= visible_height { app.info_selected - visible_height + 1 } else { 0 };
    let end = (start + visible_height).min(app.preview_items.len());

    let items: Vec<ListItem> = app.preview_items[start..end].iter().enumerate()
        .map(|(i, (old, _))| {
            let idx = start + i;
            let prefix = if idx == app.info_selected { "▶ " } else { "  " };
            let style = if idx == app.info_selected {
                Style::default().fg(theme.accent).bg(theme.bg_highlight).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(Line::from(Span::styled(format!("{}{}", prefix, old), style)))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Files"));
    f.render_widget(list, chunks[0]);

    // Info panel
    if app.info_selected < app.preview_items.len() {
        let (old, _) = &app.preview_items[app.info_selected];
        let path = PathBuf::from(&app.config.dest).join(old);
        let info = app.get_file_info(&path.to_string_lossy());

        let items: Vec<ListItem> = info.iter()
            .map(|(key, value)| {
                ListItem::new(Line::from(vec![
                    Span::styled(format!("  {}: ", key), Style::default().fg(theme.primary).add_modifier(Modifier::BOLD)),
                    Span::styled(value.clone(), Style::default().fg(Color::White)),
                ]))
            })
            .collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title("File Info"));
        f.render_widget(list, chunks[1]);
    } else {
        let empty = Paragraph::new("  Select a file to view info")
            .style(Style::default().fg(theme.muted))
            .block(Block::default().borders(Borders::ALL).title("File Info"));
        f.render_widget(empty, chunks[1]);
    }
}

// ============================================================
// Feature #13: Confirm Dialog
// ============================================================

fn render_confirm_dialog(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme();

    let action_text = match &app.confirm_action {
        Some(ConfirmAction::StartProcessing) => "Start processing with selected steps?",
        Some(ConfirmAction::ClearHistory) => "Clear all processing history?",
        Some(ConfirmAction::ClearUndo) => "Clear undo log?",
        None => "Are you sure?",
    };

    let dialog_lines = vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("  {}", action_text),
            Style::default().fg(theme.warning).add_modifier(Modifier::BOLD),
        )),
        Line::from(""),
        Line::from(vec![
            Span::styled(
                format!("  {} Yes", if app.confirm_yes { "▶" } else { " " }),
                if app.confirm_yes { Style::default().fg(theme.success).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::White) },
            ),
            Span::raw("    "),
            Span::styled(
                format!("{} No", if !app.confirm_yes { "▶" } else { " " }),
                if !app.confirm_yes { Style::default().fg(theme.error).add_modifier(Modifier::BOLD) } else { Style::default().fg(Color::White) },
            ),
        ]),
        Line::from(""),
        Line::from(Span::styled(
            "  j/k: Toggle │ y/Enter: Confirm │ n/Esc: Cancel",
            Style::default().fg(theme.muted),
        )),
    ];

    // Center the dialog
    let dialog_height = dialog_lines.len() as u16 + 2;
    let dialog_width = 50.min(area.width);
    let x = (area.width.saturating_sub(dialog_width)) / 2;
    let y = (area.height.saturating_sub(dialog_height)) / 2;
    let dialog_area = Rect::new(area.x + x, area.y + y, dialog_width, dialog_height);

    // Draw background
    let bg = Block::default()
        .borders(Borders::ALL)
        .border_style(Style::default().fg(theme.warning))
        .title(" Confirm ");
    f.render_widget(bg, dialog_area);

    let inner = Rect::new(dialog_area.x + 1, dialog_area.y + 1, dialog_area.width - 2, dialog_area.height - 2);
    let dialog = Paragraph::new(dialog_lines);
    f.render_widget(dialog, inner);
}

// ============================================================
// New Feature Render Functions
// ============================================================

// New #1: Size Comparison
fn render_size_compare(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    // Summary
    let total_orig: u64 = app.size_comparisons.iter().map(|c| c.original_size).sum();
    let total_conv: u64 = app.size_comparisons.iter().map(|c| c.converted_size).sum();
    let total_reduction = if total_orig > 0 { (1.0 - total_conv as f64 / total_orig as f64) * 100.0 } else { 0.0 };
    let summary = Paragraph::new(format!(
        "  Total: {} → {} (↓{:.1}%)",
        format_size(total_orig), format_size(total_conv), total_reduction
    ))
    .style(Style::default().fg(theme.success))
    .block(Block::default().borders(Borders::ALL).title("Summary"));
    f.render_widget(summary, chunks[0]);

    // Table
    let visible = chunks[1].height.saturating_sub(2) as usize;
    let start = app.size_compare_scroll.min(app.size_comparisons.len().saturating_sub(visible));
    let end = (start + visible).min(app.size_comparisons.len());

    let header = format!("  {:<30} {:>10} {:>10} {:>8}", "Filename", "Original", "Converted", "Reduce");
    let mut lines = vec![
        Line::from(Span::styled(header, Style::default().fg(theme.primary).add_modifier(Modifier::BOLD))),
        Line::from(Span::styled("  ────────────────────────────────────────────────────────────────", Style::default().fg(theme.muted))),
    ];

    for comp in &app.size_comparisons[start..end] {
        let bar_len = (comp.reduction_pct / 5.0) as usize;
        let bar = "█".repeat(bar_len);
        let line = format!("  {:<30} {:>10} {:>10} {:>6.1}% {}",
            comp.filename, format_size(comp.original_size), format_size(comp.converted_size),
            comp.reduction_pct, bar);
        let color = if comp.reduction_pct > 50.0 { theme.success } else if comp.reduction_pct > 20.0 { theme.warning } else { theme.error };
        lines.push(Line::from(Span::styled(line, Style::default().fg(color))));
    }

    let list = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(
            format!("Size Comparison ({})", app.size_comparisons.len())
        ));
    f.render_widget(list, chunks[1]);
}

// New #3: Error Panel
fn render_error_panel(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme();

    let visible = area.height.saturating_sub(2) as usize;
    let start = app.error_scroll.min(app.error_details.len().saturating_sub(visible));
    let end = (start + visible).min(app.error_details.len());

    let mut lines = vec![];
    if app.error_details.is_empty() {
        lines.push(Line::from(Span::styled("  No errors recorded", Style::default().fg(theme.success))));
    } else {
        for err in &app.error_details[start..end] {
            lines.push(Line::from(vec![
                Span::styled(format!("  [{}] ", err.timestamp), Style::default().fg(theme.muted)),
                Span::styled(format!("{}: ", err.filename), Style::default().fg(theme.warning)),
                Span::styled(err.error_msg.clone(), Style::default().fg(theme.error)),
            ]));
        }
    }

    let list = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(
            format!("Error Details ({})", app.error_details.len())
        ));
    f.render_widget(list, area);
}

// New #4: Conversion Presets
fn render_presets(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme();

    let items: Vec<ListItem> = app.presets.iter().enumerate()
        .map(|(i, preset)| {
            let prefix = if i == app.preset_selected { "▶ " } else { "  " };
            let active = if i == app.active_preset { " ✓" } else { "" };
            let style = if i == app.preset_selected {
                Style::default().fg(theme.accent).bg(theme.bg_highlight).add_modifier(Modifier::BOLD)
            } else if i == app.active_preset {
                Style::default().fg(theme.success)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(Line::from(Span::styled(
                format!("{}  {} (q:{} lossless:{}) {}{}", prefix, preset.name, preset.quality, preset.lossless, preset.description, active),
                style,
            )))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Conversion Presets (Enter to apply)"));
    f.render_widget(list, area);
}

// New #9: Scheduler
fn render_scheduler(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)])
        .split(area);

    let items: Vec<ListItem> = app.scheduler_jobs.iter().enumerate()
        .map(|(i, job)| {
            let prefix = if i == app.scheduler_selected { "▶ " } else { "  " };
            let status = if job.enabled { "● ON" } else { "○ OFF" };
            let status_color = if job.enabled { theme.success } else { theme.muted };
            let style = if i == app.scheduler_selected {
                Style::default().fg(theme.accent).bg(theme.bg_highlight)
            } else {
                Style::default().fg(Color::White)
            };
            let days: Vec<&str> = job.days.iter().map(|d| match d {
                0 => "Sun", 1 => "Mon", 2 => "Tue", 3 => "Wed",
                4 => "Thu", 5 => "Fri", 6 => "Sat", _ => "?",
            }).collect();
            ListItem::new(Line::from(vec![
                Span::styled(format!("{}  {} ", prefix, status), Style::default().fg(status_color)),
                Span::styled(format!("{:02}:{:02} {} ", job.hour, job.minute, days.join(",")),
                    if app.scheduler_editing && i == app.scheduler_selected { Style::default().fg(theme.warning) } else { style }),
                Span::styled(format!(" {}", job.name), Style::default().fg(theme.primary)),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Process Scheduler"));
    f.render_widget(list, chunks[0]);

    let help = Paragraph::new("  a: Add │ e: Edit │ t: Toggle │ d: Delete │ Esc: Back")
        .style(Style::default().fg(theme.muted))
        .block(Block::default().borders(Borders::ALL).title("Actions"));
    f.render_widget(help, chunks[1]);
}

// New #10: History Export
fn render_history_export(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme();

    let formats = ["CSV (Excel compatible)", "JSON (structured data)"];
    let items: Vec<ListItem> = formats.iter().enumerate()
        .map(|(i, fmt)| {
            let prefix = if i == app.export_format { "▶ " } else { "  " };
            let style = if i == app.export_format {
                Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(Line::from(Span::styled(format!("{}  {}", prefix, fmt), style)))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Export Format (Enter to export)"));
    f.render_widget(list, area);
}

// New #11: Theme Editor
fn render_theme_editor(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme();

    let colors = [
        ("Primary", theme.primary),
        ("Secondary", theme._secondary),
        ("Accent", theme.accent),
        ("Success", theme.success),
        ("Warning", theme.warning),
        ("Error", theme.error),
        ("Background", theme.bg),
        ("Foreground", theme.fg),
        ("Muted", theme.muted),
        ("Highlight", theme.bg_highlight),
    ];

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)])
        .split(area);

    let items: Vec<ListItem> = colors.iter().enumerate()
        .map(|(i, (name, color))| {
            let prefix = if i == app.theme_edit_selected { "▶ " } else { "  " };
            let style = if i == app.theme_edit_selected {
                Style::default().fg(theme.accent).bg(theme.bg_highlight).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            let rgb = match color {
                Color::Rgb(r, g, b) => format!("({},{},{})", r, g, b),
                Color::Red => "(255,0,0)".to_string(),
                Color::Green => "(0,255,0)".to_string(),
                Color::Blue => "(0,0,255)".to_string(),
                Color::Yellow => "(255,255,0)".to_string(),
                Color::Cyan => "(0,255,255)".to_string(),
                Color::Magenta => "(255,0,255)".to_string(),
                Color::White => "(255,255,255)".to_string(),
                Color::Black => "(0,0,0)".to_string(),
                Color::Gray => "(128,128,128)".to_string(),
                Color::DarkGray => "(64,64,64)".to_string(),
                Color::LightRed => "(255,128,128)".to_string(),
                Color::LightGreen => "(128,255,128)".to_string(),
                Color::LightBlue => "(128,128,255)".to_string(),
                Color::LightYellow => "(255,255,128)".to_string(),
                Color::LightCyan => "(128,255,255)".to_string(),
                Color::LightMagenta => "(255,128,255)".to_string(),
                Color::Indexed(i) => format!("idx:{}", i),
                _ => "(?,?,?)".to_string(),
            };
            let _sample = Block::default().style(Style::default().fg(*color));
            ListItem::new(Line::from(vec![
                Span::styled(format!("{}  {:<12} {} ", prefix, name, rgb), style),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Theme Editor"));
    f.render_widget(list, chunks[0]);

    let help = Paragraph::new("  j/k: Color │ h/l: Channel │ a: Save New │ Enter: Done")
        .style(Style::default().fg(theme.muted))
        .block(Block::default().borders(Borders::ALL).title("Controls"));
    f.render_widget(help, chunks[1]);
}

// New #12: Dashboard Customization
fn render_dashboard_custom(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme();

    let widgets = [
        ("Summary", app.widget_layout.show_summary),
        ("Chart", app.widget_layout.show_chart),
        ("History", app.widget_layout.show_history),
        ("Compression", app.widget_layout.show_compression),
    ];

    let items: Vec<ListItem> = widgets.iter().enumerate()
        .map(|(i, (name, enabled))| {
            let prefix = if i == app.dashboard_selected { "▶ " } else { "  " };
            let status = if *enabled { "✓ ON" } else { "✗ OFF" };
            let status_color = if *enabled { theme.success } else { theme.error };
            let style = if i == app.dashboard_selected {
                Style::default().fg(theme.accent).bg(theme.bg_highlight).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{}  {} ", prefix, name), style),
                Span::styled(status, Style::default().fg(status_color)),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Dashboard Widgets (Space to toggle, Enter to save)"));
    f.render_widget(list, area);
}

// New #6: Compression Graph
fn render_compression_graph(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(8), Constraint::Min(0)])
        .split(area);

    // Bar chart
    if !app.compression_stats.is_empty() {
        let max_count = app.compression_stats.iter().map(|s| s.count as u64).max().unwrap_or(1);
        let bar_data: Vec<(&str, u64)> = app.compression_stats.iter()
            .map(|s| (s.format.as_str(), s.count as u64))
            .collect();

        let barchart = BarChart::default()
            .block(Block::default().borders(Borders::ALL).title("Files by Format"))
            .data(&bar_data)
            .bar_width(4)
            .bar_style(Style::default().fg(theme.primary))
            .value_style(Style::default().fg(Color::White))
            .max(max_count);
        f.render_widget(barchart, chunks[0]);
    } else {
        let empty = Paragraph::new("  No data. Run Size Comparison first.")
            .style(Style::default().fg(theme.muted))
            .block(Block::default().borders(Borders::ALL).title("Files by Format"));
        f.render_widget(empty, chunks[0]);
    }

    // Detail list
    let visible = chunks[1].height.saturating_sub(2) as usize;
    let start = app.compress_scroll.min(app.compression_stats.len().saturating_sub(visible));
    let end = (start + visible).min(app.compression_stats.len());

    let mut lines = vec![];
    for stat in &app.compression_stats[start..end] {
        let ratio = if stat.original_size > 0 {
            (1.0 - stat.converted_size as f64 / stat.original_size as f64) * 100.0
        } else { 0.0 };
        let bar_len = (ratio / 5.0) as usize;
        let bar = "█".repeat(bar_len);
        lines.push(Line::from(vec![
            Span::styled(format!("  {:<8} ", stat.format.to_uppercase()), Style::default().fg(theme.primary)),
            Span::styled(format!("{} → {} ", format_size(stat.original_size), format_size(stat.converted_size)),
                Style::default().fg(Color::White)),
            Span::styled(format!("↓{:.1}% ", ratio), 
                if ratio > 50.0 { Style::default().fg(theme.success) } else { Style::default().fg(theme.warning) }),
            Span::styled(bar, Style::default().fg(theme.accent)),
            Span::styled(format!(" ({} files)", stat.count), Style::default().fg(theme.muted)),
        ]));
    }

    let detail = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title("Compression by Format"));
    f.render_widget(detail, chunks[1]);
}

// New #7: File Classification
fn render_file_classify(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)])
        .split(area);

    let items: Vec<ListItem> = app.classify_rules.iter().enumerate()
        .map(|(i, (pattern, folder))| {
            let prefix = if i == app.classify_selected { "▶ " } else { "  " };
            let style = if i == app.classify_selected {
                Style::default().fg(theme.accent).bg(theme.bg_highlight).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(Line::from(Span::styled(
                format!("{}  \"{}\" → {}/", prefix, pattern, folder),
                style,
            )))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title(
            format!("Classification Rules ({})", app.classify_rules.len())
        ));
    f.render_widget(list, chunks[0]);

    if app.classify_adding {
        let input = Paragraph::new(format!("  pattern:folder = {}_", app.classify_input))
            .style(Style::default().fg(theme.warning))
            .block(Block::default().borders(Borders::ALL).title("Add Rule (Enter to confirm)"));
        f.render_widget(input, chunks[1]);
    } else {
        let help = Paragraph::new("  a: Add │ d: Delete │ r: Run Classification │ Esc: Back")
            .style(Style::default().fg(theme.muted))
            .block(Block::default().borders(Borders::ALL).title("Actions"));
        f.render_widget(help, chunks[1]);
    }
}

// New #8: Metadata Editor
fn render_meta_edit(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)])
        .split(area);

    let visible = chunks[0].height.saturating_sub(2) as usize;
    let start = app.meta_scroll.min(app.meta_files.len().saturating_sub(visible));
    let end = (start + visible).min(app.meta_files.len());

    let mut lines = vec![
        Line::from(Span::styled(
            format!("  {:>4}  {:<40}  {}", " ", "Filename", "Select"),
            Style::default().fg(theme.primary).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled("  ─────────────────────────────────────────────────────────", Style::default().fg(theme.muted))),
    ];

    for (i, (name, selected)) in app.meta_files[start..end].iter().enumerate() {
        let idx = start + i;
        let prefix = if idx == app.meta_scroll { "▶" } else { " " };
        let check = if *selected { "✓" } else { " " };
        let style = if idx == app.meta_scroll {
            Style::default().fg(theme.accent).bg(theme.bg_highlight)
        } else if *selected {
            Style::default().fg(theme.success)
        } else {
            Style::default().fg(Color::White)
        };
        lines.push(Line::from(Span::styled(
            format!("  {} {:>4}  {:<40}  [{}]", prefix, idx + 1, name, check),
            style,
        )));
    }

    let list = Paragraph::new(lines)
        .block(Block::default().borders(Borders::ALL).title(
            format!("Metadata Editor ({}/{} selected)", 
                app.meta_files.iter().filter(|(_, s)| *s).count(), app.meta_files.len())
        ));
    f.render_widget(list, chunks[0]);

    let fields = ["DateTime", "Artist", "Remove All"];
    let field_str: Vec<String> = fields.iter().enumerate()
        .map(|(i, f)| if i == app.meta_field { format!("[{}]", f) } else { f.to_string() })
        .collect();
    let help = Paragraph::new(format!("  Tab: Field ({}) │ x: Remove Meta │ a: Select All │ Esc: Back", field_str.join("/")))
        .style(Style::default().fg(theme.muted))
        .block(Block::default().borders(Borders::ALL).title("Actions"));
    f.render_widget(help, chunks[1]);
}

// New #19: Config Import/Export
fn render_config_io(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)])
        .split(area);

    let options = ["Export Config", "Import Config"];
    let items: Vec<ListItem> = options.iter().enumerate()
        .map(|(i, opt)| {
            let prefix = if i == app.config_io_selected { "▶ " } else { "  " };
            let style = if i == app.config_io_selected {
                Style::default().fg(theme.accent).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(Line::from(Span::styled(format!("{}  {}", prefix, opt), style)))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Config Import/Export"));
    f.render_widget(list, chunks[0]);

    if app.config_io_adding {
        let input = Paragraph::new(format!("  Path: {}_", app.config_io_path))
            .style(Style::default().fg(theme.warning))
            .block(Block::default().borders(Borders::ALL).title("Import Path (Enter to confirm)"));
        f.render_widget(input, chunks[1]);
    } else {
        let help = Paragraph::new("  e: Export │ i: Import │ Esc: Back")
            .style(Style::default().fg(theme.muted))
            .block(Block::default().borders(Borders::ALL).title("Actions"));
        f.render_widget(help, chunks[1]);
    }
}

// New #20: Plugins
fn render_plugins(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme();

    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Min(0), Constraint::Length(3)])
        .split(area);

    if app.plugins.is_empty() {
        let empty = Paragraph::new("  No plugins found. Add .json plugin files to ./plugins/")
            .style(Style::default().fg(theme.muted))
            .block(Block::default().borders(Borders::ALL).title("Plugins"));
        f.render_widget(empty, chunks[0]);
    } else {
        let items: Vec<ListItem> = app.plugins.iter().enumerate()
            .map(|(i, plugin)| {
                let prefix = if i == app.plugin_selected { "▶ " } else { "  " };
                let status = if plugin.enabled { "● ON" } else { "○ OFF" };
                let status_color = if plugin.enabled { theme.success } else { theme.muted };
                let style = if i == app.plugin_selected {
                    Style::default().fg(theme.accent).bg(theme.bg_highlight).add_modifier(Modifier::BOLD)
                } else {
                    Style::default().fg(Color::White)
                };
                ListItem::new(Line::from(vec![
                    Span::styled(format!("{}  {} ", prefix, status), Style::default().fg(status_color)),
                    Span::styled(format!("{} - {}", plugin.name, plugin.description), style),
                ]))
            })
            .collect();

        let list = List::new(items)
            .block(Block::default().borders(Borders::ALL).title("Plugins"));
        f.render_widget(list, chunks[0]);
    }

    let help = Paragraph::new("  j/k: Select │ Space: Toggle │ r: Scan │ o: Open Dir │ Esc: Back")
        .style(Style::default().fg(theme.muted))
        .block(Block::default().borders(Borders::ALL).title("Actions"));
    f.render_widget(help, chunks[1]);
}

// New #15: Statusbar Customization
fn render_statusbar_custom(f: &mut Frame, app: &mut App, area: Rect) {
    let theme = app.theme();

    let items: Vec<ListItem> = app.statusbar_items.iter().enumerate()
        .map(|(i, (name, enabled))| {
            let prefix = if i == app.statusbar_selected { "▶ " } else { "  " };
            let status = if *enabled { "✓" } else { "✗" };
            let status_color = if *enabled { theme.success } else { theme.error };
            let style = if i == app.statusbar_selected {
                Style::default().fg(theme.accent).bg(theme.bg_highlight).add_modifier(Modifier::BOLD)
            } else {
                Style::default().fg(Color::White)
            };
            ListItem::new(Line::from(vec![
                Span::styled(format!("{}  {} ", prefix, name), style),
                Span::styled(format!("[{}]", status), Style::default().fg(status_color)),
            ]))
        })
        .collect();

    let list = List::new(items)
        .block(Block::default().borders(Borders::ALL).title("Statusbar Items (Space to toggle, Enter to save)"));
    f.render_widget(list, area);
}

// ============================================================
// Core processing functions
// ============================================================

fn is_image_file(path: &PathBuf) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| DEFAULT_IMAGE_EXTENSIONS.iter().any(|e| e.eq_ignore_ascii_case(&format!(".{}", ext))))
        .unwrap_or(false)
}

fn calculate_sha256(path: &PathBuf) -> Result<String, Box<dyn std::error::Error>> {
    let file = fs::File::open(path)?;
    let mut reader = BufReader::with_capacity(BUFFER_SIZE, file);
    let mut hasher = Sha256::new();
    let mut buffer = [0; BUFFER_SIZE];

    loop {
        let n = reader.read(&mut buffer)?;
        if n == 0 {
            break;
        }
        hasher.update(&buffer[..n]);
    }

    Ok(format!("{:x}", hasher.finalize()))
}

// ============================================================
// Perceptual Hash (Similar Image Detection)
// ============================================================

/// Average Hash (aHash): resize to 8x8 grayscale, compare to mean
fn calculate_ahash(path: &PathBuf) -> Result<u64, Box<dyn std::error::Error>> {
    let img = image::open(path)?;
    let gray = img.resize_exact(8, 8, image::imageops::FilterType::Lanczos3).to_luma8();
    let pixels: Vec<u8> = gray.pixels().map(|p| p[0]).collect();
    let mean: u64 = pixels.iter().map(|&p| p as u64).sum::<u64>() / 64;
    let mut hash: u64 = 0;
    for (i, &pixel) in pixels.iter().enumerate() {
        if pixel as u64 >= mean {
            hash |= 1 << (63 - i);
        }
    }
    Ok(hash)
}

/// Difference Hash (dHash): compare adjacent pixels horizontally
fn calculate_dhash(path: &PathBuf) -> Result<u64, Box<dyn std::error::Error>> {
    let img = image::open(path)?;
    let gray = img.resize_exact(9, 8, image::imageops::FilterType::Lanczos3).to_luma8();
    let pixels: Vec<u8> = gray.pixels().map(|p| p[0]).collect();
    let mut hash: u64 = 0;
    for row in 0..8 {
        for col in 0..8 {
            let idx = row * 9 + col;
            if pixels[idx] < pixels[idx + 1] {
                hash |= 1 << (63 - (row * 8 + col));
            }
        }
    }
    Ok(hash)
}

/// Hamming distance between two 64-bit hashes
fn hamming_distance(a: u64, b: u64) -> u32 {
    (a ^ b).count_ones()
}

fn rename_by_timestamp(dest: &str) -> Result<(), Box<dyn std::error::Error>> {
    let entries: Vec<_> = fs::read_dir(dest)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file() && is_image_file(&e.path()))
        .collect();

    for entry in entries {
        let path = entry.path();
        let file_name = path.file_name().unwrap().to_string_lossy();

        if file_name.starts_with(|c: char| c.is_numeric()) && file_name.len() > 14 {
            continue;
        }

        let ext = path.extension().unwrap().to_string_lossy().to_string();
        let metadata = fs::metadata(&path)?;
        let modified = metadata.modified()?;
        let datetime: chrono::DateTime<chrono::Local> = modified.into();
        let timestamp = datetime.format("%Y%m%d%H%M%S").to_string();

        let new_name = format!("{}{}", timestamp, ext);
        let final_name = get_unique_filename(&path, &new_name)?;

        let new_path = path.parent().unwrap().join(&final_name);
        fs::rename(&path, &new_path)?;
    }
    Ok(())
}

fn get_unique_filename(path: &PathBuf, name: &str) -> Result<String, Box<dyn std::error::Error>> {
    let mut new_path = path.parent().unwrap().to_path_buf();
    new_path.push(name);

    if !new_path.exists() {
        return Ok(name.to_string());
    }

    let base = name.trim_end_matches(|c: char| c.is_numeric() || c == '.');
    let ext = path.extension().unwrap().to_string_lossy().to_string();

    for i in 0..10 {
        let candidate = format!("{}{}{}", base, i, ext);
        let candidate_path = path.parent().unwrap().join(&candidate);
        if !candidate_path.exists() {
            return Ok(candidate);
        }
    }

    Err("Could not find unique filename".into())
}

fn rename_remove_underscore_parens(dest: &str) -> Result<(), Box<dyn std::error::Error>> {
    let entries: Vec<_> = fs::read_dir(dest)?
        .filter_map(|e| e.ok())
        .filter(|e| e.path().is_file() && is_image_file(&e.path()))
        .collect();

    for entry in entries {
        let path = entry.path();
        let file_stem = path.file_stem().unwrap().to_string_lossy();
        let ext = path.extension().unwrap().to_string_lossy().to_string();

        let new_name = if is_digit_underscore_digit(&file_stem) {
            format!("{}{}", file_stem.replace("_", ""), ext)
        } else if let Some(cleaned) = remove_trailing_parentheses(&file_stem) {
            format!("{}{}", cleaned, ext)
        } else {
            continue;
        };

        let final_name = get_unique_filename(&path, &new_name)?;
        let new_path = path.parent().unwrap().join(&final_name);
        fs::rename(&path, &new_path)?;
    }
    Ok(())
}

fn is_digit_underscore_digit(s: &str) -> bool {
    let parts: Vec<&str> = s.split('_').collect();
    parts.len() == 2
        && parts[0].chars().all(char::is_numeric)
        && parts[1].chars().all(char::is_numeric)
}

fn remove_trailing_parentheses(s: &str) -> Option<String> {
    if !s.ends_with(')') || !s.contains('(') {
        return None;
    }

    let pos = s.rfind('(')?;
    let inner = &s[pos + 1..].trim_end_matches(')');

    if inner.chars().all(char::is_numeric) {
        Some(s[..pos].trim_end().to_string())
    } else {
        None
    }
}

fn convert_to_jxl(dest: &str) -> Result<(), Box<dyn std::error::Error>> {
    let status = Command::new("powershell")
        .args(&["-NoProfile", "-ExecutionPolicy", "Bypass", "-File"])
        .arg(r"Z:\Closet\bat\jpg-to-jxl.ps1")
        .arg("-convertPath")
        .arg(dest)
        .status()?;

    if !status.success() {
        return Err("JXL conversion failed".into());
    }
    Ok(())
}

// ============================================================
// Batch 3: Render Functions
// ============================================================

fn render_image_preview(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(area);

    let path_info = if app.preview_image_path.is_empty() {
        "No image selected".to_string()
    } else {
        app.preview_image_path.clone()
    };
    let header = Paragraph::new(Line::from(vec![
        Span::styled("  🖼️  Image Preview", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
        Span::raw(format!("  │  {}", path_info)),
    ]))
    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Magenta)))
    .alignment(ratatui::layout::Alignment::Left);
    f.render_widget(header, chunks[0]);

    match &app.image_preview {
        Some(preview) => {
            let inner = Layout::default()
                .direction(Direction::Horizontal)
                .constraints([Constraint::Percentage(70), Constraint::Percentage(30)])
                .split(chunks[1]);

            // ASCII art
            let ascii_text: Vec<Line> = preview.ascii_lines.iter()
                .map(|l| Line::from(Span::styled(l.clone(), Style::default().fg(Color::White))))
                .collect();
            let ascii_block = Paragraph::new(ascii_text)
                .block(Block::default()
                    .title(format!(" {}x{} ", preview.width, preview.height))
                    .borders(Borders::ALL)
                    .border_style(Style::default().fg(Color::DarkGray)));
            f.render_widget(ascii_block, inner[0]);

            // File info
            let info_lines = vec![
                Line::from(vec![Span::styled("File: ", Style::default().fg(Color::Yellow)), Span::raw(&preview.filename)]),
                Line::from(vec![Span::styled("Size: ", Style::default().fg(Color::Yellow)), Span::raw(format!("{}x{}", preview.width, preview.height))]),
                Line::from(vec![Span::styled("Lines: ", Style::default().fg(Color::Yellow)), Span::raw(format!("{}", preview.ascii_lines.len()))]),
            ];
            let info_block = Paragraph::new(info_lines)
                .block(Block::default().title(" Info ").borders(Borders::ALL).border_style(Style::default().fg(Color::DarkGray)));
            f.render_widget(info_block, inner[1]);
        }
        None => {
            let msg = Paragraph::new("  画像を選択するとASCIIプレビューが表示されます。")
                .style(Style::default().fg(Color::DarkGray))
                .alignment(ratatui::layout::Alignment::Center);
            f.render_widget(msg, chunks[1]);
        }
    }
}

fn render_split_pane(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    let header = Paragraph::new(Line::from(vec![
        Span::styled("  📑  Split Pane View", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw("  │  [Tab]パネル切替  [←→]リサイズ  [q]戻る"),
    ]))
    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan)));
    f.render_widget(header, chunks[0]);

    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    // Left pane - batch queue
    let left_items: Vec<ListItem> = app.batch_queue.iter().enumerate().map(|(i, job)| {
        let fname = std::path::Path::new(&job.path).file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| job.path.clone());
        ListItem::new(Line::from(Span::raw(format!("  {}", fname))))
    }).collect();
    let left_list = List::new(left_items)
        .block(Block::default().title(" Source Files ").borders(Borders::ALL).border_style(Style::default().fg(Color::Green)));
    f.render_widget(left_list, panes[0]);

    // Right pane - dest dir
    let right_items: Vec<ListItem> = app.duplicate_groups.iter().flat_map(|g| g.files.iter()).map(|df| {
        let fname = std::path::Path::new(&df.path).file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| df.path.clone());
        ListItem::new(Line::from(Span::raw(format!("  {}", fname))))
    }).collect();
    let right_list = List::new(right_items)
        .block(Block::default().title(" Destination Files ").borders(Borders::ALL).border_style(Style::default().fg(Color::Blue)));
    f.render_widget(right_list, panes[1]);
}

fn render_quick_actions(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    let header = Paragraph::new(Line::from(vec![
        Span::styled("  ⚡  Quick Actions", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::raw("  │  [↑↓]選択  [Enter]実行  [q]戻る"),
    ]))
    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Yellow)));
    f.render_widget(header, chunks[0]);

    let items: Vec<ListItem> = app.quick_actions.iter().enumerate().map(|(i, (label, _))| {
        let style = if i == app.quick_selected {
            Style::default().fg(Color::Black).bg(Color::Yellow)
        } else {
            Style::default().fg(Color::White)
        };
        let icon = match i {
            0 => " ",
            1 => " ",
            2 => " ",
            3 => " ",
            4 => " ",
            _ => " ",
        };
        ListItem::new(Line::from(Span::styled(format!("  {} {}", icon, label), style)))
    }).collect();

    let list = List::new(items)
        .block(Block::default().title(" アクション ").borders(Borders::ALL).border_style(Style::default().fg(Color::DarkGray)));
    f.render_widget(list, chunks[1]);
}

fn render_recent_files(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    let header = Paragraph::new(Line::from(vec![
        Span::styled("  📂  Recent Files", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::raw(format!("  │  {} files  │  [↑↓]選択  [Enter]プレビュー  [q]戻る", app.recent_files.len())),
    ]))
    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Green)));
    f.render_widget(header, chunks[0]);

    if app.recent_files.is_empty() {
        let msg = Paragraph::new("  最近処理したファイルはありません。")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(ratatui::layout::Alignment::Center);
        f.render_widget(msg, chunks[1]);
        return;
    }

    let items: Vec<ListItem> = app.recent_files.iter().enumerate().map(|(i, rf)| {
        let fname = std::path::Path::new(&rf.path).file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_else(|| rf.path.clone());
        let size_str = if rf.size > 1_000_000 {
            format!("{:.1} MB", rf.size as f64 / 1_000_000.0)
        } else {
            format!("{:.1} KB", rf.size as f64 / 1_000.0)
        };
        ListItem::new(Line::from(vec![
            Span::styled(format!("  {} ", fname), Style::default().fg(Color::White)),
            Span::styled(format!("[{}] ", size_str), Style::default().fg(Color::DarkGray)),
            Span::styled(format!("{} ", rf.file_type), Style::default().fg(Color::Cyan)),
            Span::styled(&rf.processed_at, Style::default().fg(Color::DarkGray)),
        ]))
    }).collect();

    let list = List::new(items)
        .block(Block::default().title(" 最近のファイル ").borders(Borders::ALL).border_style(Style::default().fg(Color::DarkGray)));
    f.render_widget(list, chunks[1]);
}

fn render_tag_system(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    let header = Paragraph::new(Line::from(vec![
        Span::styled("  🏷️  Tag System", Style::default().fg(Color::Blue).add_modifier(Modifier::BOLD)),
        Span::raw(format!("  │  {} tagged patterns  │  [↑↓]選択  [a]タグ追加  [d]削除  [q]戻る", app.file_tags.len())),
    ]))
    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Blue)));
    f.render_widget(header, chunks[0]);

    if app.file_tags.is_empty() {
        let msg = Paragraph::new("  タグ付きファイルパターンはありません。[a]で追加してください。")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(ratatui::layout::Alignment::Center);
        f.render_widget(msg, chunks[1]);
        return;
    }

    let items: Vec<ListItem> = app.file_tags.iter().enumerate().map(|(i, ft)| {
        let style = if i == app.tag_selected {
            Style::default().fg(Color::Black).bg(Color::Blue)
        } else {
            Style::default().fg(Color::White)
        };
        let tags_str = ft.tags.join(", ");
        ListItem::new(Line::from(vec![
            Span::styled(format!("  📁 {} ", ft.file_pattern), style),
            Span::styled(format!("[{}]", tags_str), Style::default().fg(Color::Yellow)),
        ]))
    }).collect();

    let list = List::new(items)
        .block(Block::default().title(" タグ ").borders(Borders::ALL).border_style(Style::default().fg(Color::DarkGray)));
    f.render_widget(list, chunks[1]);
}

fn render_side_by_side_diff(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    let header = Paragraph::new(Line::from(vec![
        Span::styled("  🔀  Side-by-side Diff", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
        Span::raw("  │  [↑↓]スクロール  [Tab]フォーカス切替  [q]戻る"),
    ]))
    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Magenta)));
    f.render_widget(header, chunks[0]);

    let panes = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(chunks[1]);

    // Left side
    let left_lines: Vec<Line> = if app.diff_left.is_empty() {
        vec![Line::from(Span::styled("  差分データがありません", Style::default().fg(Color::DarkGray)))]
    } else {
        app.diff_left.iter().map(|l| {
            let color = if l.starts_with('+') {
                Color::Green
            } else if l.starts_with('-') {
                Color::Red
            } else {
                Color::White
            };
            Line::from(Span::styled(format!("  {}", l), Style::default().fg(color)))
        }).collect()
    };
    let left_para = Paragraph::new(left_lines)
        .block(Block::default().title(" Original ").borders(Borders::ALL).border_style(Style::default().fg(Color::Red)));
    f.render_widget(left_para, panes[0]);

    // Right side
    let right_lines: Vec<Line> = if app.diff_right.is_empty() {
        vec![Line::from(Span::styled("  差分データがありません", Style::default().fg(Color::DarkGray)))]
    } else {
        app.diff_right.iter().map(|l| {
            let color = if l.starts_with('+') {
                Color::Green
            } else if l.starts_with('-') {
                Color::Red
            } else {
                Color::White
            };
            Line::from(Span::styled(format!("  {}", l), Style::default().fg(color)))
        }).collect()
    };
    let right_para = Paragraph::new(right_lines)
        .block(Block::default().title(" Modified ").borders(Borders::ALL).border_style(Style::default().fg(Color::Green)));
    f.render_widget(right_para, panes[1]);
}

fn render_file_tree_view(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    let header = Paragraph::new(Line::from(vec![
        Span::styled("  🌳  File Tree", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::raw("  │  [↑↓]選択  [Enter]展開/折畑  [q]戻る"),
    ]))
    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Green)));
    f.render_widget(header, chunks[0]);

    if app.file_tree.is_empty() {
        let msg = Paragraph::new("  ファイルツリーがありません。")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(ratatui::layout::Alignment::Center);
        f.render_widget(msg, chunks[1]);
        return;
    }

    let mut lines: Vec<Line> = Vec::new();
    let mut counter = 0usize;
    render_tree_nodes(&app.file_tree, &mut lines, &mut counter, app.tree_selected);

    let tree_para = Paragraph::new(lines)
        .block(Block::default().title(" ディレクトリ構造 ").borders(Borders::ALL).border_style(Style::default().fg(Color::DarkGray)));
    f.render_widget(tree_para, chunks[1]);
}

fn render_tree_nodes(nodes: &[FileTreeNode], lines: &mut Vec<Line>, counter: &mut usize, selected: usize) {
    for node in nodes {
        let indent = "  ".repeat(node.depth);
        let icon = if node.is_dir {
            if node.expanded { "📂" } else { "📁" }
        } else {
            "  "
        };
        let style = if *counter == selected {
            Style::default().fg(Color::Black).bg(Color::Green)
        } else {
            Style::default().fg(Color::White)
        };
        lines.push(Line::from(Span::styled(
            format!("{}{} {} {}", indent, icon, node.name, if node.is_dir { "/" } else { "" }),
            style,
        )));
        *counter += 1;
        if node.expanded && !node.children.is_empty() {
            render_tree_nodes(&node.children, lines, counter, selected);
        }
    }
}

fn render_rename_pattern(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(area);

    let header = Paragraph::new(Line::from(vec![
        Span::styled("  ✏️  Batch Rename Pattern", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::raw(format!("  │  {} patterns  │  [a]追加  [↑↓]選択  [Tab]フィールド  [p]プレビュー  [q]戻る", app.rename_patterns.len())),
    ]))
    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Yellow)));
    f.render_widget(header, chunks[0]);

    // Pattern input area
    let current_pattern = if let Some(pat) = app.rename_patterns.get(app.rename_selected) {
        format!("Pattern: {}  →  Replacement: {}  {}", pat.pattern, pat.replacement, if pat.use_regex { "[Regex]" } else { "[Glob]" })
    } else {
        "パターンを選択してください".to_string()
    };
    let input_style = if app.rename_field == 0 { Color::Cyan } else { Color::Yellow };
    let input_block = Paragraph::new(Line::from(Span::styled(
        format!("  {}", current_pattern),
        Style::default().fg(input_style),
    )))
    .block(Block::default().title(" Pattern ").borders(Borders::ALL).border_style(Style::default().fg(Color::DarkGray)));
    f.render_widget(input_block, chunks[1]);

    // Preview list
    let preview_items: Vec<ListItem> = if let Some(pat) = app.rename_patterns.get(app.rename_selected) {
        pat.preview.iter().map(|(old, new)| {
            ListItem::new(Line::from(vec![
                Span::styled(format!("  {} ", old), Style::default().fg(Color::Red)),
                Span::styled("→ ", Style::default().fg(Color::DarkGray)),
                Span::styled(format!("{}", new), Style::default().fg(Color::Green)),
            ]))
        }).collect()
    } else {
        vec![ListItem::new(Line::from(Span::styled("  プレビューなし", Style::default().fg(Color::DarkGray))))]
    };
    let preview_list = List::new(preview_items)
        .block(Block::default().title(" プレビュー ").borders(Borders::ALL).border_style(Style::default().fg(Color::DarkGray)));
    f.render_widget(preview_list, chunks[2]);
}

fn render_timeline(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    let header = Paragraph::new(Line::from(vec![
        Span::styled("  ⏱️  Processing Timeline", Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)),
        Span::raw(format!("  │  {} events  │  [↑↓]スクロール  [q]戻る", app.timeline_entries.len())),
    ]))
    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Cyan)));
    f.render_widget(header, chunks[0]);

    if app.timeline_entries.is_empty() {
        let msg = Paragraph::new("  タイムラインイベントがありません。")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(ratatui::layout::Alignment::Center);
        f.render_widget(msg, chunks[1]);
        return;
    }

    let items: Vec<ListItem> = app.timeline_entries.iter().map(|entry| {
        let (icon, color) = match entry.event_type.as_str() {
            "start" => ("▶", Color::Green),
            "progress" => ("●", Color::Yellow),
            "complete" => ("✔", Color::Cyan),
            "error" => ("✖", Color::Red),
            _ => ("•", Color::White),
        };
        let progress_bar = if entry.progress > 0.0 {
            let filled = (entry.progress * 20.0) as usize;
            let empty = 20 - filled;
            format!("[{}{}]", "█".repeat(filled), "░".repeat(empty))
        } else {
            String::new()
        };
        ListItem::new(Line::from(vec![
            Span::styled(format!("  {} ", icon), Style::default().fg(color)),
            Span::styled(format!("[{}] ", entry.timestamp), Style::default().fg(Color::DarkGray)),
            Span::raw(&entry.description),
            Span::styled(format!(" {}", progress_bar), Style::default().fg(color)),
        ]))
    }).collect();

    let list = List::new(items)
        .block(Block::default().title(" タイムライン ").borders(Borders::ALL).border_style(Style::default().fg(Color::DarkGray)));
    f.render_widget(list, chunks[1]);
}

fn render_command_palette(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(area);

    let header = Paragraph::new(Line::from(vec![
        Span::styled("  🎨  Command Palette", Style::default().fg(Color::Magenta).add_modifier(Modifier::BOLD)),
        Span::raw(format!("  │  Query: {} │  {} results", app.palette_query, app.palette_results.len())),
    ]))
    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Magenta)));
    f.render_widget(header, chunks[0]);

    if app.palette_results.is_empty() {
        let msg = Paragraph::new("  コマンドを入力してください...")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(ratatui::layout::Alignment::Center);
        f.render_widget(msg, chunks[1]);
        return;
    }

    let items: Vec<ListItem> = app.palette_results.iter().enumerate().map(|(i, (label, _))| {
        let style = if i == app.palette_selected {
            Style::default().fg(Color::Black).bg(Color::Magenta)
        } else {
            Style::default().fg(Color::White)
        };
        ListItem::new(Line::from(Span::styled(format!("  {} ", label), style)))
    }).collect();

    let list = List::new(items)
        .block(Block::default().title(" コマンド ").borders(Borders::ALL).border_style(Style::default().fg(Color::DarkGray)));
    f.render_widget(list, chunks[1]);
}

fn render_notification_center(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(0)])
        .split(area);

    let unread = app.notifications.iter().filter(|n| !n.read).count();
    let header = Paragraph::new(Line::from(vec![
        Span::styled("  🔔  Notification Center", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::raw(format!("  │  {} notifications ({} unread)  │  [↑↓]選択  [r]既読  [q]戻る", app.notifications.len(), unread)),
    ]))
    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Yellow)));
    f.render_widget(header, chunks[0]);

    if app.notifications.is_empty() {
        let msg = Paragraph::new("  通知はありません。")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(ratatui::layout::Alignment::Center);
        f.render_widget(msg, chunks[1]);
        return;
    }

    let items: Vec<ListItem> = app.notifications.iter().map(|notif| {
        let (icon, color) = match notif.level.as_str() {
            "info" => ("ℹ", Color::Cyan),
            "warning" => ("⚠", Color::Yellow),
            "error" => ("✖", Color::Red),
            "success" => ("✔", Color::Green),
            _ => ("•", Color::White),
        };
        let read_marker = if notif.read { "  " } else { "  " };
        ListItem::new(Line::from(vec![
            Span::raw(read_marker),
            Span::styled(format!("{} ", icon), Style::default().fg(color)),
            Span::styled(format!("[{}] ", notif.timestamp), Style::default().fg(Color::DarkGray)),
            Span::styled(&notif.message, if notif.read {
                Style::default().fg(Color::DarkGray)
            } else {
                Style::default().fg(Color::White)
            }),
        ]))
    }).collect();

    let list = List::new(items)
        .block(Block::default().title(" 通知 ").borders(Borders::ALL).border_style(Style::default().fg(Color::DarkGray)));
    f.render_widget(list, chunks[1]);
}

fn render_export_report(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Length(5),
            Constraint::Min(0),
        ])
        .split(area);

    let header = Paragraph::new(Line::from(vec![
        Span::styled("  📊  Export Report", Style::default().fg(Color::Green).add_modifier(Modifier::BOLD)),
        Span::raw("  │  [←→]フォーマット切替  [Enter]エクスポート  [q]戻る"),
    ]))
    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Green)));
    f.render_widget(header, chunks[0]);

    // Format selection
    let formats = ["HTML", "Markdown"];
    let format_items: Vec<Line> = formats.iter().enumerate().map(|(i, fmt)| {
        let style = if i == app.report_format {
            Style::default().fg(Color::Black).bg(Color::Green)
        } else {
            Style::default().fg(Color::White)
        };
        Line::from(Span::styled(format!("  {} {}", if i == app.report_format { "▶" } else { " " }, fmt), style))
    }).collect();
    let format_block = Paragraph::new(format_items)
        .block(Block::default().title(" フォーマット ").borders(Borders::ALL).border_style(Style::default().fg(Color::DarkGray)));
    f.render_widget(format_block, chunks[1]);

    // Report preview
    let report_text = format!(
        "  Report Preview\n  \n  Format: {}\n  Path: {}\n  \n  処理統計、重複検出結果、圧縮効率などのレポートを生成します。",
        formats[app.report_format], app.report_path
    );
    let report_lines: Vec<Line> = report_text.lines().map(|l| Line::from(Span::raw(l.to_string()))).collect();
    let report_block = Paragraph::new(report_lines)
        .block(Block::default().title(" プレビュー ").borders(Borders::ALL).border_style(Style::default().fg(Color::DarkGray)));
    f.render_widget(report_block, chunks[2]);
}

fn render_similar_images(f: &mut Frame, app: &mut App, area: Rect) {
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(3),
            Constraint::Min(0),
        ])
        .split(area);

    let header = Paragraph::new(Line::from(vec![
        Span::styled("  🔍 類似画像検索", Style::default().fg(Color::Yellow).add_modifier(Modifier::BOLD)),
        Span::raw(format!(
            "  │  閾値: {}ビット  │  グループ数: {}  │  [↑↓]グループ  [j/k]ファイル  [d]削除  [+/-]閾値",
            app.similar_threshold, app.similar_groups.len()
        )),
    ]))
    .block(Block::default().borders(Borders::ALL).border_style(Style::default().fg(Color::Yellow)))
    .alignment(ratatui::layout::Alignment::Left);
    f.render_widget(header, chunks[0]);

    if app.similar_groups.is_empty() {
        let msg = Paragraph::new("  類似画像グループが見つかりません。メニューからスキャンを実行してください。")
            .style(Style::default().fg(Color::DarkGray))
            .alignment(ratatui::layout::Alignment::Center);
        f.render_widget(msg, chunks[1]);
        return;
    }

    let inner = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Percentage(35),
            Constraint::Percentage(65),
        ])
        .split(chunks[1]);

    // Group list (left pane)
    let mut group_items: Vec<ListItem> = Vec::new();
    for (i, group) in app.similar_groups.iter().enumerate() {
        let style = if i == app.similar_selected {
            Style::default().fg(Color::Black).bg(Color::Cyan)
        } else {
            Style::default().fg(Color::White)
        };
        let icon = if group.hash_type == "aHash" { "🎨" } else { "📐" };
        group_items.push(ListItem::new(Line::from(Span::styled(
            format!("{} Group {:02} ({} files, hash: {:016x})", icon, i + 1, group.files.len(), group.hash),
            style,
        ))));
    }
    let group_list = List::new(group_items)
        .block(Block::default()
            .title(" 類似グループ ")
            .borders(Borders::ALL)
            .border_style(Style::default().fg(Color::DarkGray)))
        .highlight_style(Style::default().fg(Color::Black).bg(Color::Cyan));
    f.render_widget(group_list, inner[0]);

    // File list (right pane)
    if let Some(group) = app.similar_groups.get(app.similar_selected) {
        let mut file_items: Vec<ListItem> = Vec::new();
        for (i, (path, size)) in group.files.iter().enumerate() {
            let fname = std::path::Path::new(path).file_name()
                .map(|n| n.to_string_lossy().to_string())
                .unwrap_or_else(|| path.clone());
            let size_str = if *size > 1_000_000 {
                format!("{:.1} MB", *size as f64 / 1_000_000.0)
            } else {
                format!("{:.1} KB", *size as f64 / 1_000.0)
            };
            let style = if i == app.similar_file_selected {
                Style::default().fg(Color::Black).bg(Color::Yellow)
            } else {
                Style::default().fg(Color::White)
            };
            let dist_info = if i > 0 {
                let d = hamming_distance(group.hash, group.hash); // placeholder
                format!(" dist:{}", d)
            } else {
                " (基準)".to_string()
            };
            file_items.push(ListItem::new(Line::from(vec![
                Span::styled(format!("  {:2}. {} ", i + 1, fname), style),
                Span::styled(format!("[{}]", size_str), Style::default().fg(Color::DarkGray)),
                Span::styled(dist_info, Style::default().fg(Color::Cyan)),
            ])));
        }
        let file_list = List::new(file_items)
            .block(Block::default()
                .title(format!(" Group {} ファイル ({}) ", app.similar_selected + 1, group.hash_type))
                .borders(Borders::ALL)
                .border_style(Style::default().fg(Color::DarkGray)));
        f.render_widget(file_list, inner[1]);
    }
}

fn hash_cache_db() {
    let status = Command::new(r"Z:\Closet\Remove-Duplicates\hash_cache_db.exe")
        .status();

    match status {
        Ok(_) => {}
        Err(e) => eprintln!("Error: {}", e),
    }
}
