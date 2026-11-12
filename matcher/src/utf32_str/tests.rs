use crate::{Utf32Str, Utf32String};

#[test]
fn test_utf32str_ascii() {
    /// Helper function for testing
    fn expect_ascii(src: &str, is_ascii: bool) {
        let mut buffer = Vec::new();
        assert!(Utf32Str::new(src, &mut buffer).is_ascii() == is_ascii);
        assert!(Utf32String::from(src).slice(..).is_ascii() == is_ascii);
        assert!(Utf32String::from(src.to_owned()).slice(..).is_ascii() == is_ascii);
    }

    // ascii
    expect_ascii("", true);
    expect_ascii("a", true);
    expect_ascii("a\nb", true);
    expect_ascii("\n\r", true);

    // not ascii
    expect_ascii("aü", false);
    expect_ascii("au\u{0308}", false);

    // windows-style newline
    expect_ascii("a\r\nb", false);
    expect_ascii("ü\r\n", false);
    expect_ascii("\r\n", false);
}

#[test]
fn test_grapheme_truncation() {
    // ascii is preserved
    let s = Utf32String::from("ab");
    assert_eq!(s.slice(..).get(0), 'a');
    assert_eq!(s.slice(..).get(1), 'b');

    // windows-style newline is truncated to '\n'
    let s = Utf32String::from("\r\n");
    assert_eq!(s.slice(..).get(0), '\n');

    // normal graphemes are truncated to the first character
    let s = Utf32String::from("u\u{0308}\r\n");
    assert_eq!(s.slice(..).get(0), 'u');
    assert_eq!(s.slice(..).get(1), '\n');
}

#[test]
fn shared_matches_ascii() {
    let chunk = crate::SharedChunk::new("src/main.rs\nother");
    let shared = Utf32String::from(chunk.slice(0..11));
    assert!(shared.shared().is_some());
    let owned = Utf32String::from("src/main.rs");
    assert_eq!(shared.len(), owned.len());
    assert_eq!(shared.slice(4..8), owned.slice(4..8));
    assert_eq!(shared.slice_u32(..), owned.slice_u32(..));
    assert_eq!(shared.to_string(), "src/main.rs");
    // Non-ASCII text cannot be shared and is converted to codepoints.
    let unicode = Utf32String::from(crate::SharedStr::from("a\u{00e9}b"));
    assert!(unicode.shared().is_none());
    assert!(matches!(unicode.slice(..), Utf32Str::Unicode(_)));
    assert_eq!(unicode.len(), 3);
}

#[test]
fn arc_str_is_copied() {
    let text: std::sync::Arc<str> = "src/main.rs".into();
    let copy = Utf32String::from(text);
    assert!(copy.shared().is_none());
    assert!(matches!(copy.slice(..), Utf32Str::Ascii(_)));
}

#[test]
fn every_kind_clones_compares_and_drops() {
    let chunk = crate::SharedChunk::new("src/main.rs\nother");
    let kinds = [
        Utf32String::from("src/main.rs"),
        Utf32String::from("a\u{00e9}b"),
        Utf32String::from(chunk.slice(0..11)),
        Utf32String::from(chunk.slice(12..17)),
        Utf32String::default(),
        Utf32String::from(chunk.slice(17..17)),
    ];
    for s in &kinds {
        let copy = s.clone();
        assert_eq!(&copy, s);
        assert_eq!(copy.to_string(), s.to_string());
        assert_eq!(copy.shared().is_some(), s.shared().is_some());
    }
    assert_eq!(kinds[2].to_string(), "src/main.rs");
    assert_eq!(kinds[3].to_string(), "other");
    assert!(kinds[5].is_empty());
    // The same text owned and shared compares as different kinds, like the enum it was.
    assert_ne!(kinds[0], kinds[2]);
    assert_eq!(kinds[0].slice(..), kinds[2].slice(..));
    let shared = kinds[2].shared().unwrap();
    assert!(crate::SharedChunk::ptr_eq(&shared.chunk(), &chunk));
    assert_eq!(&*shared, "src/main.rs");
    // The chunk outlives the strings cut from it, whichever is dropped first.
    drop(chunk);
    drop(shared);
    assert_eq!(kinds[3].clone().to_string(), "other");
}

#[test]
fn is_two_words() {
    assert_eq!(std::mem::size_of::<Utf32String>(), 16);
}
