//! The DBC grammar — a hand-rolled, total scanner for the Vector DBC
//! format.
//!
//! Handles the core keyword set: `VERSION`, `NS_`, `BS_`, `BU_`, `BO_`,
//! `SG_` (with `M` / `m<N>` multiplexing), `CM_` (multi-line quoted strings
//! with `""` escapes), `VAL_`, `VAL_TABLE_`, `BA_`, `BA_DEF_`,
//! `BA_DEF_DEF_`, and `SG_MUL_VAL_` (extended multiplexing). Unknown
//! keywords are skipped line-wise rather than rejected — real-world DBC
//! files carry vendor extensions — while malformed known constructs are
//! typed [`DbcError::Syntax`] errors with line numbers.

use crate::error::DbcError;
use crate::model::{
    Attribute, AttributeObject, AttributeValue, ByteOrder, CanId, Dbc, ExtendedMux, Message,
    MultiplexingInfo, Node, Signal, ValueTable,
};
use alloc::format;
use alloc::string::{String, ToString};
use alloc::vec::Vec;

/// Parses a full DBC document.
///
/// # Errors
///
/// [`DbcError::Syntax`] for malformed known constructs (with 1-based line
/// numbers), [`DbcError::InvalidId`] for unrepresentable message ids.
pub fn parse(input: &str) -> Result<Dbc, DbcError> {
    Parser::new(input).run()
}

/// One lexical token. Words stop at whitespace or any punctuation character;
/// quoted strings may span lines and use `""` as an escaped quote.
#[derive(Debug, Clone, PartialEq)]
enum Token<'a> {
    Word(&'a str),
    Quoted(String),
    Punct(char),
}

struct Parser<'a> {
    src: &'a str,
    pos: usize,
}

impl<'a> Parser<'a> {
    fn new(src: &'a str) -> Self {
        Self { src, pos: 0 }
    }

    /// 1-based line of the current position (computed lazily, errors only).
    fn line(&self) -> usize {
        1 + self
            .src
            .as_bytes()
            .iter()
            .take(self.pos)
            .filter(|byte| **byte == b'\n')
            .count()
    }

    fn syntax(&self, message: &str) -> DbcError {
        DbcError::Syntax {
            line: self.line(),
            message: message.to_string(),
        }
    }

    fn skip_trivia(&mut self) {
        while let Some(byte) = self.src.as_bytes().get(self.pos) {
            if byte.is_ascii_whitespace() {
                self.pos += 1;
            } else {
                break;
            }
        }
    }

