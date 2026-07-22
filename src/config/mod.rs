// Config Module - Configuration management for PixPipe
// This module handles settings, presets, and configuration persistence.

#![allow(dead_code)]

use anyhow::Result;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::PathBuf;

/// Application configuration
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AppConfig {
    pub general: GeneralConfig,
    pub processing: ProcessingConfig,
    pub ui: UiConfig,
    pub presets: HashMap<String, Preset>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneralConfig {
    pub input_dir: Option<PathBuf>,
    pub output_dir: Option<PathBuf>,
    pub log_dir: Option<PathBuf>,
    pub auto_save: bool,
    pub confirm_exit: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessingConfig {
    pub default_format: String,
    pub quality: u8,
    pub preserve_metadata: bool,
    pub create_backup: bool,
    pub max_parallel: usize,
    pub hash_algorithm: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    pub theme: String,
    pub show_preview: bool,
    pub show_statusbar: bool,
    pub show_sidebar: bool,
    pub compact_mode: bool,
    pub animation_speed: u32,
}

impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            input_dir: None,
            output_dir: None,
            log_dir: None,
            auto_save: true,
            confirm_exit: true,
        }
    }
}

impl Default for ProcessingConfig {
    fn default() -> Self {
        Self {
            default_format: "keep".to_string(),
            quality: 85,
            preserve_metadata: true,
            create_backup: true,
            max_parallel: 4,
            hash_algorithm: "sha256".to_string(),
        }
    }
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: "dark".to_string(),
            show_preview: true,
            show_statusbar: true,
            show_sidebar: false,
            compact_mode: false,
            animation_speed: 300,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preset {
    pub name: String,
    pub description: String,
    pub format: Option<String>,
    pub quality: Option<u8>,
    pub resize: Option<ResizeConfig>,
    pub filters: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResizeConfig {
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub maintain_aspect: bool,
}

impl AppConfig {
    /// Load configuration from file
    pub fn load(path: &std::path::Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(path)?;
        let config: Self = toml::from_str(&content)?;
        Ok(config)
    }

    /// Save configuration to file
    pub fn save(&self, path: &std::path::Path) -> Result<()> {
        let content = toml::to_string_pretty(self)?;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::write(path, content)?;
        Ok(())
    }

    /// Get preset by name
    pub fn get_preset(&self, name: &str) -> Option<&Preset> {
        self.presets.get(name)
    }

    /// Add or update preset
    pub fn set_preset(&mut self, name: String, preset: Preset) {
        self.presets.insert(name, preset);
    }

    /// Remove preset
    pub fn remove_preset(&mut self, name: &str) -> Option<Preset> {
        self.presets.remove(name)
    }

    /// List all preset names
    pub fn preset_names(&self) -> Vec<&str> {
        self.presets
            .keys()
            .map(std::string::String::as_str)
            .collect()
    }
}

/// Key binding configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyBindings {
    pub bindings: HashMap<String, String>,
}

impl Default for KeyBindings {
    fn default() -> Self {
        let mut bindings = HashMap::new();

        // Navigation
        bindings.insert("quit".to_string(), "q,Esc".to_string());
        bindings.insert("help".to_string(), "h,?".to_string());
        bindings.insert("menu_up".to_string(), "k,Up".to_string());
        bindings.insert("menu_down".to_string(), "j,Down".to_string());
        bindings.insert("select".to_string(), "l,Enter".to_string());
        bindings.insert("back".to_string(), "h,Backspace".to_string());

        // Processing
        bindings.insert("start".to_string(), "s".to_string());
        bindings.insert("pause".to_string(), "p".to_string());
        bindings.insert("cancel".to_string(), "c".to_string());
        bindings.insert("undo".to_string(), "u".to_string());

        // View
        bindings.insert("preview".to_string(), "v".to_string());
        bindings.insert("info".to_string(), "i".to_string());
        bindings.insert("filter".to_string(), "f".to_string());
        bindings.insert("sort".to_string(), "o".to_string());

        Self { bindings }
    }
}

impl KeyBindings {
    /// Load key bindings from file
    pub fn load(path: &std::path::Path) -> Result<Self> {
        if !path.exists() {
            return Ok(Self::default());
        }

        let content = std::fs::read_to_string(path)?;
        let bindings: Self = toml::from_str(&content)?;
        Ok(bindings)
    }

    /// Save key bindings to file
    pub fn save(&self, path: &std::path::Path) -> Result<()> {
        let content = toml::to_string_pretty(self)?;

        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        std::fs::write(path, content)?;
        Ok(())
    }

    /// Get keys for action
    pub fn get_keys(&self, action: &str) -> Vec<&str> {
        self.bindings
            .get(action)
            .map(|s| s.split(',').collect())
            .unwrap_or_default()
    }
}

/// Plugin configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PluginConfig {
    pub enabled: bool,
    pub path: PathBuf,
    pub args: Vec<String>,
    pub env: HashMap<String, String>,
}

/// Scheduled task configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScheduledTask {
    pub name: String,
    pub cron: String,
    pub action: String,
    pub args: Vec<String>,
    pub enabled: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = AppConfig::default();
        assert_eq!(config.processing.quality, 85);
        assert_eq!(config.ui.theme, "dark");
    }

    #[test]
    fn test_preset_management() {
        let mut config = AppConfig::default();

        let preset = Preset {
            name: "web".to_string(),
            description: "Web optimized".to_string(),
            format: Some("webp".to_string()),
            quality: Some(80),
            resize: Some(ResizeConfig {
                width: Some(1920),
                height: None,
                maintain_aspect: true,
            }),
            filters: vec![],
        };

        config.set_preset("web".to_string(), preset);
        assert!(config.get_preset("web").is_some());
        assert_eq!(config.get_preset("web").unwrap().quality, Some(80));
    }

    #[test]
    fn test_key_bindings() {
        let bindings = KeyBindings::default();
        let quit_keys = bindings.get_keys("quit");
        assert!(quit_keys.contains(&"q"));
        assert!(quit_keys.contains(&"Esc"));
    }

    #[test]
    fn test_preset_management_basic() {
        let mut config = AppConfig::default();
        let preset = Preset {
            name: "test".to_string(),
            description: "Test preset".to_string(),
            format: Some("jpg".to_string()),
            quality: Some(90),
            resize: None,
            filters: vec![],
        };

        config.set_preset("test".to_string(), preset);
        assert!(config.get_preset("test").is_some());
        assert_eq!(config.preset_names().len(), 1);

        let removed = config.remove_preset("test");
        assert!(removed.is_some());
        assert!(config.get_preset("test").is_none());
    }

    #[test]
    fn test_theme_config() {
        let config = UiConfig::default();
        // Default theme may vary
        assert!(!config.theme.is_empty());
        assert!(config.show_preview);
        assert!(config.show_statusbar);
    }
}
