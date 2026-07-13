pub mod cbor;
pub mod slip_coap;
pub mod text;
pub mod traits;
pub mod zephyr_dict;

pub use cbor::CborDatagramParser;
pub use slip_coap::SlipCoapParser;
pub use text::TextParser;
pub use traits::StreamParser;
pub use zephyr_dict::ZephyrDictParser;

use crate::config::models::ParserConfig;

/// Create a parser from a config type string.
pub fn create_parser(parser: &ParserConfig) -> Box<dyn StreamParser> {
    match parser.parser_type.as_str() {
        "text" => Box::new(TextParser::new()),
        "cbor-datagram" => Box::new(CborDatagramParser::new()),
        "slip-coap" => Box::new(SlipCoapParser::new()),
        "zephyr-dict" => Box::new(ZephyrDictParser::new(
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
