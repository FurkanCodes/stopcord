//! Answers to the questions programs ask their terminal.
//!
//! Agents built on terminal UI libraries ask for the cursor position, the
//! terminal's capabilities and its colours when they start, and some wait for
//! the answer. Nobody may be watching, so Stopcord answers for the terminal.

/// The colours reported back: Stopcord's `term` ground and `ink`.
const BACKGROUND: &str = "rgb:1616/1818/1c1c";
const FOREGROUND: &str = "rgb:e8e8/eaea/eeee";

pub(crate) fn csi(
    screen: &vt100::Screen,
    i1: Option<u8>,
    params: &[&[u16]],
    c: char,
) -> Option<Vec<u8>> {
    let first = params.first().and_then(|p| p.first()).copied().unwrap_or(0);
    match (i1, c, first) {
        // Device status: "are you OK?"
        (None, 'n', 5) => Some(b"\x1b[0n".to_vec()),
        // Cursor position, 1-based.
        (None, 'n', 6) => {
            let (row, col) = screen.cursor_position();
            Some(format!("\x1b[{};{}R", row + 1, col + 1).into_bytes())
        }
        // Primary device attributes: a VT220 with ANSI colour.
        (None, 'c', 0) => Some(b"\x1b[?62;22c".to_vec()),
        // Secondary device attributes.
        (Some(b'>'), 'c', 0) => Some(b"\x1b[>1;10;0c".to_vec()),
        // Keyboard-protocol query: say nothing, which means "not supported".
        _ => None,
    }
}

pub(crate) fn osc(params: &[&[u8]]) -> Option<Vec<u8>> {
    match params {
        [b"10", b"?"] => Some(format!("\x1b]10;{FOREGROUND}\x1b\\").into_bytes()),
        [b"11", b"?"] => Some(format!("\x1b]11;{BACKGROUND}\x1b\\").into_bytes()),
        _ => None,
    }
}
