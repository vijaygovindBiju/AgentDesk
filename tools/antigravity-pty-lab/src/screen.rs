//! VT100 / ANSI terminal screen emulator for the Antigravity PTY Lab.
//!
//! Maintains an in-memory 2D grid of character cells with styling attributes,
//! tracks cursor position, handles alternate screen buffer switching, and provides
//! inspection methods for structured state detection.

use std::cmp::min;

/// SGR visual attributes for a character cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct CellAttributes {
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub reverse: bool,
    pub fg: Option<u8>,
    pub bg: Option<u8>,
}

/// A single cell in the terminal grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub attrs: CellAttributes,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            attrs: CellAttributes::default(),
        }
    }
}

/// Immutable snapshot of the terminal screen for state analysis.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScreenSnapshot {
    pub cols: u16,
    pub rows: u16,
    pub cursor_row: u16,
    pub cursor_col: u16,
    pub in_alt_screen: bool,
    pub cursor_visible: bool,
    pub lines: Vec<String>,
    pub reversed_lines: Vec<(u16, String)>,
}

/// 2D VT100/ANSI screen emulator.
pub struct Screen {
    pub cols: u16,
    pub rows: u16,
    pub cursor_row: u16,
    pub cursor_col: u16,
    pub saved_cursor: (u16, u16),
    pub in_alt_screen: bool,
    pub cursor_visible: bool,

    main_grid: Vec<Vec<Cell>>,
    alt_grid: Vec<Vec<Cell>>,
    current_attrs: CellAttributes,

    // Internal parser state
    parser_state: ParserState,
    csi_params: Vec<u16>,
    csi_current_param: Option<u16>,
    csi_is_private: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ParserState {
    Ground,
    Escape,
    Csi,
    Osc,
}

impl Screen {
    pub fn new(cols: u16, rows: u16) -> Self {
        let main_grid = vec![vec![Cell::default(); cols as usize]; rows as usize];
        let alt_grid = vec![vec![Cell::default(); cols as usize]; rows as usize];
        Self {
            cols,
            rows,
            cursor_row: 0,
            cursor_col: 0,
            saved_cursor: (0, 0),
            in_alt_screen: false,
            cursor_visible: true,
            main_grid,
            alt_grid,
            current_attrs: CellAttributes::default(),
            parser_state: ParserState::Ground,
            csi_params: Vec::new(),
            csi_current_param: None,
            csi_is_private: false,
        }
    }

    fn grid_mut(&mut self) -> &mut Vec<Vec<Cell>> {
        if self.in_alt_screen {
            &mut self.alt_grid
        } else {
            &mut self.main_grid
        }
    }

    fn grid(&self) -> &Vec<Vec<Cell>> {
        if self.in_alt_screen {
            &self.alt_grid
        } else {
            &self.main_grid
        }
    }

    /// Process a stream of raw bytes received from the PTY.
    pub fn process_bytes(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.process_byte(b);
        }
    }

    /// Process a single byte through the VT100 state machine.
    pub fn process_byte(&mut self, b: u8) {
        match self.parser_state {
            ParserState::Ground => match b {
                0x1b => {
                    self.parser_state = ParserState::Escape;
                }
                b'\r' => {
                    self.cursor_col = 0;
                }
                b'\n' => {
                    self.line_feed();
                }
                0x08 => {
                    self.cursor_col = self.cursor_col.saturating_sub(1);
                }
                b'\t' => {
                    let next = (self.cursor_col / 8 + 1) * 8;
                    self.cursor_col = min(next, self.cols.saturating_sub(1));
                }
                0x07 => {}        // Bell, ignore
                0x00..=0x1f => {} // Ignore unhandled control characters
                _ => {
                    let ch = b as char;
                    self.put_char(ch);
                }
            },
            ParserState::Escape => match b {
                b'[' => {
                    self.parser_state = ParserState::Csi;
                    self.csi_params.clear();
                    self.csi_current_param = None;
                    self.csi_is_private = false;
                }
                b']' => {
                    self.parser_state = ParserState::Osc;
                }
                b'7' => {
                    self.saved_cursor = (self.cursor_row, self.cursor_col);
                    self.parser_state = ParserState::Ground;
                }
                b'8' => {
                    self.cursor_row = min(self.saved_cursor.0, self.rows.saturating_sub(1));
                    self.cursor_col = min(self.saved_cursor.1, self.cols.saturating_sub(1));
                    self.parser_state = ParserState::Ground;
                }
                b'M' => {
                    // Reverse index (scroll down if at top)
                    if self.cursor_row == 0 {
                        self.scroll_down();
                    } else {
                        self.cursor_row -= 1;
                    }
                    self.parser_state = ParserState::Ground;
                }
                _ => {
                    self.parser_state = ParserState::Ground;
                }
            },
            ParserState::Csi => match b {
                b'0'..=b'9' => {
                    let digit = (b - b'0') as u16;
                    let current = self.csi_current_param.unwrap_or(0);
                    self.csi_current_param = Some(current.saturating_mul(10).saturating_add(digit));
                }
                b';' => {
                    self.csi_params.push(self.csi_current_param.unwrap_or(0));
                    self.csi_current_param = None;
                }
                b'?' => {
                    self.csi_is_private = true;
                }
                _ => {
                    // Final character of CSI sequence
                    if let Some(param) = self.csi_current_param {
                        self.csi_params.push(param);
                    }
                    self.execute_csi(b);
                    self.parser_state = ParserState::Ground;
                }
            },
            ParserState::Osc => {
                // String terminator: BEL (0x07) or ST (\x1b\)
                if b == 0x07 || b == 0x1b {
                    self.parser_state = ParserState::Ground;
                }
            }
        }
    }

