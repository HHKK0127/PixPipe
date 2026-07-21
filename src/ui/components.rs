// UI Components - Ferrocopy-inspired components for PixPipe TUI
// These components provide modern, styled UI elements.

#![allow(dead_code)]

use ratatui::{
    layout::Rect,
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Paragraph},
    Frame,
};

use crate::Theme;

// Toast notification types
#[derive(Debug, Clone, PartialEq)]
pub enum ToastType {
    Info,
    Success,
    Error,
    Warning,
}

// Toast notification structure
#[derive(Debug, Clone)]
pub struct Toast {
    pub id: u64,
    pub message: String,
    pub toast_type: ToastType,
    pub remaining: f64,
}

static NEXT_TOAST_ID: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

impl Toast {
    pub fn new(message: impl Into<String>, toast_type: ToastType) -> Self {
        let id = NEXT_TOAST_ID.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        Self {
            id,
            message: message.into(),
            toast_type,
            remaining: 5.0,
        }
    }
}

// Button variant styles
#[derive(Clone, Copy, PartialEq)]
pub enum ButtonVariant {
    Primary,
    Secondary,
    Success,
    Danger,
    Ghost,
}

// Badge variant styles
#[derive(Clone, Copy, PartialEq)]
pub enum BadgeVariant {
    Info,
    Success,
    Warning,
    Error,
    Muted,
}

// Alert variant styles
#[derive(Clone, Copy, PartialEq)]
pub enum AlertVariant {
    Info,
    Success,
    Warning,
    Error,
}

/// Render a status badge with icon and color
pub fn render_status_badge(status: &str, theme: &Theme) -> Line<'static> {
    let (icon, color, text) = match status {
        "copying" | "processing" => ("▶", theme.accent, "Processing"),
        "paused" => ("⏸", theme.warning, "Paused"),
        "done" | "complete" => ("✓", theme.success, "Done"),
        "error" => ("✗", theme.error, "Error"),
        "scanning" => ("⟳", theme.primary, "Scanning"),
        _ => (" ", theme.muted, "Ready"),
    };
    Line::from(Span::styled(
        format!(" {} {} ", icon, text),
        Style::default().fg(Color::White).bg(color),
    ))
}

/// Render a styled button with variant
#[allow(dead_code)]
pub fn render_button_variant(label: &str, variant: ButtonVariant, theme: &Theme) -> Line<'static> {
    let (fg, _bg) = match variant {
        ButtonVariant::Primary => (Color::White, theme.primary),
        ButtonVariant::Secondary => (theme.fg, theme.muted),
        ButtonVariant::Success => (Color::White, theme.success),
        ButtonVariant::Danger => (Color::White, theme.error),
        ButtonVariant::Ghost => (theme.fg, Color::Reset),
    };
    Line::from(Span::styled(
        format!(" {} ", label),
        Style::default().fg(fg),
    ))
}

/// Render a badge with variant
#[allow(dead_code)]
pub fn render_badge(text: &str, variant: BadgeVariant, theme: &Theme) -> Line<'static> {
    let color = match variant {
        BadgeVariant::Info => theme.primary,
        BadgeVariant::Success => theme.success,
        BadgeVariant::Warning => theme.warning,
        BadgeVariant::Error => theme.error,
        BadgeVariant::Muted => theme.muted,
    };
    Line::from(Span::styled(
        format!(" {} ", text),
        Style::default().fg(Color::White).bg(color),
    ))
}

/// Render an alert box with icon, title, and message
#[allow(dead_code)]
pub fn render_alert(
    title: &str,
    message: &str,
    variant: AlertVariant,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let (icon, color) = match variant {
        AlertVariant::Info => ("ℹ", theme.primary),
        AlertVariant::Success => ("✔", theme.success),
        AlertVariant::Warning => ("⚠", theme.warning),
        AlertVariant::Error => ("✖", theme.error),
    };
    vec![
        Line::from(vec![
            Span::styled(format!("  {} ", icon), Style::default().fg(color)),
            Span::styled(
                title.to_string(),
                Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
            ),
        ]),
        Line::from(Span::styled(
            format!("    {}", message),
            Style::default().fg(theme.muted),
        )),
    ]
}

/// Render a section heading with icon
#[allow(dead_code)]
pub fn render_section_heading(icon: &str, text: &str, theme: &Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{} ", icon), Style::default().fg(theme.accent)),
        Span::styled(
            text.to_string(),
            Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
        ),
    ])
}

/// Render an empty state placeholder
pub fn render_empty_state(
    icon: &str,
    title: &str,
    message: &str,
    theme: &Theme,
) -> Vec<Line<'static>> {
    vec![
        Line::from(""),
        Line::from(Span::styled(
            format!("    {}", icon),
            Style::default().fg(theme.muted),
        )),
        Line::from(Span::styled(
            format!("    {}", title),
            Style::default().fg(theme.fg).add_modifier(Modifier::BOLD),
        )),
        Line::from(Span::styled(
            format!("    {}", message),
            Style::default().fg(theme.muted),
        )),
        Line::from(""),
    ]
}

/// Render a card/panel frame with border
#[allow(dead_code)]
pub fn render_card_frame<'a>(
    title: &str,
    content_lines: Vec<Line<'a>>,
    theme: &Theme,
) -> Vec<Line<'a>> {
    let width: usize = 60;
    let border = "─".repeat(width.saturating_sub(2));
    let mut lines = vec![Line::from(vec![
        Span::styled("┌", Style::default().fg(theme.muted)),
        Span::styled(
            format!(" {} ", title),
            Style::default()
                .fg(theme.accent)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled(border.clone(), Style::default().fg(theme.muted)),
    ])];
    for line in content_lines {
        let mut new_spans: Vec<Span<'a>> =
            vec![Span::styled("│ ", Style::default().fg(theme.muted))];
        for span in line.spans {
            new_spans.push(span);
        }
        lines.push(Line::from(new_spans));
    }
    lines.push(Line::from(Span::styled(
        format!("└{}", "─".repeat(width.saturating_sub(1))),
        Style::default().fg(theme.muted),
    )));
    lines
}

