/// Convert a string into a filesystem-safe slug.
///
/// Mirrors the Python `backend.core.naming.slugify()` behavior:
/// lowercase, alphanumeric + hyphens, no leading/trailing hyphens.
pub fn slugify(input: &str) -> String {
    slug::slugify(input)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basic() {
        assert_eq!(slugify("Hello World"), "hello-world");
    }

    #[test]
    fn special_chars() {
        assert_eq!(slugify("DUT UART /dev/ttyUSB0"), "dut-uart-dev-ttyusb0");
    }

    #[test]
    fn empty() {
        assert_eq!(slugify(""), "");
    }

    #[test]
    fn already_slug() {
        assert_eq!(slugify("my-source"), "my-source");
    }
}
