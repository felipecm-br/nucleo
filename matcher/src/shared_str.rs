//! [`SharedStr`]: a reference-counted slice of a shared text chunk.

use std::borrow::Borrow;
use std::cmp::Ordering;
use std::hash::{Hash, Hasher};
use std::ops::Deref;
use std::sync::Arc;
use std::{fmt, mem};

/// An immutable string that is a slice of a reference-counted chunk of text.
///
/// Many strings can share one chunk, so a caller that reads many short records (for example
/// the lines of a file) can store their text in a few large allocations instead of one per
/// record. A [`SharedStr`] is two words: a pointer to the chunk and the slice's 32-bit
/// offset and length. The chunk lives as long as any slice of it.
#[derive(Clone)]
pub struct SharedStr {
    pub(crate) chunk: Arc<Chunk>,
    pub(crate) start: u32,
    pub(crate) len: u32,
}

const _: () = assert!(mem::size_of::<SharedStr>() == 16);

/// The text behind [`SharedStr`]s, with whether it can be matched as plain ASCII (see
/// [`SharedStr::is_ascii_graphemes`]), worked out once for the whole chunk.
pub(crate) struct Chunk {
    pub(crate) text: Box<str>,
    ascii: bool,
}

impl Chunk {
    #[inline]
    fn new(text: Box<str>) -> Arc<Self> {
        let ascii = crate::utf32_str::has_ascii_graphemes(&text);
        Arc::new(Self { text, ascii })
    }
}

impl Deref for Chunk {
    type Target = str;
    #[inline]
    fn deref(&self) -> &str {
        &self.text
    }
}

impl SharedStr {
    /// A string that owns a chunk of its own.
    ///
    /// # Panics
    ///
    /// If `text` is longer than `u32::MAX` bytes.
    pub fn new(text: impl Into<Box<str>>) -> Self {
        let chunk: Box<str> = text.into();
        let len = byte_len(chunk.len());
        Self {
            chunk: Chunk::new(chunk),
            start: 0,
            len,
        }
    }

    /// The text of this string.
    #[inline]
    pub fn as_str(&self) -> &str {
        // SAFETY: `start..start + len` is in bounds and on char boundaries of `chunk`, which
        // every constructor checks and which never changes (the chunk is immutable).
        unsafe {
            self.chunk
                .get_unchecked(self.start as usize..(self.start + self.len) as usize)
        }
    }

    /// Whether this text can be matched as plain ASCII: it is ASCII and has no `\r\n`
    /// (which is one grapheme). Free when the whole chunk qualifies, which is checked once
    /// when the chunk is made; otherwise the slice itself is checked.
    #[inline]
    pub fn is_ascii_graphemes(&self) -> bool {
        self.chunk.ascii || crate::utf32_str::has_ascii_graphemes(self.as_str())
    }

    /// The chunk this string is a slice of. Strings cut from the same chunk return the same
    /// [`SharedChunk`].
    #[inline]
    pub fn chunk(&self) -> SharedChunk {
        SharedChunk(self.chunk.clone())
    }

    /// Whether `a` and `b` are slices of the same chunk. Unlike comparing [`Self::chunk`]s,
    /// this does not touch the chunk's reference count.
    #[inline]
    pub fn same_chunk(a: &Self, b: &Self) -> bool {
        Arc::ptr_eq(&a.chunk, &b.chunk)
    }

    /// A slice of this string over `range` (byte offsets into this string), sharing its chunk.
    ///
    /// # Panics
    ///
    /// If `range` is out of bounds or not on char boundaries.
    pub fn slice(&self, range: std::ops::Range<usize>) -> Self {
        let _ = &self.as_str()[range.clone()];
        Self {
            chunk: self.chunk.clone(),
            start: self.start + range.start as u32,
            len: (range.end - range.start) as u32,
        }
    }
}

/// A reference-counted chunk of text that [`SharedStr`]s are cut from.
#[derive(Clone)]
pub struct SharedChunk(Arc<Chunk>);

impl SharedChunk {
    /// A chunk holding `text`.
    ///
    /// # Panics
    ///
    /// If `text` is longer than `u32::MAX` bytes.
    pub fn new(text: impl Into<Box<str>>) -> Self {
        let text: Box<str> = text.into();
        byte_len(text.len());
        Self(Chunk::new(text))
    }

    /// The whole text of the chunk.
    #[inline]
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// A string over `range` (byte offsets into the chunk) that shares this chunk.
    ///
    /// # Panics
    ///
    /// If `range` is out of bounds or not on char boundaries.
    #[inline]
    pub fn slice(&self, range: std::ops::Range<usize>) -> SharedStr {
        let _ = &self.0[range.clone()];
        SharedStr {
            chunk: self.0.clone(),
            start: range.start as u32,
            len: (range.end - range.start) as u32,
        }
    }

