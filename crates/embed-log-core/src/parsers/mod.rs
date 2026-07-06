pub mod cbor;
pub mod slip_coap;
pub mod text;
pub mod traits;

pub use cbor::CborDatagramParser;
pub use slip_coap::SlipCoapParser;
pub use text::TextParser;
pub use traits::StreamParser;

/// Create a parser from a config type string.
pub fn create_parser(parser_type: &str) -> Box<dyn StreamParser> {
    match parser_type {
        "text" => Box::new(TextParser::new()),
        "cbor-datagram" => Box::new(CborDatagramParser::new()),
        "slip-coap" => Box::new(SlipCoapParser::new()),
        _ => {
            tracing::warn!("unknown parser type {parser_type:?}, falling back to text");
            Box::new(TextParser::new())
        }
    }
}
