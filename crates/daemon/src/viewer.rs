//! For anything that shows a daemon terminal in another terminal: removes the
//! queries a program sends to its terminal.
//!
//! The daemon already answers those queries. If a viewer's own terminal saw
//! them too, it would answer again, and its reply would reach the program as typing.

/// Drops terminal queries from a stream of output, keeping everything else.
/// Holds back an escape sequence split across chunks until it's complete.
#[derive(Debug, Default)]
pub struct QueryFilter {
    pending: Vec<u8>,
}

const ESC: u8 = 0x1b;
const MAX_PENDING: usize = 4096;

impl QueryFilter {
    pub fn filter(&mut self, input: &[u8]) -> Vec<u8> {
        let mut data = std::mem::take(&mut self.pending);
        data.extend_from_slice(input);
        let mut out = Vec::with_capacity(data.len());
        let mut i = 0;
        while i < data.len() {
            if data[i] != ESC {
                out.push(data[i]);
                i += 1;
                continue;
            }
            match sequence_len(&data[i..]) {
                Some(len) => {
                    let seq = &data[i..i + len];
                    if !is_query(seq) {
                        out.extend_from_slice(seq);
                    }
                    i += len;
                }
                None if data.len() - i < MAX_PENDING => {
                    self.pending = data[i..].to_vec();
                    break;
                }
                None => {
                    out.extend_from_slice(&data[i..]);
                    break;
                }
            }
        }
        out
    }
}

/// The length of the escape sequence at the start of `s`, or `None` if it isn't complete yet.
fn sequence_len(s: &[u8]) -> Option<usize> {
    match s.get(1)? {
        b'[' => {
            // CSI: parameters 0x30–0x3F, intermediates 0x20–0x2F, one final byte 0x40–0x7E.
            let mut j = 2;
            while j < s.len() && (0x20..=0x3f).contains(&s[j]) {
                j += 1;
            }
            let last = *s.get(j)?;
            Some(if (0x40..=0x7e).contains(&last) {
                j + 1
            } else {
                j
            })
        }
        b']' | b'P' | b'_' | b'^' => {
            // OSC, DCS, APC, PM: end at BEL or ESC \.
            let mut j = 2;
            while j < s.len() {
                if s[j] == 0x07 {
                    return Some(j + 1);
                }
                if s[j] == ESC && s.get(j + 1) == Some(&b'\\') {
                    return Some(j + 2);
                }
                j += 1;
            }
            None
        }
        _ => Some(2),
    }
}

fn is_query(seq: &[u8]) -> bool {
    if seq.get(1) == Some(&b'[') {
        let body = &seq[2..];
        return matches!(
            body,
            b"5n"
                | b"6n"
                | b"?6n"
                | b"c"
                | b"0c"
                | b">c"
                | b">0c"
                | b"=c"
                | b"?u"
                | b">q"
                | b">0q"
                | b"14t"
                | b"16t"
                | b"18t"
        ) || (body.ends_with(b"$p"));
    }
    if seq.get(1) == Some(&b']') {
        let body = &seq[2..];
        return [&b"10;?"[..], b"11;?", b"12;?"]
            .iter()
            .any(|q| body.starts_with(q))
            || (body.starts_with(b"4;") && body.windows(2).any(|w| w == b";?"));
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn drops_queries_and_keeps_drawing() {
        let mut f = QueryFilter::default();
        let out = f.filter(b"hi\x1b[6n\x1b[31mred\x1b]11;?\x07\x1b[c\x1b[0m");
        assert_eq!(out, b"hi\x1b[31mred\x1b[0m");
    }

    #[test]
    fn holds_a_split_sequence() {
        let mut f = QueryFilter::default();
        assert_eq!(f.filter(b"a\x1b["), b"a");
        assert_eq!(f.filter(b"6nb\x1b[2"), b"b");
        assert_eq!(f.filter(b"Jc"), b"\x1b[2Jc");
    }

    #[test]
    fn keeps_titles_and_mode_changes() {
        let mut f = QueryFilter::default();
        let input = b"\x1b]2;work\x07\x1b[?1049h\x1b[?2026$p";
        assert_eq!(f.filter(input), b"\x1b]2;work\x07\x1b[?1049h");
    }
}
