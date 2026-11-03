//! Pure half of `typing`: turn text into keystrokes (tested without Windows).

/// One key press-and-release.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stroke {
    /// A UTF-16 code unit sent as a Unicode keystroke (surrogate pairs are two units, which
    /// is what `KEYEVENTF_UNICODE` expects).
    Unit(u16),
    Enter,
    Tab,
}

/// Keystrokes for `text`: `\r\n`, `\r` and `\n` become one Enter, `\t` Tab, other control
/// characters are dropped, everything else is sent as UTF-16 units.
pub fn plan(text: &str) -> Vec<Stroke> {
    let mut out = Vec::with_capacity(text.len());
    let mut chars = text.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\r' => {
                if chars.peek() == Some(&'\n') {
                    chars.next();
                }
                out.push(Stroke::Enter);
            }
            '\n' => out.push(Stroke::Enter),
            '\t' => out.push(Stroke::Tab),
            c if c.is_control() => {}
            c => {
                let mut buf = [0u16; 2];
                for u in c.encode_utf16(&mut buf) {
                    out.push(Stroke::Unit(*u));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plain_text_is_one_unit_per_char() {
        assert_eq!(
            plan("Hi!"),
            vec![
                Stroke::Unit(b'H' as u16),
                Stroke::Unit(b'i' as u16),
                Stroke::Unit(b'!' as u16)
            ]
        );
    }

    #[test]
    fn newlines_and_tabs() {
        let enters = plan("a\r\nb\nc\rd\te")
            .iter()
            .filter(|s| **s == Stroke::Enter)
            .count();
        assert_eq!(enters, 3);
        assert!(plan("a\tb").contains(&Stroke::Tab));
    }

    #[test]
    fn control_chars_are_dropped() {
        assert_eq!(plan("a\u{7}\u{0}b").len(), 2);
    }

    #[test]
    fn devanagari_and_emoji_survive() {
        let text = "नमस्ते 👋";
        let p = plan(text);
        let units: Vec<u16> = text.encode_utf16().collect();
        assert_eq!(p.len(), units.len());
        assert!(p.iter().zip(units).all(|(s, u)| *s == Stroke::Unit(u)));
    }
}