/// Render a file table row with columns
#[allow(dead_code)]
pub fn render_file_table_row(
    icon: &str,
    name: &str,
    size: &str,
    progress: f64,
    status: &str,
    theme: &Theme,
) -> Line<'static> {
    let status_color = match status {
        "Done" => theme.success,
        "Copying" | "Processing" => theme.accent,
        "Error" => theme.error,
        _ => theme.muted,
    };
    let bar = make_gauge_bar(progress, 10);
    Line::from(vec![
        Span::styled(format!("{} ", icon), Style::default().fg(theme.accent)),
        Span::styled(
            format!("{:<30}", truncate_str(name, 30)),
            Style::default().fg(theme.fg),
        ),
        Span::styled(format!("{:>10}", size), Style::default().fg(theme.muted)),
        Span::styled(format!(" [{}]", bar), Style::default().fg(theme.primary)),
        Span::styled(
            format!(" {:>5.1}%", progress * 100.0),
            Style::default().fg(theme.accent),
        ),
        Span::styled(format!(" {}", status), Style::default().fg(status_color)),
    ])
}

/// Render enhanced progress display with speed, ETA, elapsed
#[allow(dead_code, clippy::too_many_arguments)]
pub fn render_progress_detail(
    progress: f64,
    speed: &str,
    eta: &str,
    elapsed: &str,
    files_done: usize,
    total_files: usize,
    current_file: &str,
    theme: &Theme,
) -> Vec<Line<'static>> {
    let bar = make_gauge_bar(progress, 40);
    vec![
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                format!("{:>5.1}%", progress * 100.0),
                Style::default()
                    .fg(theme.accent)
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(format!(" [{}]", bar), Style::default().fg(theme.primary)),
        ]),
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(format!("Speed: {}", speed), Style::default().fg(theme.fg)),
            Span::styled("  │  ", Style::default().fg(theme.muted)),
            Span::styled(format!("ETA: {}", eta), Style::default().fg(theme.fg)),
            Span::styled("  │  ", Style::default().fg(theme.muted)),
            Span::styled(
                format!("Elapsed: {}", elapsed),
                Style::default().fg(theme.fg),
            ),
        ]),
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                format!("Files: {}/{}", files_done, total_files),
                Style::default().fg(theme.muted),
            ),
        ]),
        Line::from(vec![
            Span::styled("  ", Style::default()),
            Span::styled(
                format!("Current: {}", truncate_str(current_file, 50)),
                Style::default().fg(theme.muted),
            ),
        ]),
    ]
}

/// Render toast overlay (top-right corner)
pub fn render_toasts(toasts: &[Toast], area: Rect, f: &mut Frame, theme: &Theme) {
    let max_toasts = 3;
    let toast_width = 40.min(area.width.saturating_sub(2) as usize);
    let start_y = area.y + 1;

    for (i, toast) in toasts.iter().take(max_toasts).enumerate() {
        let y = start_y + i as u16 * 3;
        if y + 2 > area.y + area.height {
            break;
        }
        let x = area.x + area.width.saturating_sub(toast_width as u16 + 2);
        let toast_area = Rect::new(x, y, toast_width as u16 + 2, 2);

        let (icon, color) = match toast.toast_type {
            ToastType::Info => ("ℹ", theme.primary),
            ToastType::Success => ("✔", theme.success),
            ToastType::Error => ("✖", theme.error),
            ToastType::Warning => ("⚠", theme.warning),
        };

        let toast_block = Paragraph::new(Line::from(vec![
            Span::styled(format!("{} ", icon), Style::default().fg(color)),
            Span::styled(
                truncate_str(&toast.message, toast_width.saturating_sub(4)),
                Style::default().fg(theme.fg),
            ),
        ]))
        .block(
            Block::default()
                .borders(Borders::ALL)
                .border_style(Style::default().fg(color)),
        );
        f.render_widget(toast_block, toast_area);
    }
}

/// Truncate string to max length with ellipsis (UTF-8 safe)
pub fn truncate_str(s: &str, max_len: usize) -> String {
    if s.len() <= max_len {
        s.to_string()
    } else {
        let trunc = max_len.saturating_sub(3);
        // Find a valid char boundary at or before trunc
        let end = if trunc >= s.len() {
            s.len()
        } else {
            let mut boundary = trunc;
            while boundary > 0 && !s.is_char_boundary(boundary) {
                boundary -= 1;
            }
            boundary
        };
        format!("{}...", &s[..end])
    }
}

// Animated gauge characters
const GAUGE_CHARS: &[char] = &['░', '▒', '▓', '█'];
pub const SPINNER_CHARS: &[char] = &['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];

/// Create a gauge bar string
pub fn make_gauge_bar(ratio: f64, width: usize) -> String {
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

/// Format file size to human readable string
pub fn format_size(bytes: u64) -> String {
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

/// Format duration in seconds to human readable string
pub fn format_duration(secs: f64) -> String {
    if secs >= 3600.0 {
        let h = (secs / 3600.0) as u64;
        let m = ((secs % 3600.0) / 60.0) as u64;
        let s = (secs % 60.0) as u64;
        format!("{}h{}m{}s", h, m, s)
    } else if secs >= 60.0 {
        let m = (secs / 60.0) as u64;
        let s = (secs % 60.0) as u64;
        format!("{}m{}s", m, s)
    } else {
        format!("{:.1}s", secs)
    }
}