    fn peek(&self) -> Option<Token<'a>> {
        let rest = self.src.get(self.pos..)?;
        let first = rest.as_bytes().first().copied()?;
        if first == b'"' {
            // Quoted strings are lexed whole on peek (cheap: bounded by the
            // string itself); the scanner re-lexes on next().
            let mut i = 1;
            let bytes = rest.as_bytes();
            while let Some(&byte) = bytes.get(i) {
                match byte {
                    b'"' if bytes.get(i + 1) == Some(&b'"') => i += 2, // "" escape
                    b'"' => {
                        let raw = rest.get(1..i)?;
                        return Some(Token::Quoted(raw.replace("\"\"", "\"")));
                    }
                    _ => i += 1,
                }
            }
            None
        } else if is_punct(first) {
            let ch = char::from(first);
            Some(Token::Punct(ch))
        } else if first.is_ascii_whitespace() {
            None
        } else {
            let end = rest
                .bytes()
                .position(|byte| byte.is_ascii_whitespace() || is_punct(byte) || byte == b'"')
                .unwrap_or(rest.len());
            let word = rest.get(..end)?;
            Some(Token::Word(word))
        }
    }

    fn bump(&mut self, token: &Token<'a>) {
        match token {
            Token::Word(word) => self.pos += word.len(),
            Token::Punct(_) => self.pos += 1,
            // `+ 2` for the enclosing quotes; inner `""` pairs re-collapse
            // to `"` so the raw span is (unescaped length) + 2.
            Token::Quoted(quoted) => {
                self.pos += quoted.len() + 2 + quoted.matches('"').count();
            }
        }
    }

    fn next(&mut self) -> Option<Token<'a>> {
        self.skip_trivia();
        let token = self.peek()?;
        self.bump(&token);
        Some(token)
    }

    /// Skips spaces and tabs — never newlines.
    fn skip_spaces(&mut self) {
        while matches!(self.src.as_bytes().get(self.pos), Some(b' ' | b'\t')) {
            self.pos += 1;
        }
    }

    /// Expects a word on the current line: like [`next`](Self::next), but
    /// refuses to cross a newline (receiver lists, sender names).
    fn next_on_line(&mut self) -> Option<Token<'a>> {
        self.skip_spaces();
        match self.src.as_bytes().get(self.pos) {
            Some(b'\n' | b'\r') | None => None,
            Some(_) => {
                let token = self.peek()?;
                self.bump(&token);
                Some(token)
            }
        }
    }

    fn expect_word(&mut self, what: &str) -> Result<&'a str, DbcError> {
        match self.next() {
            Some(Token::Word(word)) => Ok(word),
            other => Err(self.syntax(&format!("expected {what}, found {other:?}"))),
        }
    }

    fn expect_punct(&mut self, ch: char) -> Result<(), DbcError> {
        match self.next() {
            Some(Token::Punct(found)) if found == ch => Ok(()),
            other => Err(self.syntax(&format!("expected '{ch}', found {other:?}"))),
        }
    }

    fn expect_quoted(&mut self, what: &str) -> Result<String, DbcError> {
        match self.next() {
            Some(Token::Quoted(quoted)) => Ok(quoted),
            other => Err(self.syntax(&format!("expected {what} string, found {other:?}"))),
        }
    }

    fn parse_num<T: core::str::FromStr>(&self, text: &str, what: &str) -> Result<T, DbcError> {
        text.parse::<T>()
            .map_err(|_| self.syntax(&format!("invalid {what}: {text:?}")))
    }

    /// Skips tokens until the statement-terminating `;` (quote-aware: the
    /// scanner never splits a quoted string).
    fn skip_to_semicolon(&mut self) -> Result<(), DbcError> {
        loop {
            match self.next() {
                Some(Token::Punct(';')) => return Ok(()),
                Some(_) => {}
                None => return Err(self.syntax("expected ';' before end of file")),
            }
        }
    }

    /// Skips the remainder of the current line (unknown-keyword tolerance).
    fn skip_rest_of_line(&mut self) {
        if let Some(offset) = self.src.as_bytes().get(self.pos..).map(|rest| {
            rest.iter()
                .position(|byte| *byte == b'\n')
                .unwrap_or(rest.len())
        }) {
            self.pos += offset;
        }
    }

    fn run(mut self) -> Result<Dbc, DbcError> {
        let mut dbc = Dbc::default();
        loop {
            self.skip_trivia();
            let Some(Token::Word(keyword)) = self.peek() else {
                // End of input, or a stray non-word byte.
                if self.pos < self.src.len() {
                    return Err(self.syntax("unexpected character at statement position"));
                }
                break;
            };
            match keyword {
                "VERSION" => {
                    self.bump(&Token::Word(keyword));
                    dbc.version = Some(self.expect_quoted("version")?);
                }
                "NS_" => {
                    self.bump(&Token::Word(keyword));
                    // `NS_ :` opens a block of per-line keyword names that
                    // ends at `BS_`.
                    loop {
                        self.skip_trivia();
                        match self.peek() {
                            Some(Token::Word("BS_")) => break,
                            Some(_) => {
                                self.next();
                            }
                            None => return Err(self.syntax("NS_ block not terminated by BS_")),
                        }
                    }
                }
                "BS_" => {
                    self.bump(&Token::Word(keyword));
                    self.expect_punct(':')?;
                    self.skip_rest_of_line();
                }
                "BU_" => {
                    self.bump(&Token::Word(keyword));
                    self.expect_punct(':')?;
                    while let Some(Token::Word(name)) = self.next_on_line() {
                        dbc.nodes.push(Node {
                            name: name.to_string(),
                        });
                    }
                }
                "BO_" => {
                    self.bump(&Token::Word(keyword));
                    dbc.messages.push(self.parse_message()?);
                }
                "SG_" => {
                    self.bump(&Token::Word(keyword));
                    let signal = self.parse_signal()?;
                    let Some(message) = dbc.messages.last_mut() else {
                        return Err(self.syntax("SG_ before any BO_"));
                    };
                    message.signals.push(signal);
                }
                "CM_" => {
                    self.bump(&Token::Word(keyword));
                    // `CM_ [BO_ id | SG_ id sig | BU_ node] "text" ;`
                    self.skip_to_semicolon()?;
                }
                "BA_DEF_" | "BA_DEF_DEF_" => {
                    self.bump(&Token::Word(keyword));
                    // Attribute schema/default definitions are validated
                    // (terminated) and skipped; the values live in BA_.
                    self.skip_to_semicolon()?;
                }
                "BA_" => {
                    self.bump(&Token::Word(keyword));
                    dbc.attributes.push(self.parse_attribute()?);
                }
                "VAL_" => {
                    self.bump(&Token::Word(keyword));
                    dbc.value_tables.push(self.parse_value_table()?);
                }
                "VAL_TABLE_" => {
                    self.bump(&Token::Word(keyword));
                    dbc.value_tables.push(self.parse_val_table()?);
                }
                "SG_MUL_VAL_" => {
                    self.bump(&Token::Word(keyword));
                    dbc.extended_multiplexing.push(self.parse_sg_mul_val()?);
                }
                _ => {
                    // Unknown keyword: tolerate and skip the line.
                    self.bump(&Token::Word(keyword));
                    self.skip_rest_of_line();
                }
            }
        }
        Ok(dbc)
    }

    /// `BO_ <id> <name>: <dlc> <sender>` followed by ` SG_` lines.
    fn parse_message(&mut self) -> Result<Message, DbcError> {
        let id_word = self.expect_word("message id")?;
        let raw_id = self.parse_num::<u32>(id_word, "message id")?;
        let id = CanId::from_dbc_raw(raw_id)?;
        let name = self.expect_word("message name")?.to_string();
        self.expect_punct(':')?;
        let dlc_word = self.expect_word("message dlc")?;
        let dlc = self.parse_num::<u8>(dlc_word, "message dlc")?;
        if u32::from(dlc) > 64 {
            return Err(self.syntax(&format!("message dlc {dlc} exceeds 64")));
        }
        let sender = match self.next_on_line() {
            Some(Token::Word(sender)) => sender.to_string(),
            Some(other) => return Err(self.syntax(&format!("expected sender, found {other:?}"))),
            None => "Vector__XXX".to_string(),
        };
        Ok(Message {
            id,
            name,
            dlc,
            sender,
            signals: Vec::new(),
        })
    }

    /// `SG_ <name> [M | m<N>] : <start>|<len>@<order><sign> (<scale>,<offset>)
    ///  [<min>|<max>] "<unit>" <recv1>,<recv2>`
    fn parse_signal(&mut self) -> Result<Signal, DbcError> {
        let name = self.expect_word("signal name")?.to_string();
        self.skip_spaces();
        let multiplexing_info = match self.peek() {
            Some(Token::Word(word)) if word == "M" => {
                self.bump(&Token::Word(word));
                MultiplexingInfo::Multiplexor
            }
            Some(Token::Word(word))
                if word.starts_with('m')
                    && matches!(word.as_bytes().get(1), Some(digit) if digit.is_ascii_digit()) =>
            {
                self.bump(&Token::Word(word));
                // `m<N>` or extended-multiplexing `m<N>M` — the value is
                // what matters for extraction; `SG_MUL_VAL_` refines it.
                let digits = word
                    .strip_suffix('M')
                    .unwrap_or(word)
                    .strip_prefix('m')
                    .unwrap_or(word);
                let value = self.parse_num::<i32>(digits, "multiplex value")?;
                MultiplexingInfo::Value(value)
            }
            _ => MultiplexingInfo::None,
        };
        self.expect_punct(':')?;
        let start_word = self.expect_word("start bit")?;
        let start_bit = self.parse_num::<u16>(start_word, "start bit")?;
        if start_bit > 511 {
            // A 64-byte payload numbers bits 0..=511.
            return Err(self.syntax(&format!("start bit {start_bit} exceeds 511")));
        }
        self.expect_punct('|')?;
        let len_word = self.expect_word("signal length")?;
        let bit_length = self.parse_num::<u8>(len_word, "signal length")?;
        if bit_length == 0 || bit_length > 64 {
            return Err(self.syntax(&format!("signal length {bit_length} outside 1..=64")));
        }
        self.expect_punct('@')?;
        let order_sign = self.expect_word("byte order/sign")?;
        let order_bytes = order_sign.as_bytes();
        let byte_order = match order_bytes.first() {
            Some(b'0') => ByteOrder::Motorola,
            Some(b'1') => ByteOrder::Intel,
            _ => return Err(self.syntax(&format!("invalid byte order: {order_sign:?}"))),
        };
        let is_signed = match order_bytes.get(1) {
            Some(b'-') => true,
            Some(b'+') => false,
            _ => return Err(self.syntax(&format!("invalid sign flag: {order_sign:?}"))),
        };
        self.expect_punct('(')?;
        let scale_word = self.expect_word("scale")?;
        let scale = self.parse_num::<f64>(scale_word, "scale")?;
        if scale == 0.0 {
            return Err(self.syntax("scale must be non-zero"));
        }
        self.expect_punct(',')?;
        let offset_word = self.expect_word("offset")?;
        let offset = self.parse_num::<f64>(offset_word, "offset")?;
        self.expect_punct(')')?;
        self.expect_punct('[')?;
        let min_word = self.expect_word("minimum")?;
        let min = self.parse_num::<f64>(min_word, "minimum")?;
        self.expect_punct('|')?;
        let max_word = self.expect_word("maximum")?;
        let max = self.parse_num::<f64>(max_word, "maximum")?;
        self.expect_punct(']')?;
        let unit = self.expect_quoted("unit")?;
        let receivers = self.parse_receivers()?;
        Ok(Signal {
            name,
            start_bit,
            bit_length,
            byte_order,
            is_signed,
            scale,
            offset,
            min,
            max,
            unit,
            receivers,
            multiplexing_info,
        })
    }

    /// Receiver list to end of line: `Node1,Node2` or `Vector__XXX`.
    fn parse_receivers(&mut self) -> Result<Vec<String>, DbcError> {
        let mut receivers = Vec::new();
        let mut expect_name = true;
        loop {
            match self.next_on_line() {
                Some(Token::Word(name)) if expect_name => {
                    receivers.push(name.to_string());
                    expect_name = false;
                }
                Some(Token::Punct(',')) if !expect_name => expect_name = true,
                Some(other) => {
                    return Err(self.syntax(&format!("in receiver list, found {other:?}")));
                }
                None => break,
            }
        }
        if expect_name && !receivers.is_empty() {
            return Err(self.syntax("receiver list ends with ','"));
        }
        Ok(receivers)
    }

    /// `BA_ "name" [BU_ node | BO_ id | SG_ id sig] <value> ;`
    fn parse_attribute(&mut self) -> Result<Attribute, DbcError> {
        let name = self.expect_quoted("attribute name")?;
        self.skip_spaces();
        let object = match self.peek() {
            Some(Token::Word(word)) if word == "BU_" => {
                self.bump(&Token::Word(word));
                AttributeObject::Node(self.expect_word("node name")?.to_string())
            }
            Some(Token::Word(word)) if word == "BO_" => {
                self.bump(&Token::Word(word));
                let id_word = self.expect_word("message id")?;
                let id = self.parse_num::<u32>(id_word, "message id")?;
                AttributeObject::Message(id & 0x1FFF_FFFF)
            }
            Some(Token::Word(word)) if word == "SG_" => {
                self.bump(&Token::Word(word));
                let id_word = self.expect_word("message id")?;
                let id = self.parse_num::<u32>(id_word, "message id")?;
                let signal = self.expect_word("signal name")?.to_string();
                AttributeObject::Signal {
                    message_id: id & 0x1FFF_FFFF,
                    signal,
                }
            }
            _ => AttributeObject::Database,
        };
        let value = match self.next() {
            Some(Token::Quoted(quoted)) => AttributeValue::Str(quoted),
            Some(Token::Word(word)) => {
                if let Ok(int) = word.parse::<i64>() {
                    AttributeValue::Int(int)
                } else {
                    AttributeValue::Float(self.parse_num::<f64>(word, "attribute value")?)
                }
            }
            other => {
                return Err(self.syntax(&format!("expected attribute value, found {other:?}")));
            }
        };
        self.expect_punct(';')?;
        Ok(Attribute {
            name,
            object,
            value,
        })
    }

    /// `VAL_ <id> <signal> (<num> "label")* ;`
    fn parse_value_table(&mut self) -> Result<ValueTable, DbcError> {
        let id_word = self.expect_word("message id")?;
        let id = self.parse_num::<u32>(id_word, "message id")?;
        let signal = self.expect_word("signal name")?.to_string();
        let entries = self.parse_table_entries()?;
        Ok(ValueTable {
            name: None,
            // Stored without the extended-flag bit, matching CanId::raw().
            message_id: Some(id & 0x1FFF_FFFF),
            signal,
            entries,
        })
    }

    /// `VAL_TABLE_ <name> (<num> "label")* ;`
    fn parse_val_table(&mut self) -> Result<ValueTable, DbcError> {
        let name = self.expect_word("table name")?.to_string();
        let entries = self.parse_table_entries()?;
        Ok(ValueTable {
            name: Some(name),
            message_id: None,
            signal: String::new(),
            entries,
        })
    }

    /// `(num "label")*` until the terminating `;`.
    fn parse_table_entries(&mut self) -> Result<Vec<(i64, String)>, DbcError> {
        let mut entries = Vec::new();
        loop {
            self.skip_trivia();
            match self.peek() {
                Some(Token::Punct(';')) => {
                    self.bump(&Token::Punct(';'));
                    return Ok(entries);
                }
                Some(Token::Word(word)) => {
                    self.bump(&Token::Word(word));
                    let raw = self.parse_num::<i64>(word, "table entry value")?;
                    let label = self.expect_quoted("table entry label")?;
                    entries.push((raw, label));
                }
                other => {
                    return Err(
                        self.syntax(&format!("expected table entry or ';', found {other:?}"))
                    );
                }
            }
        }
    }

    /// `SG_MUL_VAL_ <id> <signal> <selector> <a-b, c-d> ;`
    fn parse_sg_mul_val(&mut self) -> Result<ExtendedMux, DbcError> {
        let id_word = self.expect_word("message id")?;
        let message_id = self.parse_num::<u32>(id_word, "message id")?;
        let signal = self.expect_word("signal name")?.to_string();
        let selector = self.expect_word("selector signal")?.to_string();
        let mut ranges = Vec::new();
        loop {
            self.skip_trivia();
            match self.peek() {
                Some(Token::Punct(';')) => {
                    self.bump(&Token::Punct(';'));
                    return Ok(ExtendedMux {
                        // Stored without the extended-flag bit, matching
                        // CanId::raw().
                        message_id: message_id & 0x1FFF_FFFF,
                        signal,
                        selector,
                        ranges,
                    });
                }
                Some(Token::Punct(',')) => {
                    self.bump(&Token::Punct(','));
                }
                Some(Token::Word(word)) => {
                    self.bump(&Token::Word(word));
                    let Some((from, to)) = word.split_once('-') else {
                        return Err(self.syntax(&format!("invalid mux range: {word:?}")));
                    };
                    let from = self.parse_num::<i32>(from, "mux range start")?;
                    let to = self.parse_num::<i32>(to, "mux range end")?;
                    ranges.push((from, to));
                }
                other => {
                    return Err(self.syntax(&format!("expected mux range or ';', found {other:?}")));
                }
            }
        }
    }
}

