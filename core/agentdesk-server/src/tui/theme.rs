//! Unified visual theme for AgentDesk TUI, matching the mobile dark theme design system.
//! Aligned with `mobile/lib/ui/app_theme.dart`.

use ratatui::style::{Color, Modifier, Style};
use ratatui::widgets::{Block, BorderType, Borders};

pub struct Theme;

impl Theme {
    // -------------------------------------------------------------------------
    // Brand & Status Colors
    // -------------------------------------------------------------------------
    /// Primary emerald brand accent (#10B981) - Used for primary actions, success, running status
    pub const EMERALD: Color = Color::Rgb(16, 185, 129);

    /// Secondary cyan brand accent (#06B6D4) - Used for logo mark, highlights, active tabs, focus
    pub const CYAN: Color = Color::Rgb(6, 182, 212);

    /// Warning amber accent (#F59E0B) - Used for attention requests, warnings, input required
    pub const AMBER: Color = Color::Rgb(245, 158, 11);

    /// Error red accent (#EF4444) - Used for critical errors, deny actions, offline status
    pub const RED: Color = Color::Rgb(239, 68, 68);

    /// Effort purple accent (#8B5CF6) - Used for special tags, tokens, metrics
    pub const PURPLE: Color = Color::Rgb(139, 92, 246);

    // -------------------------------------------------------------------------
    // Surface & Border Colors
    // -------------------------------------------------------------------------
    /// Dark background (#0A0D12)
    pub const DARK_BG: Color = Color::Rgb(10, 13, 18);

    /// Surface card (#161B22)
    pub const SURFACE: Color = Color::Rgb(22, 27, 34);

    /// Elevated surface (#1E242C)
    pub const SURFACE_ELEVATED: Color = Color::Rgb(30, 36, 44);

    /// Card border (#30363D)
    pub const BORDER: Color = Color::Rgb(48, 54, 61);

    /// Subtle border (#21262D)
    pub const BORDER_SUBTLE: Color = Color::Rgb(33, 38, 45);

    // -------------------------------------------------------------------------
    // Text Hierarchy
    // -------------------------------------------------------------------------
    /// Primary text (#F0F6FC) - High contrast
    pub const TEXT_PRIMARY: Color = Color::Rgb(240, 246, 252);

    /// Secondary text (#8B949E) - Labels, inactive items, descriptions
    pub const TEXT_SECONDARY: Color = Color::Rgb(139, 148, 158);

    /// Muted text (#484F58) - Placeholders, shortcuts, separators
    pub const TEXT_MUTED: Color = Color::Rgb(72, 79, 88);

    // -------------------------------------------------------------------------
    // Reusable Typography & Styles
    // -------------------------------------------------------------------------
    pub fn brand() -> Style {
        Style::default().fg(Self::CYAN).add_modifier(Modifier::BOLD)
    }

    pub fn primary() -> Style {
        Style::default().fg(Self::EMERALD).add_modifier(Modifier::BOLD)
    }

    pub fn accent() -> Style {
        Style::default().fg(Self::CYAN)
    }

    pub fn warning() -> Style {
        Style::default().fg(Self::AMBER).add_modifier(Modifier::BOLD)
    }

    pub fn error() -> Style {
        Style::default().fg(Self::RED).add_modifier(Modifier::BOLD)
    }

    pub fn text_bold() -> Style {
        Style::default().fg(Self::TEXT_PRIMARY).add_modifier(Modifier::BOLD)
    }

    pub fn text_normal() -> Style {
        Style::default().fg(Self::TEXT_PRIMARY)
    }

    pub fn text_secondary() -> Style {
        Style::default().fg(Self::TEXT_SECONDARY)
    }

    pub fn text_muted() -> Style {
        Style::default().fg(Self::TEXT_MUTED)
    }

    pub fn shortcut_key() -> Style {
        Style::default().fg(Self::CYAN).add_modifier(Modifier::BOLD)
    }

    pub fn shortcut_desc() -> Style {
        Style::default().fg(Self::TEXT_SECONDARY)
    }

    // -------------------------------------------------------------------------
    // Component Cards & Blocks
    // -------------------------------------------------------------------------
    pub fn card(title: impl Into<ratatui::text::Line<'static>>) -> Block<'static> {
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Self::BORDER))
            .title(title)
    }

    pub fn card_active(title: impl Into<ratatui::text::Line<'static>>) -> Block<'static> {
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Self::CYAN))
            .title(title)
    }

    pub fn card_attention(title: impl Into<ratatui::text::Line<'static>>) -> Block<'static> {
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Self::AMBER))
            .title(title)
    }

    pub fn card_error(title: impl Into<ratatui::text::Line<'static>>) -> Block<'static> {
        Block::default()
            .borders(Borders::ALL)
            .border_type(BorderType::Rounded)
            .border_style(Style::default().fg(Self::RED))
            .title(title)
    }
}
