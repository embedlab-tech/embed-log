pub mod hex_coap;
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
        "zephyr-dict" => Ok(Box::new(ZephyrDictBinaryParser::try_new(path)?)),
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
        "zephyr-dict" => Box::new(ZephyrDictBinaryParser::new(
            parser.database.as_deref().unwrap_or_default(),
        )),
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
