//! Plain text from terminal output, for session logs: escape sequences
//! (colours, cursor moves, titles, hyperlinks…) and control characters are
//! removed (C1 controls too, written as UTF-8), line ends become `\n`. The
//! state carries over between chunks, so a sequence split across two reads is
//! still removed whole.

/// Longest string sequence (OSC, DCS…) kept track of; a longer one is cut
/// short so an unterminated sequence cannot swallow the rest of the log.
const MAX_STRING: usize = 64 * 1024;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
enum State {
    #[default]
    Ground,
    /// After ESC.
    Escape,
    /// ESC followed by intermediate bytes (e.g. `ESC ( B`).
    EscapeIntermediate,
    /// CSI: `ESC [` … final byte.
    Csi,
    /// OSC, DCS, SOS, PM or APC: until BEL (OSC only) or `ESC \`.
    String,
    /// ESC seen inside a string sequence.
    StringEscape,
}

#[derive(Debug, Default)]
pub struct Stripper {
    state: State,
    /// A `\r` waiting to see whether `\n` follows.
    cr: bool,
    /// A 0xC2 byte waiting to see whether it starts a C1 control (U+0080..U+009F).
    c2: bool,
    /// A space waiting to see whether a carriage return follows.
    space: bool,
    string_len: usize,
}

impl Stripper {
    pub fn new() -> Self {
        Self::default()
    }

    /// The printable text of `input`, appended to `out`.
    pub fn feed(&mut self, input: &[u8], out: &mut Vec<u8>) {
        for &b in input {
            match self.state {
                State::Ground => self.ground(b, out),
                State::Escape => {
                    self.state = match b {
                        b'[' => State::Csi,
                        b']' | b'P' | b'X' | b'^' | b'_' => self.string(),
                        0x20..=0x2f => State::EscapeIntermediate,
                        _ => State::Ground,
                    }
                }
                State::EscapeIntermediate => {
                    if !(0x20..=0x2f).contains(&b) {
                        self.state = State::Ground;
                    }
                }
                State::Csi => {
                    if !(0x20..=0x3f).contains(&b) {
                        // The final byte, or a stray control that ends it.
                        self.state = State::Ground;
                    }
                }
                State::String => {
                    self.string_len += 1;
                    if b == 0x07 || self.string_len > MAX_STRING {
                        self.state = State::Ground;
                    } else if b == 0x1b {
                        self.state = State::StringEscape;
                    }
                }
                State::StringEscape => {
                    self.state = if b == b'\\' { State::Ground } else { State::String };
                }
            }
        }
    }

    /// The output stops here (a note of the log's own follows, or the end):
    /// a held space is written, a sequence cut short is dropped.
    pub fn break_off(&mut self, out: &mut Vec<u8>) {
        if self.c2 {
            out.push(0xc2);
        }
        self.flush_space(out);
        *self = Self::default();
    }

    fn string(&mut self) -> State {
        self.string_len = 0;
        State::String
    }

    fn ground(&mut self, b: u8, out: &mut Vec<u8>) {
        if self.c2 {
            self.c2 = false;
            if (0x80..=0x9f).contains(&b) {
                // CSI, and DCS, SOS, OSC, PM, APC, work like their ESC forms.
                match b {
                    0x9b => self.state = State::Csi,
                    0x90 | 0x98 | 0x9d | 0x9e | 0x9f => self.state = self.string(),
                    _ => {}
                }
                return;
            }
            out.push(0xc2);
        }
        // A carriage return on its own (progress bars rewriting a line) is
        // dropped; CR LF is one line end.
        if self.cr && b != b'\n' {
            self.cr = false;
        }
        // A space right before a carriage return goes too: shells write
        // " \r" where a long command line wraps on screen, and the next
        // character overwrites that space.
        match b {
            0x1b => {
                self.flush_space(out);
                self.state = State::Escape;
            }
            0xc2 => {
                self.flush_space(out);
                self.c2 = true;
            }
            b' ' => {
                self.flush_space(out);
                self.space = true;
            }
            b'\r' => {
                self.space = false;
                self.cr = true;
            }
            b'\n' => {
                self.flush_space(out);
                self.cr = false;
                out.push(b'\n');
            }
            // Other controls are dropped.
            0x00..=0x08 | 0x0b..=0x1f | 0x7f => {}
            _ => {
                self.flush_space(out);
                out.push(b);
            }
        }
    }

