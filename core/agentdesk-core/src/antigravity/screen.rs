//! VT100 / ANSI terminal screen emulator for Antigravity integration.
//!
//! Maintains an in-memory 2D grid of character cells with styling attributes,
//! tracks cursor position, handles alternate screen buffer switching, dynamically
//! resizes geometry, and provides visual snapshot inspection for state detection.

use std::cmp::min;

/// Underline styles for modern terminal rendering.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum UnderlineStyle {
    #[default]
    None,
    Straight,
    Double,
    Curly,
    Dotted,
    Dashed,
}

/// SGR visual attributes for a character cell.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CellAttributes {
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub underline_style: UnderlineStyle,
    pub strikethrough: bool,
    pub overline: bool,
    pub reverse: bool,
    pub fg: Option<u8>,
    pub bg: Option<u8>,
    pub fg_rgb: Option<(u8, u8, u8)>,
    pub bg_rgb: Option<(u8, u8, u8)>,
    pub underline_color: Option<(u8, u8, u8)>,
    pub hyperlink: Option<String>,
}

/// A single cell in the terminal grid.
#[derive(Debug, Clone, PartialEq, Eq)]
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
    /// Foreground colour of the first non-blank cell of each row (`None` = default colour).
    /// Lets the detector tell Antigravity's transcript roles apart (user prompt, thoughts,
    /// agent reply) from the rendered styling rather than from text heuristics.
    pub line_fg: Vec<Option<u8>>,
    /// Lines that have scrolled off the top of the main screen grid.
    pub scrollback_lines: Vec<String>,
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
    pub bracketed_paste: bool,

    main_grid: Vec<Vec<Cell>>,
    alt_grid: Vec<Vec<Cell>>,
    scrollback: Vec<String>,
    current_attrs: CellAttributes,

    // Internal parser state
    parser_state: ParserState,
    csi_params: Vec<u16>,
    csi_current_param: Option<u16>,
    csi_is_private: bool,
    csi_has_intermediate: bool,
    /// The last printable character landed in the final column; the wrap to the next row is
    /// deferred until the next printable character (xterm "pending wrap" semantics).
    pending_wrap: bool,
    utf8_buf: Vec<u8>,
    utf8_needed: usize,
    osc_buf: Vec<u8>,
    active_osc_url: Option<String>,
    pub osc52_clipboard: Option<String>,
    pub last_osc133_prompt_row: Option<u16>,
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
            bracketed_paste: false,
            main_grid,
            alt_grid,
            scrollback: Vec::new(),
            current_attrs: CellAttributes::default(),
            parser_state: ParserState::Ground,
            csi_params: Vec::new(),
            csi_current_param: None,
            csi_is_private: false,
            csi_has_intermediate: false,
            pending_wrap: false,
            utf8_buf: Vec::new(),
            utf8_needed: 0,
            osc_buf: Vec::new(),
            active_osc_url: None,
            osc52_clipboard: None,
            last_osc133_prompt_row: None,
        }
    }

    /// Resize the screen dimensions.
    pub fn resize(&mut self, cols: u16, rows: u16) {
        self.cols = cols;
        self.rows = rows;
        self.main_grid
            .resize(rows as usize, vec![Cell::default(); cols as usize]);
        for row in &mut self.main_grid {
            row.resize(cols as usize, Cell::default());
        }
        self.alt_grid
            .resize(rows as usize, vec![Cell::default(); cols as usize]);
        for row in &mut self.alt_grid {
            row.resize(cols as usize, Cell::default());
        }
        self.cursor_row = min(self.cursor_row, rows.saturating_sub(1));
        self.cursor_col = min(self.cursor_col, cols.saturating_sub(1));
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
        // UTF-8 multi-byte continuation handling (only in Ground state).
        if self.utf8_needed > 0 && self.parser_state == ParserState::Ground {
            if (0x80..0xC0).contains(&b) {
                self.utf8_buf.push(b);
                if self.utf8_buf.len() == self.utf8_needed {
                    let ch = std::str::from_utf8(&self.utf8_buf)
                        .ok()
                        .and_then(|s| s.chars().next())
                        .unwrap_or('\u{FFFD}');
                    self.utf8_buf.clear();
                    self.utf8_needed = 0;
                    self.put_char(ch);
                }
                return;
            }
            // Invalid continuation: emit replacement and reprocess this byte.
            self.utf8_buf.clear();
            self.utf8_needed = 0;
            self.put_char('\u{FFFD}');
        }

        match self.parser_state {
            ParserState::Ground => match b {
                0x1b => {
                    self.parser_state = ParserState::Escape;
                }
                0xC2..=0xDF => {
                    self.utf8_buf.clear();
                    self.utf8_buf.push(b);
                    self.utf8_needed = 2;
                }
                0xE0..=0xEF => {
                    self.utf8_buf.clear();
                    self.utf8_buf.push(b);
                    self.utf8_needed = 3;
                }
                0xF0..=0xF4 => {
                    self.utf8_buf.clear();
                    self.utf8_buf.push(b);
                    self.utf8_needed = 4;
                }
                0x80..=0xC1 | 0xF5..=0xFF => {
                    self.put_char('\u{FFFD}');
                }
                b'\r' => {
                    self.pending_wrap = false;
                    self.cursor_col = 0;
                }
                b'\n' => {
                    self.pending_wrap = false;
                    self.line_feed();
                }
                0x08 => {
                    self.pending_wrap = false;
                    let old_col = self.cursor_col;
                    self.cursor_col = self.cursor_col.saturating_sub(1);
                    let r = self.cursor_row as usize;
                    let c = self.cursor_col as usize;
                    let grid = self.grid_mut();
                    if r < grid.len() {
                        if (old_col as usize) < grid[r].len() && grid[r][old_col as usize].ch == ' ' {
                            grid[r][old_col as usize] = Cell::default();
                        }
                        if c < grid[r].len() && grid[r][c].ch == ' ' {
                            grid[r][c] = Cell::default();
                        }
                    }
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
                    self.csi_has_intermediate = false;
                }
                b']' => {
                    self.parser_state = ParserState::Osc;
                    self.osc_buf.clear();
                }
                b'7' => {
                    self.saved_cursor = (self.cursor_row, self.cursor_col);
                    self.parser_state = ParserState::Ground;
                }
                b'8' => {
                    self.pending_wrap = false;
                    self.cursor_row = min(self.saved_cursor.0, self.rows.saturating_sub(1));
                    self.cursor_col = min(self.saved_cursor.1, self.cols.saturating_sub(1));
                    self.parser_state = ParserState::Ground;
                }
                b'\\' => {
                    // String Terminator (ST)
                    if !self.osc_buf.is_empty() {
                        self.execute_osc();
                    }
                    self.parser_state = ParserState::Ground;
                }
                b'M' => {
                    // Reverse index (scroll down if at top)
                    self.pending_wrap = false;
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
                b';' | b':' => {
                    self.csi_params.push(self.csi_current_param.unwrap_or(0));
                    self.csi_current_param = None;
                }
                // Private parameter markers (`?`, `>`, `<`, `=`), e.g. `\x1b[?25h`, `\x1b[>4m`.
                b'?' | b'>' | b'<' | b'=' => {
                    self.csi_is_private = true;
                }
                // Intermediate bytes, e.g. `$` in `\x1b[?2026$p` or space in `\x1b[0 q`.
                0x20..=0x2f => {
                    self.csi_has_intermediate = true;
                }
                0x40..=0x7e => {
                    // Final character of CSI sequence
                    if let Some(param) = self.csi_current_param {
                        self.csi_params.push(param);
                    }
                    if !self.csi_has_intermediate {
                        self.execute_csi(b);
                    }
                    self.parser_state = ParserState::Ground;
                }
                // Anything else aborts the sequence (C0 controls are executed as usual).
                _ => {
                    self.parser_state = ParserState::Ground;
                    if b < 0x20 {
                        self.process_byte(b);
                    }
                }
            },
            ParserState::Osc => {
                // String terminator: BEL (0x07) or ST (\x1b\)
                if b == 0x07 {
                    self.execute_osc();
                    self.parser_state = ParserState::Ground;
                } else if b == 0x1b {
                    self.parser_state = ParserState::Escape;
                } else {
                    if self.osc_buf.len() < 8192 {
                        self.osc_buf.push(b);
                    }
                }
            }
        }
    }

    fn execute_csi(&mut self, cmd: u8) {
        if cmd != b'm' {
            self.pending_wrap = false;
        }
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
            b'X' => {
                // Erase Character(s): blank n cells from the cursor, cursor does not move
                let n = self.csi_params.first().copied().unwrap_or(1).max(1) as usize;
                let r = self.cursor_row as usize;
                let cc = self.cursor_col as usize;
                let cols = self.cols as usize;
                if r < self.rows as usize {
                    let grid = self.grid_mut();
                    for cell in grid[r][cc..min(cc + n, cols)].iter_mut() {
                        *cell = Cell::default();
                    }
                }
            }
            b'P' => {
                // Delete Character(s): shift the rest of the line left
                let n = self.csi_params.first().copied().unwrap_or(1).max(1) as usize;
                let r = self.cursor_row as usize;
                let cc = self.cursor_col as usize;
                let cols = self.cols as usize;
                if r < self.rows as usize && cc < cols {
                    let grid = self.grid_mut();
                    let n = min(n, cols - cc);
                    grid[r].drain(cc..cc + n);
                    grid[r].resize(cols, Cell::default());
                }
            }
            b'@' => {
                // Insert blank Character(s): shift the rest of the line right
                let n = self.csi_params.first().copied().unwrap_or(1).max(1) as usize;
                let r = self.cursor_row as usize;
                let cc = self.cursor_col as usize;
                let cols = self.cols as usize;
                if r < self.rows as usize && cc < cols {
                    let grid = self.grid_mut();
                    let n = min(n, cols - cc);
                    for _ in 0..n {
                        grid[r].insert(cc, Cell::default());
                    }
                    grid[r].truncate(cols);
                }
            }
            b'L' => {
                // Insert Line(s) at cursor, pushing lines below down
                let n = self.csi_params.first().copied().unwrap_or(1).max(1) as usize;
                let r = self.cursor_row as usize;
                let rows = self.rows as usize;
                let cols = self.cols as usize;
                let grid = self.grid_mut();
                for _ in 0..min(n, rows.saturating_sub(r)) {
                    grid.pop();
                    grid.insert(r, vec![Cell::default(); cols]);
                }
            }
            b'M' => {
                // Delete Line(s) at cursor, pulling lines below up
                let n = self.csi_params.first().copied().unwrap_or(1).max(1) as usize;
                let r = self.cursor_row as usize;
                let rows = self.rows as usize;
                let cols = self.cols as usize;
                let grid = self.grid_mut();
                for _ in 0..min(n, rows.saturating_sub(r)) {
                    if r < grid.len() {
                        grid.remove(r);
                        grid.push(vec![Cell::default(); cols]);
                    }
                }
            }
            b'm' => {
                // SGR (Select Graphic Rendition)
                self.execute_sgr();
            }
            b'h' if self.csi_is_private => {
                // Private mode set
                match self.csi_params.first().copied() {
                    Some(1049) | Some(47) => {
                        self.saved_cursor = (self.cursor_row, self.cursor_col);
                        self.in_alt_screen = true;
                        for row in &mut self.alt_grid {
                            for cell in row {
                                *cell = Cell::default();
                            }
                        }
                    }
                    Some(25) => {
                        self.cursor_visible = true;
                    }
                    Some(2004) => {
                        self.bracketed_paste = true;
                    }
                    _ => {}
                }
            }
            b'l' if self.csi_is_private => {
                // Private mode reset
                match self.csi_params.first().copied() {
                    Some(1049) | Some(47) => {
                        self.in_alt_screen = false;
                        self.cursor_row = min(self.saved_cursor.0, self.rows.saturating_sub(1));
                        self.cursor_col = min(self.saved_cursor.1, self.cols.saturating_sub(1));
                    }
                    Some(25) => {
                        self.cursor_visible = false;
                    }
                    Some(2004) => {
                        self.bracketed_paste = false;
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
                4 => {
                    self.current_attrs.underline = true;
                    if i + 1 < self.csi_params.len() {
                        match self.csi_params[i + 1] {
                            0 => {
                                self.current_attrs.underline = false;
                                self.current_attrs.underline_style = UnderlineStyle::None;
                                i += 1;
                            }
                            1 => {
                                self.current_attrs.underline_style = UnderlineStyle::Straight;
                                i += 1;
                            }
                            2 => {
                                self.current_attrs.underline_style = UnderlineStyle::Double;
                                i += 1;
                            }
                            3 => {
                                self.current_attrs.underline_style = UnderlineStyle::Curly;
                                i += 1;
                            }
                            4 => {
                                self.current_attrs.underline_style = UnderlineStyle::Dotted;
                                i += 1;
                            }
                            5 => {
                                self.current_attrs.underline_style = UnderlineStyle::Dashed;
                                i += 1;
                            }
                            _ => {
                                self.current_attrs.underline_style = UnderlineStyle::Straight;
                            }
                        }
                    } else {
                        self.current_attrs.underline_style = UnderlineStyle::Straight;
                    }
                }
                7 => self.current_attrs.reverse = true,
                9 => self.current_attrs.strikethrough = true,
                22 => {
                    self.current_attrs.bold = false;
                    self.current_attrs.dim = false;
                }
                23 => self.current_attrs.italic = false,
                24 => {
                    self.current_attrs.underline = false;
                    self.current_attrs.underline_style = UnderlineStyle::None;
                }
                27 => self.current_attrs.reverse = false,
                29 => self.current_attrs.strikethrough = false,
                30..=37 => {
                    self.current_attrs.fg = Some((self.csi_params[i] - 30) as u8);
                    self.current_attrs.fg_rgb = None;
                }
                39 => {
                    self.current_attrs.fg = None;
                    self.current_attrs.fg_rgb = None;
                }
                40..=47 => {
                    self.current_attrs.bg = Some((self.csi_params[i] - 40) as u8);
                    self.current_attrs.bg_rgb = None;
                }
                49 => {
                    self.current_attrs.bg = None;
                    self.current_attrs.bg_rgb = None;
                }
                53 => self.current_attrs.overline = true,
                55 => self.current_attrs.overline = false,
                58 => {
                    // Underline color: 58;2;R;G;B or 58;5;idx
                    if i + 4 < self.csi_params.len() && self.csi_params[i + 1] == 2 {
                        let r = self.csi_params[i + 2] as u8;
                        let g = self.csi_params[i + 3] as u8;
                        let b = self.csi_params[i + 4] as u8;
                        self.current_attrs.underline_color = Some((r, g, b));
                        i += 4;
                    } else if i + 2 < self.csi_params.len() && self.csi_params[i + 1] == 5 {
                        let idx = self.csi_params[i + 2] as u8;
                        self.current_attrs.underline_color = Some((idx, idx, idx));
                        i += 2;
                    }
                }
                59 => {
                    self.current_attrs.underline_color = None;
                }
                90..=97 => {
                    self.current_attrs.fg = Some((self.csi_params[i] - 90 + 8) as u8);
                    self.current_attrs.fg_rgb = None;
                }
                100..=107 => {
                    self.current_attrs.bg = Some((self.csi_params[i] - 100 + 8) as u8);
                    self.current_attrs.bg_rgb = None;
                }
                38 => {
                    if i + 2 < self.csi_params.len() && self.csi_params[i + 1] == 5 {
                        self.current_attrs.fg = Some(self.csi_params[i + 2] as u8);
                        self.current_attrs.fg_rgb = None;
                        i += 2;
                    } else if i + 4 < self.csi_params.len() && self.csi_params[i + 1] == 2 {
                        let r = self.csi_params[i + 2] as u8;
                        let g = self.csi_params[i + 3] as u8;
                        let b = self.csi_params[i + 4] as u8;
                        self.current_attrs.fg = None;
                        self.current_attrs.fg_rgb = Some((r, g, b));
                        i += 4;
                    }
                }
                48 => {
                    if i + 2 < self.csi_params.len() && self.csi_params[i + 1] == 5 {
                        self.current_attrs.bg = Some(self.csi_params[i + 2] as u8);
                        self.current_attrs.bg_rgb = None;
                        i += 2;
                    } else if i + 4 < self.csi_params.len() && self.csi_params[i + 1] == 2 {
                        let r = self.csi_params[i + 2] as u8;
                        let g = self.csi_params[i + 3] as u8;
                        let b = self.csi_params[i + 4] as u8;
                        self.current_attrs.bg = None;
                        self.current_attrs.bg_rgb = Some((r, g, b));
                        i += 4;
                    }
                }
                _ => {}
            }
            i += 1;
        }
    }

    /// Process complete OSC escape sequence accumulated in `self.osc_buf`.
    fn execute_osc(&mut self) {
        if self.osc_buf.is_empty() {
            return;
        }
        let osc_str = String::from_utf8_lossy(&self.osc_buf);
        let mut parts = osc_str.splitn(2, ';');
        let code = parts.next().unwrap_or("").trim();
        let payload = parts.next().unwrap_or("");

        match code {
            "8" => {
                // OSC 8 Hyperlinks: \x1b]8;params;url\x1b\
                // payload is "params;url" or ";url" or ";;" to close
                let mut url_parts = payload.splitn(2, ';');
                let _params = url_parts.next();
                let url = url_parts.next().unwrap_or("");
                if url.is_empty() {
                    self.active_osc_url = None;
                    self.current_attrs.hyperlink = None;
                } else {
                    let u = url.to_string();
                    self.active_osc_url = Some(u.clone());
                    self.current_attrs.hyperlink = Some(u);
                }
            }
            "52" => {
                // OSC 52 System Clipboard: \x1b]52;c;<base64>\x1b\
                let mut cb_parts = payload.splitn(2, ';');
                let _target = cb_parts.next();
                let b64 = cb_parts.next().unwrap_or("");
                if !b64.is_empty() && b64 != "?" {
                    self.osc52_clipboard = Some(b64.to_string());
                }
            }
            "133" => {
                // OSC 133 Shell Marks: \x1b]133;A\x1b\ (Prompt start), B (Command start), C (Output start), D (Command done)
                let mark = payload.chars().next().unwrap_or(' ');
                if mark == 'A' {
                    self.last_osc133_prompt_row = Some(self.cursor_row);
                }
            }
            _ => {}
        }
        self.osc_buf.clear();
    }

    fn put_char(&mut self, ch: char) {
        if self.pending_wrap {
            self.pending_wrap = false;
            self.cursor_col = 0;
            self.line_feed();
        }

        let r = self.cursor_row as usize;
        let c = self.cursor_col as usize;
        let rows = self.rows as usize;
        let cols = self.cols as usize;

        if r < rows && c < cols {
            self.grid_mut()[r][c] = Cell {
                ch,
                attrs: self.current_attrs.clone(),
            };
        }

        if self.cursor_col + 1 >= self.cols {
            self.pending_wrap = true;
        } else {
            self.cursor_col += 1;
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
        if !self.in_alt_screen {
            let row_str = self.row_text(0);
            let trimmed = row_str.trim();
            if !trimmed.is_empty() || (!self.scrollback.is_empty() && !self.scrollback.last().map(|s| s.is_empty()).unwrap_or(false)) {
                self.scrollback.push(row_str.trim_end().to_string());
                if self.scrollback.len() > 2000 {
                    self.scrollback.remove(0);
                }
            }
        }
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

        if mode == 3 && !self.in_alt_screen {
            self.scrollback.clear();
        }

        let grid = self.grid_mut();

        match mode {
            0 => {
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
                for c in cc..cols {
                    grid[r][c] = Cell::default();
                }
            }
            1 => {
                for c in 0..=cc {
                    if c < cols {
                        grid[r][c] = Cell::default();
                    }
                }
            }
            2 => {
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

    /// Return reference to a specific cell in the active grid.
    pub fn cell(&self, row: u16, col: u16) -> Option<&Cell> {
        let grid = self.grid();
        grid.get(row as usize).and_then(|r| r.get(col as usize))
    }

    /// Return slice of cells for a specific row in the active grid.
    pub fn row_cells(&self, row: u16) -> Option<&[Cell]> {
        let grid = self.grid();
        grid.get(row as usize).map(|r| r.as_slice())
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
            line_fg: self
                .grid()
                .iter()
                .map(|row| row.iter().find(|c| c.ch != ' ').and_then(|c| c.attrs.fg))
                .collect(),
            scrollback_lines: self.scrollback.clone(),
        }
    }

    /// Return lines that have scrolled off the top into the scrollback history buffer.
    pub fn scrollback(&self) -> &[String] {
        &self.scrollback
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

        screen.process_bytes(b"\x1b[?1049h\x1b[H\x1b[2JAlt buffer text");
        assert!(screen.in_alt_screen);
        assert_eq!(screen.row_text(0), "Alt buffer text");

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

    #[test]
    fn test_screen_resize() {
        let mut screen = Screen::new(80, 24);
        screen.process_bytes(b"Hello World!");
        assert_eq!(screen.row_text(0), "Hello World!");

        screen.resize(100, 30);
        assert_eq!(screen.cols, 100);
        assert_eq!(screen.rows, 30);
        assert_eq!(screen.row_text(0), "Hello World!");
    }

    #[test]
    fn test_24bit_truecolor() {
        let mut screen = Screen::new(80, 24);
        screen.process_bytes(b"\x1b[38;2;123;45;67m\x1b[48;2;89;101;112mTrueColor");
        let cell = screen.cell(0, 0).unwrap();
        assert_eq!(cell.attrs.fg_rgb, Some((123, 45, 67)));
        assert_eq!(cell.attrs.bg_rgb, Some((89, 101, 112)));
    }

    #[test]
    fn test_utf8_chunk_boundary_decoding() {
        let mut screen = Screen::new(80, 24);
        let bytes = "🚀".as_bytes();
        assert_eq!(bytes.len(), 4);
        screen.process_bytes(&bytes[0..2]);
        // After first chunk, no characters emitted yet
        assert_eq!(screen.cursor_col, 0);
        screen.process_bytes(&bytes[2..4]);
        // Successfully decoded without replacement char
        assert_eq!(screen.cell(0, 0).unwrap().ch, '🚀');
    }

    #[test]
    fn test_backspace_erase_and_trailing_space_hygiene() {
        let mut screen = Screen::new(80, 24);
        screen.process_bytes(b"abc");
        assert_eq!(screen.row_text(0), "abc");
        // Erase 'c' using BS, space, BS
        screen.process_bytes(b"\x08 \x08");
        assert_eq!(screen.row_text(0), "ab");
        assert_eq!(screen.cursor_col, 2);
    }

    #[test]
    fn test_bracketed_paste_mode() {
        let mut screen = Screen::new(80, 24);
        assert!(!screen.bracketed_paste);
        screen.process_bytes(b"\x1b[?2004h");
        assert!(screen.bracketed_paste);
        screen.process_bytes(b"\x1b[?2004l");
        assert!(!screen.bracketed_paste);
    }

    #[test]
    fn test_undercurl_and_underline_styles() {
        let mut screen = Screen::new(80, 24);
        // Test curly underline CSI 4:3m
        screen.process_bytes(b"\x1b[4:3mCurly");
        let cell = screen.cell(0, 0).unwrap();
        assert!(cell.attrs.underline);
        assert_eq!(cell.attrs.underline_style, UnderlineStyle::Curly);

        // Test double underline CSI 4:2m
        screen.process_bytes(b"\x1b[4:2mDouble");
        let cell = screen.cell(0, 5).unwrap();
        assert_eq!(cell.attrs.underline_style, UnderlineStyle::Double);

        // Test underline off CSI 24m
        screen.process_bytes(b"\x1b[24mPlain");
        let cell = screen.cell(0, 11).unwrap();
        assert!(!cell.attrs.underline);
        assert_eq!(cell.attrs.underline_style, UnderlineStyle::None);
    }

    #[test]
    fn test_underline_color() {
        let mut screen = Screen::new(80, 24);
        screen.process_bytes(b"\x1b[4:3m\x1b[58;2;255;0;128mRedCurly");
        let cell = screen.cell(0, 0).unwrap();
        assert!(cell.attrs.underline);
        assert_eq!(cell.attrs.underline_style, UnderlineStyle::Curly);
        assert_eq!(cell.attrs.underline_color, Some((255, 0, 128)));

        // Reset underline color CSI 59m
        screen.process_bytes(b"\x1b[59mDefaultUnderline");
        let cell = screen.cell(0, 8).unwrap();
        assert_eq!(cell.attrs.underline_color, None);
    }

    #[test]
    fn test_strikethrough_and_overline() {
        let mut screen = Screen::new(80, 24);
        // Strikethrough CSI 9m and Overline CSI 53m
        screen.process_bytes(b"\x1b[9m\x1b[53mDecorated");
        let cell = screen.cell(0, 0).unwrap();
        assert!(cell.attrs.strikethrough);
        assert!(cell.attrs.overline);

        // Turn off strikethrough CSI 29m and overline CSI 55m
        screen.process_bytes(b"\x1b[29m\x1b[55mPlain");
        let cell = screen.cell(0, 9).unwrap();
        assert!(!cell.attrs.strikethrough);
        assert!(!cell.attrs.overline);
    }

    #[test]
    fn test_osc8_hyperlinks() {
        let mut screen = Screen::new(80, 24);
        // OSC 8 hyperlink: \x1b]8;;https://example.com\x1b\ClickHere\x1b]8;;\x1b\
        screen.process_bytes(b"\x1b]8;;https://example.com\x1b\\ClickHere\x1b]8;;\x1b\\");
        let cell = screen.cell(0, 0).unwrap();
        assert_eq!(cell.attrs.hyperlink, Some("https://example.com".to_string()));

        // Next character after closing hyperlink has None
        let cell_after = screen.cell(0, 9);
        assert_eq!(cell_after.map(|c| c.attrs.hyperlink.clone()), Some(None));
    }

    #[test]
    fn test_osc52_clipboard() {
        let mut screen = Screen::new(80, 24);
        assert_eq!(screen.osc52_clipboard, None);
        // OSC 52: \x1b]52;c;SGVsbG8=\x07
        screen.process_bytes(b"\x1b]52;c;SGVsbG8=\x07");
        assert_eq!(screen.osc52_clipboard, Some("SGVsbG8=".to_string()));
    }

    #[test]
    fn test_osc133_shell_marks() {
        let mut screen = Screen::new(80, 24);
        assert_eq!(screen.last_osc133_prompt_row, None);
        // OSC 133;A: Prompt start mark
        screen.process_bytes(b"\x1b]133;A\x1b\\");
        assert_eq!(screen.last_osc133_prompt_row, Some(0));
    }
}
