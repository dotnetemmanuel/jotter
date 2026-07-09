#![warn(clippy::pedantic)]
//! Placeholder crate for jotter-app. Real implementation lands in a later phase.

/// Returns a fixed greeting. Placeholder until this crate gains real logic.
#[must_use]
pub fn hello() -> &'static str {
    "hello from jotter-app"
}

#[cfg(test)]
mod tests {
    use super::hello;

    #[test]
    fn hello_greets() {
        assert!(hello().starts_with("hello"));
    }
}