    fn execute_csi(&mut self, cmd: u8) {
        match cmd {
            b'H' | b'f' => {
                // Cursor position: row;col (1-indexed, default 1)
                let row = self.csi_params.first().copied().unwrap_or(1).max(1) - 1;
                let col = self.csi_params.get(1).copied().unwrap_or(1).max(1) - 1;
                self.cursor_row = min(row, self.rows.saturating_sub(1));
                self.cursor_col = min(col, self.cols.saturating_sub(1));
            }
            b'A' => {
                // Cursor Up
                let n = self.csi_params.first().copied().unwrap_or(1).max(1);
                self.cursor_row = self.cursor_row.saturating_sub(n);
            }
            b'B' => {
                // Cursor Down
                let n = self.csi_params.first().copied().unwrap_or(1).max(1);
                self.cursor_row = min(self.cursor_row + n, self.rows.saturating_sub(1));
            }
            b'C' => {
                // Cursor Forward
                let n = self.csi_params.first().copied().unwrap_or(1).max(1);
                self.cursor_col = min(self.cursor_col + n, self.cols.saturating_sub(1));
            }
            b'D' => {
                // Cursor Back
                let n = self.csi_params.first().copied().unwrap_or(1).max(1);
                self.cursor_col = self.cursor_col.saturating_sub(n);
            }
            b'E' => {
                // Cursor Next Line
                let n = self.csi_params.first().copied().unwrap_or(1).max(1);
                self.cursor_row = min(self.cursor_row + n, self.rows.saturating_sub(1));
                self.cursor_col = 0;
            }
            b'F' => {
                // Cursor Previous Line
                let n = self.csi_params.first().copied().unwrap_or(1).max(1);
                self.cursor_row = self.cursor_row.saturating_sub(n);
                self.cursor_col = 0;
            }
            b'G' => {
                // Cursor Horizontal Absolute (1-indexed)
                let col = self.csi_params.first().copied().unwrap_or(1).max(1) - 1;
                self.cursor_col = min(col, self.cols.saturating_sub(1));
            }
            b'd' => {
                // Line Position Absolute (1-indexed)
                let row = self.csi_params.first().copied().unwrap_or(1).max(1) - 1;
                self.cursor_row = min(row, self.rows.saturating_sub(1));
            }
            b'J' => {
                // Erase in display
                let mode = self.csi_params.first().copied().unwrap_or(0);
                self.erase_display(mode);
            }
            b'K' => {
                // Erase in line
                let mode = self.csi_params.first().copied().unwrap_or(0);
                self.erase_line(mode);
            }
            b'm' => {
                // SGR (Select Graphic Rendition)
                self.execute_sgr();
            }
            b'h' if self.csi_is_private => {
                // Private mode set
                match self.csi_params.first().copied() {
                    Some(1049) | Some(47) => {
                        self.in_alt_screen = true;
                    }
                    Some(25) => {
                        self.cursor_visible = true;
                    }
                    _ => {}
                }
            }
            b'l' if self.csi_is_private => {
                // Private mode reset
                match self.csi_params.first().copied() {
                    Some(1049) | Some(47) => {
                        self.in_alt_screen = false;
                    }
                    Some(25) => {
                        self.cursor_visible = false;
                    }
                    _ => {}
                }
            }
            b'S' => {
                // Scroll up
                let n = self.csi_params.first().copied().unwrap_or(1).max(1);
                for _ in 0..n {
                    self.scroll_up();
                }
            }
            b'T' => {
                // Scroll down
                let n = self.csi_params.first().copied().unwrap_or(1).max(1);
                for _ in 0..n {
                    self.scroll_down();
                }
            }
            _ => {}
        }
    }

