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

/// Create a parser from a config type string. `database` is the
/// `parser.database` config path, required by `zephyr-dict`.
pub fn create_parser(parser_type: &str, database: Option<&str>) -> Box<dyn StreamParser> {
    match parser_type {
        "text" => Box::new(TextParser::new()),
        "cbor-datagram" => Box::new(CborDatagramParser::new()),
        "slip-coap" => Box::new(SlipCoapParser::new()),
        "zephyr-dict" => Box::new(ZephyrDictParser::new(database.unwrap_or_default())),
        _ => {
            tracing::warn!("unknown parser type {parser_type:?}, falling back to text");
            Box::new(TextParser::new())
        }
    }
}