    fn flush_space(&mut self, out: &mut Vec<u8>) {
        if self.space {
            self.space = false;
            out.push(b' ');
        }
    }
}

/// `secs` since the Unix epoch as "YYYY-MM-DD HH:MM:SS UTC".
pub fn utc_stamp(secs: u64) -> String {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Days to civil date (Howard Hinnant's algorithm).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
        rem / 3600,
        rem % 3600 / 60,
        rem % 60
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn strip(chunks: &[&[u8]]) -> String {
        let mut s = Stripper::new();
        let mut out = Vec::new();
        for c in chunks {
            s.feed(c, &mut out);
        }
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn colours_titles_and_links_go_away() {
        assert_eq!(strip(&[b"\x1b[1;32mok\x1b[0m done\r\n"]), "ok done\n");
        assert_eq!(strip(&[b"\x1b]0;user@host: ~\x07$ ls\r\n"]), "$ ls\n");
        assert_eq!(strip(&[b"\x1b]8;;https://x\x1b\\link\x1b]8;;\x1b\\\n"]), "link\n");
        assert_eq!(strip(&[b"\x1b(B\x1b=text\x1bP1$r\x1b\\\n"]), "text\n");
        assert_eq!(strip(&[b"bell\x07 back\x08\x7f\n"]), "bell back\n");
    }

    #[test]
    fn c1_controls_go_away() {
        assert_eq!(strip(&["a\u{9b}31mb\u{85}c ©\n".as_bytes()]), "abc ©\n");
        assert_eq!(strip(&[b"x\xc2", b"\x9d0;title\x07y", b"\xc2"]), "xy");
    }

    #[test]
    fn sequences_split_across_chunks() {
        assert_eq!(strip(&[b"a\x1b", b"[3", b"1mb\r", b"\nc"]), "ab\nc");
        assert_eq!(strip(&[b"\x1b]0;ti", b"tle\x1b", b"\\x"]), "x");
    }

    #[test]
    fn utf8_and_progress_lines() {
        assert_eq!(strip(&["città ✓\n".as_bytes()]), "città ✓\n");
        // A rewritten progress line keeps its text, without the bare CRs.
        assert_eq!(strip(&[b"10%\r50%\r100%\r\n"]), "10%50%100%\n");
    }

    #[test]
    fn wrapped_command_lines_get_no_extra_space() {
        // What bash prints when a typed line wraps: " \r" at the margin.
        assert_eq!(strip(&[b"$ echo abc", b"de \r", b"fgh\r\n"]), "$ echo abcdefgh\n");
        // Spaces elsewhere stay, also across chunks and around sequences.
        assert_eq!(strip(&[b"a  b ", b" c\x1b[0m d\t e\n"]), "a  b  c d\t e\n");
        assert_eq!(strip(&["x \u{e0}\n".as_bytes()]), "x \u{e0}\n");
    }

    #[test]
    fn break_off_writes_what_is_held_and_forgets_cut_sequences() {
        let mut s = Stripper::new();
        let mut out = Vec::new();
        s.feed(b"$ \x1b]0;tit", &mut out);
        s.break_off(&mut out);
        s.feed(b"le\n", &mut out);
        assert_eq!(String::from_utf8(out).unwrap(), "$ le\n");
    }

    #[test]
    fn an_endless_string_sequence_is_cut_short() {
        let mut input = b"\x1b]".to_vec();
        input.extend(std::iter::repeat_n(b'a', MAX_STRING + 10));
        input.extend(b"tail");
        // What follows the cut is kept (minus the overflow).
        assert!(strip(&[&input]).ends_with("tail"));
    }

    #[test]
    fn stamps() {
        assert_eq!(utc_stamp(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(utc_stamp(1_792_355_100), "2026-10-18 20:25:00 UTC");
        assert_eq!(utc_stamp(951_782_400), "2000-02-29 00:00:00 UTC");
    }
}
