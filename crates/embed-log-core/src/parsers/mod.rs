pub mod hex_coap;
pub mod raw_decode;
pub mod slip_coap;
pub mod text;
pub mod traits;
pub mod zephyr_dict;

pub use hex_coap::HexCoapParser;
pub use slip_coap::SlipCoapParser;
pub use text::TextParser;
pub use traits::StreamParser;
pub use zephyr_dict::binary::ZephyrDictBinaryParser;
pub use zephyr_dict::{validate_dictionary, ZephyrDictParser};

/// Prefix of a `.log` line that carries the bytes of a dictionary record that
/// could not be decoded because no dictionary was available. The rest of the
/// line is the Base64 of those bytes; `embed-log dictionary decode` turns it
/// back into text once the matching dictionary is known.
pub const RAW_LINE_PREFIX: &str = "[dict-raw:v1] ";

/// Prefix of a `.log` line that records bytes the parser received while it decoded them
/// (`parser.raw_capture: true`). The rest is the Base64 of one read, or `@idle` where the
/// parser gave up on an incomplete record after an idle period. The decoded lines stay
/// in the log too; offline decoding replays the capture lines instead of them.
pub const CAPTURE_LINE_PREFIX: &str = "[dict-capture:v1] ";
/// Payload of a capture line that marks an idle abandon.
pub const CAPTURE_IDLE: &str = "@idle";

/// Decode one line of device console text.
///
/// Invalid UTF-8 becomes U+FFFD, and NUL bytes are removed: devices often
/// emit them on reset or line breaks, and a single NUL makes text editors
/// (gedit, Notepad) open the session `.log` as UTF-16, showing the whole file
/// as CJK characters. Other control characters keep their terminal meaning
/// (ESC sequences, CR, backspace) and are left as they are.
///
/// Borrowed for the usual case (valid UTF-8, no NUL), so a caller that trims and
/// copies the line makes one allocation, not two.
pub(crate) fn console_text(bytes: &[u8]) -> std::borrow::Cow<'_, str> {
    let text = String::from_utf8_lossy(bytes);
    if text.contains('\0') {
        std::borrow::Cow::Owned(text.replace('\0', ""))
    } else {
        text
    }
}

/// Fallible construction for runtime replacement; never replace a working parser
/// with one that only reports a load error after receiving bytes.
pub fn try_create_dictionary_parser(
    parser: &ParserConfig,
) -> Result<Box<dyn StreamParser>, String> {
    let path = parser
        .database
        .as_deref()
        .ok_or("dictionary path is missing")?;
    match parser.parser_type.as_str() {
        "zephyr-dict" => Ok(Box::new(
            ZephyrDictBinaryParser::try_new(path)?.with_raw_capture(parser.raw_capture),
        )),
        "zephyr-dict-hex" => Ok(Box::new(ZephyrDictParser::try_new(path)?)),
        _ => Err("parser_type must be zephyr-dict or zephyr-dict-hex".into()),
    }
}

use crate::config::models::ParserConfig;

/// Create a parser from a config type string.
pub fn create_parser(parser: &ParserConfig) -> Box<dyn StreamParser> {
    match parser.parser_type.as_str() {
        "text" => Box::new(TextParser::new()),
        "hex-coap" => Box::new(HexCoapParser::new()),
        "slip-coap" => Box::new(SlipCoapParser::new()),
        "zephyr-dict" => Box::new(
            ZephyrDictBinaryParser::new(parser.database.as_deref().unwrap_or_default())
                .with_raw_capture(parser.raw_capture),
        ),
        "zephyr-dict-hex" => Box::new(ZephyrDictParser::new(
            parser.database.as_deref().unwrap_or_default(),
        )),
        _ => {
            tracing::warn!(
                "unknown parser type {:?}, falling back to text",
                parser.parser_type
            );
            Box::new(TextParser::new())
        }
    }
}

#[cfg(test)]
mod console_text_tests {
    use super::console_text;
    use std::borrow::Cow;

    #[test]
    fn plain_text_is_not_copied() {
        assert!(matches!(console_text(b"uart:~$ ready"), Cow::Borrowed(_)));
        assert!(matches!(
            console_text("zażółć".as_bytes()),
            Cow::Borrowed(_)
        ));
    }

    #[test]
    fn nul_and_invalid_utf8_are_still_cleaned() {
        assert_eq!(console_text(b"a\0b\0"), "ab");
        assert_eq!(console_text(b"ok\xff!"), "ok\u{fffd}!");
    }
}
