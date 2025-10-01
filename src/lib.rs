//! High-performance HTTP server
//! Name reserved for future development

/// Placeholder function
pub fn hello() -> &'static str {
    "maker_web - coming soon!"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        assert_eq!(hello(), "maker_web - coming soon!");
    }
}