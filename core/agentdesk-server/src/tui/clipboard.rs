//! System clipboard integration supporting OSC 52 (universal terminal escape),
//! wl-copy (Wayland), and xclip (X11).

use std::io::{self, Write};

const B64_CHARS: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Fast, dependency-free Base64 encoding for OSC 52 terminal payloads.
pub fn base64_encode(data: &[u8]) -> String {
    let mut out = String::with_capacity((data.len() + 2) / 3 * 4);
    for chunk in data.chunks(3) {
        let b0 = chunk[0];
        let b1 = if chunk.len() > 1 { chunk[1] } else { 0 };
        let b2 = if chunk.len() > 2 { chunk[2] } else { 0 };

        out.push(B64_CHARS[(b0 >> 2) as usize] as char);
        out.push(B64_CHARS[(((b0 & 0x03) << 4) | (b1 >> 4)) as usize] as char);
        if chunk.len() > 1 {
            out.push(B64_CHARS[(((b1 & 0x0f) << 2) | (b2 >> 6)) as usize] as char);
        } else {
            out.push('=');
        }
        if chunk.len() > 2 {
            out.push(B64_CHARS[(b2 & 0x3f) as usize] as char);
        } else {
            out.push('=');
        }
    }
    out
}

/// Fast, dependency-free Base64 decoding for OSC 52 terminal payloads.
pub fn base64_decode(input: &str) -> Option<Vec<u8>> {
    let mut clean = Vec::new();
    for b in input.bytes() {
        if b.is_ascii_whitespace() {
            continue;
        }
        clean.push(b);
    }
    if clean.is_empty() {
        return Some(Vec::new());
    }

    let decode_val = |b: u8| -> Option<u8> {
        match b {
            b'A'..=b'Z' => Some(b - b'A'),
            b'a'..=b'z' => Some(b - b'a' + 26),
            b'0'..=b'9' => Some(b - b'0' + 52),
            b'+' => Some(62),
            b'/' => Some(63),
            _ => None,
        }
    };

    let mut out = Vec::with_capacity(clean.len() * 3 / 4);
    for chunk in clean.chunks(4) {
        if chunk.len() < 2 {
            return None;
        }
        let c0 = decode_val(chunk[0])?;
        let c1 = decode_val(chunk[1])?;
        out.push((c0 << 2) | (c1 >> 4));

        if chunk.len() > 2 && chunk[2] != b'=' {
            let c2 = decode_val(chunk[2])?;
            out.push(((c1 & 0x0f) << 4) | (c2 >> 2));

            if chunk.len() > 3 && chunk[3] != b'=' {
                let c3 = decode_val(chunk[3])?;
                out.push(((c2 & 0x03) << 6) | c3);
            }
        }
    }
    Some(out)
}

/// Copy text to the clipboard using OSC 52, wl-copy, and xclip.
pub fn copy_to_clipboard(text: &str) {
    if text.is_empty() {
        return;
    }

    // 1. Universal OSC 52 terminal escape sequence: \x1b]52;c;<base64>\x07
    // Works across foot, alacritty, kitty, wezterm, xterm, and SSH sessions.
    let b64 = base64_encode(text.as_bytes());
    let osc52 = format!("\x1b]52;c;{}\x07", b64);
    let mut stdout = io::stdout();
    let _ = stdout.write_all(osc52.as_bytes());
    let _ = stdout.flush();

    // 2. Native Wayland clipboard (wl-copy)
    if std::env::var_os("WAYLAND_DISPLAY").is_some() {
        if let Ok(mut child) = std::process::Command::new("wl-copy")
            .stdin(std::process::Stdio::piped())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            let _ = child.wait();
        }
    } else if std::env::var_os("DISPLAY").is_some() {
        // 3. Native X11 clipboard (xclip)
        if let Ok(mut child) = std::process::Command::new("xclip")
            .args(["-selection", "clipboard"])
            .stdin(std::process::Stdio::piped())
            .spawn()
        {
            if let Some(mut stdin) = child.stdin.take() {
                let _ = stdin.write_all(text.as_bytes());
            }
            let _ = child.wait();
        }
    }
}

/// Attempt to open a URL in the user's default browser.
pub fn open_url_in_browser(url: &str) {
    if url.is_empty() {
        return;
    }
    let _ = std::process::Command::new("xdg-open")
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base64_encode() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
        assert_eq!(base64_encode(b"Hello World!"), "SGVsbG8gV29ybGQh");
    }

    #[test]
    fn test_base64_decode() {
        assert_eq!(base64_decode(""), Some(b"".to_vec()));
        assert_eq!(base64_decode("Zg=="), Some(b"f".to_vec()));
        assert_eq!(base64_decode("Zm8="), Some(b"fo".to_vec()));
        assert_eq!(base64_decode("Zm9v"), Some(b"foo".to_vec()));
        assert_eq!(base64_decode("SGVsbG8gV29ybGQh"), Some(b"Hello World!".to_vec()));
    }
}