    fn execute_sgr(&mut self) {
        if self.csi_params.is_empty() {
            self.current_attrs = CellAttributes::default();
            return;
        }

        let mut i = 0;
        while i < self.csi_params.len() {
            match self.csi_params[i] {
                0 => self.current_attrs = CellAttributes::default(),
                1 => self.current_attrs.bold = true,
                2 => self.current_attrs.dim = true,
                3 => self.current_attrs.italic = true,
                4 => self.current_attrs.underline = true,
                7 => self.current_attrs.reverse = true,
                22 => {
                    self.current_attrs.bold = false;
                    self.current_attrs.dim = false;
                }
                23 => self.current_attrs.italic = false,
                24 => self.current_attrs.underline = false,
                27 => self.current_attrs.reverse = false,
                30..=37 => self.current_attrs.fg = Some((self.csi_params[i] - 30) as u8),
                39 => self.current_attrs.fg = None,
                40..=47 => self.current_attrs.bg = Some((self.csi_params[i] - 40) as u8),
                49 => self.current_attrs.bg = None,
                90..=97 => self.current_attrs.fg = Some((self.csi_params[i] - 90 + 8) as u8),
                100..=107 => self.current_attrs.bg = Some((self.csi_params[i] - 100 + 8) as u8),
                38 => {
                    // Extended foreground color: 38;5;n or 38;2;r;g;b
                    if i + 2 < self.csi_params.len() && self.csi_params[i + 1] == 5 {
                        self.current_attrs.fg = Some(self.csi_params[i + 2] as u8);
                        i += 2;
                    } else if i + 4 < self.csi_params.len() && self.csi_params[i + 1] == 2 {
                        // Truncate 24-bit to 8-bit approximation or store as placeholder
                        self.current_attrs.fg = Some(self.csi_params[i + 2] as u8);
                        i += 4;
                    }
                }
                48 => {
                    // Extended background color: 48;5;n or 48;2;r;g;b
                    if i + 2 < self.csi_params.len() && self.csi_params[i + 1] == 5 {
                        self.current_attrs.bg = Some(self.csi_params[i + 2] as u8);
                        i += 2;
                    } else if i + 4 < self.csi_params.len() && self.csi_params[i + 1] == 2 {
                        self.current_attrs.bg = Some(self.csi_params[i + 2] as u8);
                        i += 4;
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }

    fn put_char(&mut self, ch: char) {
        let r = self.cursor_row as usize;
        let c = self.cursor_col as usize;
        let rows = self.rows as usize;
        let cols = self.cols as usize;

        if r < rows && c < cols {
            self.grid_mut()[r][c] = Cell {
                ch,
                attrs: self.current_attrs,
            };
        }

        self.cursor_col += 1;
        if self.cursor_col >= self.cols {
            self.cursor_col = 0;
            self.line_feed();
        }
    }

    fn line_feed(&mut self) {
        if self.cursor_row + 1 >= self.rows {
            self.scroll_up();
        } else {
            self.cursor_row += 1;
        }
    }

    fn scroll_up(&mut self) {
        let cols = self.cols as usize;
        let grid = self.grid_mut();
        if !grid.is_empty() {
            grid.remove(0);
            grid.push(vec![Cell::default(); cols]);
        }
    }

    fn scroll_down(&mut self) {
        let cols = self.cols as usize;
        let grid = self.grid_mut();
        if !grid.is_empty() {
            grid.pop();
            grid.insert(0, vec![Cell::default(); cols]);
        }
    }

    #[allow(clippy::needless_range_loop)]
    fn erase_display(&mut self, mode: u16) {
        let rows = self.rows as usize;
        let cols = self.cols as usize;
        let cr = self.cursor_row as usize;
        let cc = self.cursor_col as usize;
        let grid = self.grid_mut();

        match mode {
            0 => {
                // Clear from cursor to end of screen
                for c in cc..cols {
                    if cr < rows {
                        grid[cr][c] = Cell::default();
                    }
                }
                for r in (cr + 1)..rows {
                    for c in 0..cols {
                        grid[r][c] = Cell::default();
                    }
                }
            }
            1 => {
                // Clear from beginning of screen to cursor
                for r in 0..cr {
                    for c in 0..cols {
                        grid[r][c] = Cell::default();
                    }
                }
                for c in 0..=cc {
                    if cr < rows && c < cols {
                        grid[cr][c] = Cell::default();
                    }
                }
            }
            2 | 3 => {
                // Clear entire screen
                for r in 0..rows {
                    for c in 0..cols {
                        grid[r][c] = Cell::default();
                    }
                }
            }
            _ => {}
        }
    }

    #[allow(clippy::needless_range_loop)]
    fn erase_line(&mut self, mode: u16) {
        let r = self.cursor_row as usize;
        let cc = self.cursor_col as usize;
        let cols = self.cols as usize;
        let rows = self.rows as usize;

        if r >= rows {
            return;
        }
        let grid = self.grid_mut();

        match mode {
            0 => {
                // Clear from cursor to end of line
                for c in cc..cols {
                    grid[r][c] = Cell::default();
                }
            }
            1 => {
                // Clear from start of line to cursor
                for c in 0..=cc {
                    if c < cols {
                        grid[r][c] = Cell::default();
                    }
                }
            }
            2 => {
                // Clear entire line
                for c in 0..cols {
                    grid[r][c] = Cell::default();
                }
            }
            _ => {}
        }
    }

    /// Return the rendered plain text of a specific row.
    pub fn row_text(&self, row: u16) -> String {
        let r = row as usize;
        let grid = self.grid();
        if r >= grid.len() {
            return String::new();
        }
        let s: String = grid[r].iter().map(|c| c.ch).collect();
        s.trim_end().to_string()
    }

    /// Return plain text of all screen rows.
    pub fn all_lines(&self) -> Vec<String> {
        (0..self.rows).map(|r| self.row_text(r)).collect()
    }

    /// Return lines containing cells formatted with reverse video.
    pub fn reversed_lines(&self) -> Vec<(u16, String)> {
        let mut res = Vec::new();
        let grid = self.grid();
        for (r, row) in grid.iter().enumerate() {
            let has_reverse = row.iter().any(|cell| cell.attrs.reverse && cell.ch != ' ');
            if has_reverse {
                let text: String = row.iter().map(|c| c.ch).collect();
                res.push((r as u16, text.trim().to_string()));
            }
        }
        res
    }

    /// Return an immutable snapshot of current visual screen state.
    pub fn snapshot(&self) -> ScreenSnapshot {
        ScreenSnapshot {
            cols: self.cols,
            rows: self.rows,
            cursor_row: self.cursor_row,
            cursor_col: self.cursor_col,
            in_alt_screen: self.in_alt_screen,
            cursor_visible: self.cursor_visible,
            lines: self.all_lines(),
            reversed_lines: self.reversed_lines(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_plain_text_and_cursor_movement() {
        let mut screen = Screen::new(80, 24);
        screen.process_bytes(b"Hello World!\r\nSecond line");
        assert_eq!(screen.row_text(0), "Hello World!");
        assert_eq!(screen.row_text(1), "Second line");
        assert_eq!(screen.cursor_row, 1);
        assert_eq!(screen.cursor_col, 11);
    }

    #[test]
    fn test_ansi_color_and_reverse_attributes() {
        let mut screen = Screen::new(80, 24);
        // \x1b[7m = reverse, \x1b[0m = reset
        screen.process_bytes(b"\x1b[7mSelected Option\x1b[0m Normal Option");
        assert_eq!(screen.row_text(0), "Selected Option Normal Option");

        let rev = screen.reversed_lines();
        assert_eq!(rev.len(), 1);
        assert_eq!(rev[0].0, 0);
        assert!(rev[0].1.contains("Selected Option"));
    }

    #[test]
    fn test_alternate_screen_buffer_switching() {
        let mut screen = Screen::new(80, 24);
        screen.process_bytes(b"Main buffer text");
        assert_eq!(screen.row_text(0), "Main buffer text");

        // Switch to alternate screen
        screen.process_bytes(b"\x1b[?1049h\x1b[H\x1b[2JAlt buffer text");
        assert!(screen.in_alt_screen);
        assert_eq!(screen.row_text(0), "Alt buffer text");

        // Switch back to main screen
        screen.process_bytes(b"\x1b[?1049l");
        assert!(!screen.in_alt_screen);
        assert_eq!(screen.row_text(0), "Main buffer text");
    }

    #[test]
    fn test_erase_line_and_display() {
        let mut screen = Screen::new(80, 24);
        screen.process_bytes(b"Line to clear\x1b[2K");
        assert_eq!(screen.row_text(0), "");
    }
}
