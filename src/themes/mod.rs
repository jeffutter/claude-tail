use std::path::PathBuf;

use anyhow::{Result, anyhow};
use ratatui::style::{Color, Modifier, Style};
use serde::Deserialize;

use crate::ui::Theme;

/// Base16 color scheme (16 colors: base00-base0F)
#[derive(Debug, Deserialize)]
pub struct Base16Scheme {
    pub scheme: String,
    #[serde(default)]
    pub author: String,
    pub base00: String,
    pub base01: String,
    pub base02: String,
    pub base03: String,
    pub base04: String,
    pub base05: String,
    pub base06: String,
    pub base07: String,
    pub base08: String,
    pub base09: String,
    #[serde(rename = "base0A")]
    pub base0a: String,
    #[serde(rename = "base0B")]
    pub base0b: String,
    #[serde(rename = "base0C")]
    pub base0c: String,
    #[serde(rename = "base0D")]
    pub base0d: String,
    #[serde(rename = "base0E")]
    pub base0e: String,
    #[serde(rename = "base0F")]
    pub base0f: String,
}

impl Base16Scheme {
    /// Parse a hex color string (with or without #) to RGB
    fn parse_hex(hex: &str) -> Result<Color> {
        let hex = hex.trim_start_matches('#');
        if hex.len() != 6 {
            return Err(anyhow!("Invalid hex color: {}", hex));
        }
        let r = u8::from_str_radix(&hex[0..2], 16)?;
        let g = u8::from_str_radix(&hex[2..4], 16)?;
        let b = u8::from_str_radix(&hex[4..6], 16)?;
        Ok(Color::Rgb(r, g, b))
    }

    /// Convert base16 scheme to Theme
    pub fn to_theme(&self) -> Result<Theme> {
        // Base16 color mapping:
        // base00 - Default Background
        // base01 - Lighter Background (status bars, line numbers)
        // base02 - Selection Background
        // base03 - Comments, Invisibles, Line Highlighting
        // base04 - Dark Foreground (status bars)
        // base05 - Default Foreground, Caret, Delimiters
        // base06 - Light Foreground (not often used)
        // base07 - Light Background (not often used)
        // base08 - Variables, XML Tags, Markup Link Text, Diff Deleted
        // base09 - Integers, Boolean, Constants, Markup Link Url
        // base0A - Classes, Markup Bold, Search Text Background
        // base0B - Strings, Inherited Class, Markup Code, Diff Inserted
        // base0C - Support, Regular Expressions, Escape Characters
        // base0D - Functions, Methods, Attribute IDs, Headings
        // base0E - Keywords, Storage, Selector, Diff Changed
        // base0F - Deprecated, Special

        let _bg = Self::parse_hex(&self.base00)?;
        let bg_light = Self::parse_hex(&self.base01)?;
        let selection = Self::parse_hex(&self.base02)?;
        let comment = Self::parse_hex(&self.base03)?;
        let fg_dark = Self::parse_hex(&self.base04)?;
        let fg = Self::parse_hex(&self.base05)?;
        let _fg_light = Self::parse_hex(&self.base06)?;
        let red = Self::parse_hex(&self.base08)?;
        let _orange = Self::parse_hex(&self.base09)?;
        let yellow = Self::parse_hex(&self.base0a)?;
        let green = Self::parse_hex(&self.base0b)?;
        let cyan = Self::parse_hex(&self.base0c)?;
        let blue = Self::parse_hex(&self.base0d)?;
        let purple = Self::parse_hex(&self.base0e)?;

        Ok(Theme {
            border: Style::default().fg(comment),
            border_focused: Style::default().fg(cyan),
            title: Style::default().fg(fg),
            title_focused: Style::default().fg(cyan).add_modifier(Modifier::BOLD),
            selected: Style::default().bg(selection).add_modifier(Modifier::BOLD),
            user_message: Style::default().fg(fg),
            user_label: Style::default().fg(green).add_modifier(Modifier::BOLD),
            assistant_text: Style::default().fg(fg),
            assistant_label: Style::default().fg(purple).add_modifier(Modifier::BOLD),
            tool_name: Style::default().fg(yellow).add_modifier(Modifier::BOLD),
            tool_input: Style::default().fg(fg_dark),
            tool_result: Style::default().fg(cyan),
            tool_error: Style::default().fg(red),
            thinking: Style::default().fg(comment),
            thinking_collapsed: Style::default().fg(comment).add_modifier(Modifier::ITALIC),
            hook_event: Style::default().fg(blue),
            agent_spawn: Style::default().fg(purple),
            status_bar: Style::default().bg(bg_light).fg(fg),
            key_hint: Style::default().fg(cyan),
            timestamp: Style::default().fg(comment),
        })
    }
}

// Bundled preset themes
const TOKYONIGHT_STORM: &str = include_str!("presets/tokyonight-storm.yaml");
const CATPPUCCIN_MOCHA: &str = include_str!("presets/catppuccin-mocha.yaml");
const DRACULA: &str = include_str!("presets/dracula.yaml");
const NORD: &str = include_str!("presets/nord.yaml");
const GRUVBOX_DARK: &str = include_str!("presets/gruvbox-dark.yaml");
const SOLARIZED_DARK: &str = include_str!("presets/solarized-dark.yaml");

/// Get list of bundled theme names
pub fn bundled_themes() -> Vec<&'static str> {
    vec![
        "tokyonight-storm",
        "catppuccin-mocha",
        "dracula",
        "nord",
        "gruvbox-dark",
        "solarized-dark",
    ]
}

/// Load a bundled theme by name
fn load_bundled(name: &str) -> Option<&'static str> {
    match name {
        "tokyonight-storm" => Some(TOKYONIGHT_STORM),
        "catppuccin-mocha" => Some(CATPPUCCIN_MOCHA),
        "dracula" => Some(DRACULA),
        "nord" => Some(NORD),
        "gruvbox-dark" => Some(GRUVBOX_DARK),
        "solarized-dark" => Some(SOLARIZED_DARK),
        _ => None,
    }
}

/// Get custom themes directory path
fn custom_themes_dir() -> Option<PathBuf> {
    dirs::config_dir().map(|p| p.join("claude-tail").join("themes"))
}

/// List available themes (bundled + custom)
pub fn list_themes() -> Vec<String> {
    let mut themes: Vec<String> = bundled_themes().into_iter().map(String::from).collect();

    // Add custom themes
    if let Some(dir) = custom_themes_dir()
        && let Ok(entries) = std::fs::read_dir(dir)
    {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.extension().is_some_and(|e| e == "yaml" || e == "yml")
                && let Some(stem) = path.file_stem()
            {
                let name = stem.to_string_lossy().to_string();
                if !themes.contains(&name) {
                    themes.push(name);
                }
            }
        }
    }

    themes.sort();
    themes
}

/// Load a theme by name (checks bundled first, then custom directory)
pub fn load_theme(name: &str) -> Result<Theme> {
    // Try bundled themes first
    if let Some(yaml) = load_bundled(name) {
        let scheme: Base16Scheme = serde_yaml::from_str(yaml)?;
        return scheme.to_theme();
    }

    // Try custom themes directory
    if let Some(dir) = custom_themes_dir() {
        for ext in ["yaml", "yml"] {
            let path = dir.join(format!("{}.{}", name, ext));
            if path.exists() {
                let content = std::fs::read_to_string(&path)?;
                let scheme: Base16Scheme = serde_yaml::from_str(&content)?;
                return scheme.to_theme();
            }
        }
    }

    Err(anyhow!(
        "Theme '{}' not found. Available themes: {}",
        name,
        list_themes().join(", ")
    ))
}