    /// Whether `a` and `b` are the same chunk.
    #[inline]
    pub fn ptr_eq(a: &Self, b: &Self) -> bool {
        Arc::ptr_eq(&a.0, &b.0)
    }
}

impl fmt::Debug for SharedChunk {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_tuple("SharedChunk").field(&self.0.len()).finish()
    }
}

#[inline]
fn byte_len(len: usize) -> u32 {
    u32::try_from(len).expect("shared text chunks are limited to u32::MAX bytes")
}

impl Deref for SharedStr {
    type Target = str;
    #[inline]
    fn deref(&self) -> &str {
        self.as_str()
    }
}

impl AsRef<str> for SharedStr {
    #[inline]
    fn as_ref(&self) -> &str {
        self.as_str()
    }
}

impl Borrow<str> for SharedStr {
    #[inline]
    fn borrow(&self) -> &str {
        self.as_str()
    }
}

impl Default for SharedStr {
    fn default() -> Self {
        Self::new("")
    }
}

impl From<&str> for SharedStr {
    #[inline]
    fn from(value: &str) -> Self {
        Self::new(value)
    }
}

impl From<String> for SharedStr {
    #[inline]
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<Box<str>> for SharedStr {
    #[inline]
    fn from(value: Box<str>) -> Self {
        Self::new(value)
    }
}

impl From<Arc<str>> for SharedStr {
    #[inline]
    fn from(value: Arc<str>) -> Self {
        Self::new(&*value)
    }
}

impl<'a> From<std::borrow::Cow<'a, str>> for SharedStr {
    #[inline]
    fn from(value: std::borrow::Cow<'a, str>) -> Self {
        Self::new(value.into_owned())
    }
}

impl PartialEq for SharedStr {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.as_str() == other.as_str()
    }
}

impl Eq for SharedStr {}

impl PartialEq<str> for SharedStr {
    #[inline]
    fn eq(&self, other: &str) -> bool {
        self.as_str() == other
    }
}

impl PartialEq<&str> for SharedStr {
    #[inline]
    fn eq(&self, other: &&str) -> bool {
        self.as_str() == *other
    }
}

impl PartialOrd for SharedStr {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for SharedStr {
    #[inline]
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_str().cmp(other.as_str())
    }
}

impl Hash for SharedStr {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_str().hash(state)
    }
}

impl fmt::Debug for SharedStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Debug::fmt(self.as_str(), f)
    }
}

impl fmt::Display for SharedStr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        fmt::Display::fmt(self.as_str(), f)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slices_share_one_chunk() {
        let chunk = SharedChunk::new("alpha\nbeta\ngamma");
        let a = chunk.slice(0..5);
        let b = chunk.slice(6..10);
        assert_eq!(a, "alpha");
        assert_eq!(&*b, "beta");
        assert!(SharedChunk::ptr_eq(&a.chunk(), &b.chunk()));
        assert!(SharedStr::same_chunk(&a, &b));
        assert!(a.is_ascii_graphemes() && b.is_ascii_graphemes());
        assert!(!SharedStr::same_chunk(&a, &SharedStr::from("alpha")));
        let c = b.slice(1..3);
        assert_eq!(c.as_str(), "et");
        assert!(SharedChunk::ptr_eq(&c.chunk(), &chunk));
    }

    #[test]
    fn ascii_graphemes_per_slice_when_the_chunk_is_not_plain() {
        let chunk = SharedChunk::new("ab\r\ncd\né");
        assert!(chunk.slice(0..2).is_ascii_graphemes());
        assert!(!chunk.slice(0..4).is_ascii_graphemes());
        assert!(chunk.slice(4..6).is_ascii_graphemes());
        assert!(!chunk.slice(7..9).is_ascii_graphemes());
    }

    #[test]
    fn compares_by_text() {
        let chunk = SharedChunk::new("abab");
        assert_eq!(chunk.slice(0..2), chunk.slice(2..4));
        assert_eq!(chunk.slice(0..2), SharedStr::from("ab"));
        assert!(SharedStr::from("a").cmp(&SharedStr::from("b")).is_lt());
    }

    #[test]
    #[should_panic]
    fn rejects_non_char_boundary() {
        let chunk = SharedChunk::new("é");
        let _ = chunk.slice(0..1);
    }

    #[test]
    #[should_panic]
    fn rejects_out_of_bounds() {
        let _ = SharedStr::from("ab").slice(1..3);
    }
}
