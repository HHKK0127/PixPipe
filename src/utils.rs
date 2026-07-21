//! ユーティリティ関数モジュール
//!
//! 共通のヘルパー関数を提供します。

use ratatui::{
    style::{Color, Modifier, Style},
    text::{Line, Span},
};

/// 文字列を指定長に切り詰める
///
/// 長すぎる文字列を切り詰め、末尾に「...」を追加します。
///
/// # Arguments
/// * `s` - 元の文字列
/// * `max_len` - 最大長
///
/// # Returns
/// 切り詰められた文字列
pub fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        format!("{}...", &s[..max_len.saturating_sub(3)])
    }
}

/// ゲージバーの生成
///
/// 進捗率に基づいてゲージバー文字列を生成します。
///
/// # Arguments
/// * `ratio` - 進捗率（0.0〜1.0）
/// * `width` - バーの幅
///
/// # Returns
/// ゲージバー文字列（例: "[████████░░░░░░░░] 50%"）
pub fn make_gauge_bar(ratio: f64, width: usize) -> String {
    let filled = (ratio * width as f64) as usize;
    let empty = width.saturating_sub(filled);
    format!(
        "[{}{}] {:.0}%",
        "█".repeat(filled),
        "░".repeat(empty),
        ratio * 100.0
    )
}

/// サブ進捗バーの生成
///
/// ラベル付きのサブ進捗バーを生成します。
///
/// # Arguments
/// * `label` - ラベル
/// * `ratio` - 進捗率（0.0〜1.0）
/// * `width` - バーの幅
///
/// # Returns
/// サブ進捗バー文字列
pub fn make_sub_progress_bar(label: &str, ratio: f64, width: usize) -> String {
    let filled = (ratio * width as f64) as usize;
    let empty = width.saturating_sub(filled);
    format!(
        "{}: [{}{}] {:.0}%",
        label,
        "█".repeat(filled),
        "░".repeat(empty),
        ratio * 100.0
    )
}

/// 秒数を人間が読みやすい形式に変換
///
/// 秒数を「時:分:秒」形式に変換します。
///
/// # Arguments
/// * `secs` - 秒数
///
/// # Returns
/// フォーマットされた時間文字列（例: "1:23:45"）
pub fn format_duration(secs: f64) -> String {
    let hours = (secs / 3600.0) as u64;
    let minutes = ((secs % 3600.0) / 60.0) as u64;
    let seconds = (secs % 60.0) as u64;
    if hours > 0 {
        format!("{}:{:02}:{:02}", hours, minutes, seconds)
    } else {
        format!("{}:{:02}", minutes, seconds)
    }
}

/// ファイルサイズのフォーマット
///
/// バイト数を人間が読みやすい形式に変換します。
///
/// # Arguments
/// * `bytes` - バイト数
///
/// # Returns
/// フォーマットされたサイズ文字列（例: "1.23 MB"）
pub fn format_file_size(bytes: u64) -> String {
    const KB: u64 = 1024;
    const MB: u64 = KB * 1024;
    const GB: u64 = MB * 1024;

    if bytes >= GB {
        format!("{:.2} GB", bytes as f64 / GB as f64)
    } else if bytes >= MB {
        format!("{:.2} MB", bytes as f64 / MB as f64)
    } else if bytes >= KB {
        format!("{:.2} KB", bytes as f64 / KB as f64)
    } else {
        format!("{} bytes", bytes)
    }
}

/// パーセンテージのフォーマット
///
/// 進捗率をパーセンテージ文字列に変換します。
///
/// # Arguments
/// * `ratio` - 進捗率（0.0〜1.0）
///
/// # Returns
/// パーセンテージ文字列（例: "50%"）
pub fn format_percentage(ratio: f64) -> String {
    format!("{:.0}%", ratio * 100.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_truncate_str_short() {
        assert_eq!(truncate_str("hello", 10), "hello");
    }

    #[test]
    fn test_truncate_str_long() {
        assert_eq!(truncate_str("hello world", 8), "hello...");
    }

    #[test]
    fn test_make_gauge_bar() {
        let bar = make_gauge_bar(0.5, 10);
        assert!(bar.contains("50%"));
        assert!(bar.contains("█████"));
        assert!(bar.contains("░░░░░"));
    }

    #[test]
    fn test_format_duration() {
        assert_eq!(format_duration(65.0), "1:05");
        assert_eq!(format_duration(3661.0), "1:01:01");
    }

    #[test]
    fn test_format_file_size() {
        assert_eq!(format_file_size(1024), "1.00 KB");
        assert_eq!(format_file_size(1048576), "1.00 MB");
        assert_eq!(format_file_size(1073741824), "1.00 GB");
    }

    #[test]
    fn test_format_percentage() {
        assert_eq!(format_percentage(0.5), "50%");
        assert_eq!(format_percentage(0.123), "12%");
    }
}
