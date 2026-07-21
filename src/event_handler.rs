//! イベント処理モジュール
//!
//! キーボード、マウス、その他のイベントを処理します。

use anyhow::Result;
use crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use std::io;

use crate::AppState;

/// イベント処理結果
pub enum EventResult {
    /// 続行
    Continue,
    /// 終了
    Quit,
    /// 画面再描画
    Redraw,
}

/// イベントハンドラ
pub struct EventHandler;

impl EventHandler {
    /// キーイベントを処理
    ///
    /// # Arguments
    /// * `key_code` - キーコード
    /// * `modifiers` - 修飾キー
    /// * `app_state` - 現在のアプリケーション状態
    ///
    /// # Returns
    /// 処理結果
    pub fn handle_key_event(
        key_code: KeyCode,
        modifiers: KeyModifiers,
        app_state: &AppState,
    ) -> EventResult {
        // Global keys
        match key_code {
            KeyCode::Char('q') if !modifiers.contains(KeyModifiers::CONTROL) => {
                if *app_state == AppState::Menu {
                    return EventResult::Quit;
                }
            }
            KeyCode::Char('c') if modifiers.contains(KeyModifiers::CONTROL) => {
                return EventResult::Quit;
            }
            _ => {}
        }

        EventResult::Continue
    }

    /// メインループのイベント処理
    ///
    /// # Arguments
    /// * `timeout` - タイムアウト時間
    ///
    /// # Returns
    /// イベント（存在する場合）
    pub fn poll_event(timeout: std::time::Duration) -> Result<Option<Event>> {
        if event::poll(timeout)? {
            Ok(Some(event::read()?))
        } else {
            Ok(None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_event_result_variants() {
        let continue_result = EventResult::Continue;
        let quit_result = EventResult::Quit;
        let redraw_result = EventResult::Redraw;

        // Just verify they can be created
        match continue_result {
            EventResult::Continue => {}
            _ => panic!("Expected Continue"),
        }
        match quit_result {
            EventResult::Quit => {}
            _ => panic!("Expected Quit"),
        }
        match redraw_result {
            EventResult::Redraw => {}
            _ => panic!("Expected Redraw"),
        }
    }

    #[test]
    fn test_poll_event_no_timeout() {
        // This test just verifies the function signature works
        let result = EventHandler::poll_event(std::time::Duration::from_millis(0));
        assert!(result.is_ok());
    }
}