/// Punctuation that terminates a word in DBC grammar.
fn is_punct(byte: u8) -> bool {
    matches!(
        byte,
        b':' | b'|' | b'@' | b'(' | b')' | b'[' | b']' | b',' | b';'
    )
}

#[cfg(test)]
mod tests {
    // Tests exercise edge values and index fixed-size buffers directly:
    // unwrap/expect/indexing/panic are lifted at test scope by design.
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::indexing_slicing,
        clippy::panic
    )]
    use super::*;

    #[test]
    fn quoted_string_escapes() {
        let mut p = Parser::new(r#""say ""hi"" now""#);
        assert_eq!(p.expect_quoted("x").unwrap(), r#"say "hi" now"#);
    }

    #[test]
    fn unterminated_string_is_none() {
        let mut p = Parser::new("\"no closing quote");
        assert!(p.next().is_none());
    }

    #[test]
    fn words_stop_at_punct() {
        let mut p = Parser::new("7|16@0-");
        assert_eq!(p.next(), Some(Token::Word("7")));
        assert_eq!(p.next(), Some(Token::Punct('|')));
        assert_eq!(p.next(), Some(Token::Word("16")));
        assert_eq!(p.next(), Some(Token::Punct('@')));
        assert_eq!(p.next(), Some(Token::Word("0-")));
    }

    #[test]
    fn next_on_line_respects_newline() {
        let mut p = Parser::new("PCM1,FOO\nBU_:");
        assert_eq!(p.next_on_line(), Some(Token::Word("PCM1")));
        assert_eq!(p.next_on_line(), Some(Token::Punct(',')));
        assert_eq!(p.next_on_line(), Some(Token::Word("FOO")));
        assert_eq!(p.next_on_line(), None);
    }

    #[test]
    fn syntax_errors_carry_line_numbers() {
        let error = parse("BO_ nope Bad: 8 X\n").unwrap_err();
        match error {
            DbcError::Syntax { line, message } => {
                assert_eq!(line, 1);
                assert!(message.contains("message id"), "{message}");
            }
            other => panic!("expected Syntax, got {other:?}"),
        }
    }

    #[test]
    fn signal_constraints_are_checked() {
        // Zero scale:
        let error = parse("BO_ 1 M: 8 S\n SG_ A : 0|8@1+ (0,0) [0|0] \"\" X\n").unwrap_err();
        assert!(matches!(error, DbcError::Syntax { .. }));
        // Zero length:
        let error = parse("BO_ 1 M: 8 S\n SG_ A : 0|0@1+ (1,0) [0|0] \"\" X\n").unwrap_err();
        assert!(matches!(error, DbcError::Syntax { .. }));
        // Start bit beyond the 512-bit FD payload space:
        let error = parse("BO_ 1 M: 8 S\n SG_ A : 512|8@1+ (1,0) [0|0] \"\" X\n").unwrap_err();
        assert!(matches!(error, DbcError::Syntax { .. }));
        // SG_ before BO_:
        let error = parse(" SG_ A : 0|8@1+ (1,0) [0|0] \"\" X\n").unwrap_err();
        assert!(matches!(error, DbcError::Syntax { .. }));
    }

    #[test]
    fn unknown_keywords_are_skipped() {
        let dbc =
            parse("VENDOR \"acme\"\nBO_ 5 Msg: 1 Node\nSG_ A : 7|8@0+ (1,0) [0|255] \"\" X\n")
                .unwrap();
        assert_eq!(dbc.messages.len(), 1);
    }

    #[test]
    fn invalid_ids_are_rejected() {
        // Plain id above the 11-bit standard width without the extended bit:
        let error = parse("BO_ 2048 Bad: 1 X\n").unwrap_err();
        assert!(matches!(
            error,
            DbcError::InvalidId {
                raw: 2048,
                max: 0x7FF
            }
        ));
    }
}
