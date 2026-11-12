#[cfg(test)]
mod tests;

use std::borrow::Cow;
use std::hash::{Hash, Hasher};
use std::ops::{Bound, RangeBounds};
use std::ptr::NonNull;
use std::sync::Arc;
use std::{fmt, mem, slice};

use memchr::memmem;

use crate::chars;
use crate::shared_str::Chunk;
use crate::SharedStr;

/// Check if a given string can be represented internally as the `Ascii` variant in a
/// [`Utf32String`] or a [`Utf32Str`].
///
/// This returns true if the string is ASCII and does not contain a windows-style newline
/// `'\r'`.
/// The additional carriage return check is required since even for strings consisting only
/// of ASCII, the windows-style newline `\r\n` is treated as a single grapheme.
#[inline]
pub(crate) fn has_ascii_graphemes(string: &str) -> bool {
    string.is_ascii() && memmem::find(string.as_bytes(), b"\r\n").is_none()
}

/// A UTF-32 encoded (char array) string that is used as an input to (fuzzy) matching.
///
/// This is mostly intended as an internal string type, but some methods are exposed for
/// convenience. We make the following API guarantees for `Utf32Str(ing)`s produced from a string
/// using one of its `From<T>` constructors for string types `T` or from the
/// [`Utf32Str::new`] method.
///
/// 1. The `Ascii` variant contains a byte buffer which is guaranteed to be a valid string
///    slice.
/// 2. It is guaranteed that the string slice internal to the `Ascii` variant is identical
///    to the original string.
/// 3. The length of a `Utf32Str(ing)` is exactly the number of graphemes in the original string.
///
/// Since `Utf32Str(ing)`s variants may be constructed directly, you **must not** make these
/// assumptions when handling `Utf32Str(ing)`s of unknown origin.
///
/// ## Caveats
/// Despite the name, this type is quite far from being a true string type. Here are some
/// examples demonstrating this.
///
/// ### String conversions are not round-trip
/// In the presence of a multi-codepoint grapheme (e.g. `"u\u{0308}"` which is `u +
/// COMBINING_DIAERESIS`), the trailing codepoints are truncated.
/// ```
/// # use nucleo_matcher::Utf32String;
/// assert_eq!(Utf32String::from("u\u{0308}").to_string(), "u");
/// ```
///
/// ### Indexing is done by grapheme
/// Indexing into a string is done by grapheme rather than by codepoint.
/// ```
/// # use nucleo_matcher::Utf32String;
/// assert!(Utf32String::from("au\u{0308}").len() == 2);
/// ```
///
/// ### A `Unicode` variant may be produced by all-ASCII characters.
/// Since the windows-style newline `\r\n` is ASCII only but considered to be a single grapheme,
/// strings containing `\r\n` will still result in a `Unicode` variant.
/// ```
/// # use nucleo_matcher::Utf32String;
/// let s = Utf32String::from("\r\n");
/// assert!(!s.slice(..).is_ascii());
/// assert!(s.len() == 1);
/// assert!(s.slice(..).get(0) == '\n');
/// ```
///
/// ## Design rationale
/// Usually Rust's UTF-8 encoded strings are great. However, since fuzzy matching
/// operates on codepoints (ideally, it should operate on graphemes but that's too
/// much hassle to deal with), we want to quickly iterate over codepoints (up to 5
/// times) during matching.
///
/// Doing codepoint segmentation on the fly not only blows trough the cache
/// (lookup tables and I-cache) but also has nontrivial runtime compared to the
/// matching itself. Furthermore there are many extra optimizations available
/// for ASCII only text, but checking each match has too much overhead.
///
/// Of course, this comes at extra memory cost as we usually still need the UTF-8
/// encoded variant for rendering. In the (dominant) case of ASCII-only text
/// we don't require a copy. Furthermore fuzzy matching usually is applied while
/// the user is typing on the fly so the same item is potentially matched many
/// times (making the the up-front cost more worth it). That means that its
/// basically always worth it to pre-segment the string.
///
/// For usecases that only match (a lot of) strings once its possible to keep
/// char buffer around that is filled with the presegmented chars.
///
/// Another advantage of this approach is that the matcher will naturally
/// produce grapheme indices (instead of utf8 offsets) anyway. With a
/// codepoint basic representation like this the indices can be used
/// directly
#[derive(PartialEq, Eq, PartialOrd, Ord, Clone, Copy, Hash)]
pub enum Utf32Str<'a> {
    /// A string represented as ASCII encoded bytes.
    /// Correctness invariant: must only contain valid ASCII (`<= 127`)
    Ascii(&'a [u8]),
    /// A string represented as an array of unicode codepoints (basically UTF-32).
    Unicode(&'a [char]),
}

impl<'a> Utf32Str<'a> {
    /// Convenience method to construct a `Utf32Str` from a normal UTF-8 str
    pub fn new(str: &'a str, buf: &'a mut Vec<char>) -> Self {
        if has_ascii_graphemes(str) {
            Utf32Str::Ascii(str.as_bytes())
        } else {
            buf.clear();
            buf.extend(crate::chars::graphemes(str));
            Utf32Str::Unicode(buf)
        }
    }

    /// Returns the number of characters in this string.
    #[inline]
    pub fn len(self) -> usize {
        match self {
            Utf32Str::Unicode(codepoints) => codepoints.len(),
            Utf32Str::Ascii(ascii_bytes) => ascii_bytes.len(),
        }
    }

    /// Returns whether this string is empty.
    #[inline]
    pub fn is_empty(self) -> bool {
        match self {
            Utf32Str::Unicode(codepoints) => codepoints.is_empty(),
            Utf32Str::Ascii(ascii_bytes) => ascii_bytes.is_empty(),
        }
    }

    /// Creates a slice with a string that contains the characters in
    /// the specified **character range**.
    #[inline]
    pub fn slice(self, range: impl RangeBounds<usize>) -> Utf32Str<'a> {
        let start = match range.start_bound() {
            Bound::Included(&start) => start,
            Bound::Excluded(&start) => start + 1,
            Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            Bound::Included(&end) => end + 1,
            Bound::Excluded(&end) => end,
            Bound::Unbounded => self.len(),
        };
        match self {
            Utf32Str::Ascii(bytes) => Utf32Str::Ascii(&bytes[start..end]),
            Utf32Str::Unicode(codepoints) => Utf32Str::Unicode(&codepoints[start..end]),
        }
    }

    /// Returns the number of leading whitespaces in this string
    #[inline]
    pub(crate) fn leading_white_space(self) -> usize {
        match self {
            Utf32Str::Ascii(bytes) => bytes
                .iter()
                .position(|b| !b.is_ascii_whitespace())
                .unwrap_or(0),
            Utf32Str::Unicode(codepoints) => codepoints
                .iter()
                .position(|c| !c.is_whitespace())
                .unwrap_or(0),
        }
    }

    /// Returns the number of trailing whitespaces in this string
    #[inline]
    pub(crate) fn trailing_white_space(self) -> usize {
        match self {
            Utf32Str::Ascii(bytes) => bytes
                .iter()
                .rev()
                .position(|b| !b.is_ascii_whitespace())
                .unwrap_or(0),
            Utf32Str::Unicode(codepoints) => codepoints
                .iter()
                .rev()
                .position(|c| !c.is_whitespace())
                .unwrap_or(0),
        }
    }

    /// Same as `slice` but accepts a u32 range for convenience since
    /// those are the indices returned by the matcher.
    #[inline]
    pub fn slice_u32(self, range: impl RangeBounds<u32>) -> Utf32Str<'a> {
        let start = match range.start_bound() {
            Bound::Included(&start) => start as usize,
            Bound::Excluded(&start) => start as usize + 1,
            Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            Bound::Included(&end) => end as usize + 1,
            Bound::Excluded(&end) => end as usize,
            Bound::Unbounded => self.len(),
        };
        match self {
            Utf32Str::Ascii(bytes) => Utf32Str::Ascii(&bytes[start..end]),
            Utf32Str::Unicode(codepoints) => Utf32Str::Unicode(&codepoints[start..end]),
        }
    }

    /// Returns whether this string only contains graphemes which are single ASCII chars.
    ///
    /// This is almost equivalent to the string being ASCII, except with the additional requirement
    /// that the string cannot contain a windows-style newline `\r\n` which is treated as a single
    /// grapheme.
    pub fn is_ascii(self) -> bool {
        matches!(self, Utf32Str::Ascii(_))
    }

    /// Returns the `n`th character in this string, zero-indexed
    pub fn get(self, n: u32) -> char {
        match self {
            Utf32Str::Ascii(bytes) => bytes[n as usize] as char,
            Utf32Str::Unicode(codepoints) => codepoints[n as usize],
        }
    }

    /// Returns the last character in this string.
    ///
    /// Panics if the string is empty.
    pub(crate) fn last(self) -> char {
        match self {
            Utf32Str::Ascii(bytes) => bytes[bytes.len() - 1] as char,
            Utf32Str::Unicode(codepoints) => codepoints[codepoints.len() - 1],
        }
    }

    /// Returns the first character in this string.
    ///
    /// Panics if the string is empty.
    pub(crate) fn first(self) -> char {
        match self {
            Utf32Str::Ascii(bytes) => bytes[0] as char,
            Utf32Str::Unicode(codepoints) => codepoints[0],
        }
    }

    /// Returns an iterator over the characters in this string
    pub fn chars(self) -> Chars<'a> {
        match self {
            Utf32Str::Ascii(bytes) => Chars::Ascii(bytes.iter()),
            Utf32Str::Unicode(codepoints) => Chars::Unicode(codepoints.iter()),
        }
    }
}

impl fmt::Debug for Utf32Str<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "\"")?;
        for c in self.chars() {
            for c in c.escape_debug() {
                write!(f, "{c}")?
            }
        }
        write!(f, "\"")
    }
}

impl fmt::Display for Utf32Str<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for c in self.chars() {
            write!(f, "{c}")?
        }
        Ok(())
    }
}

pub enum Chars<'a> {
    Ascii(slice::Iter<'a, u8>),
    Unicode(slice::Iter<'a, char>),
}

impl Iterator for Chars<'_> {
    type Item = char;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Chars::Ascii(iter) => iter.next().map(|&c| c as char),
            Chars::Unicode(iter) => iter.next().copied(),
        }
    }
}

impl DoubleEndedIterator for Chars<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        match self {
            Chars::Ascii(iter) => iter.next_back().map(|&c| c as char),
            Chars::Unicode(iter) => iter.next_back().copied(),
        }
    }
}

/// An owned version of [`Utf32Str`].
///
/// See the API documentation for [`Utf32Str`] for more detail.
///
/// It holds one of three kinds of text, all matched the same way:
///
/// - ASCII bytes it owns ([`Utf32String::from_ascii`]),
/// - unicode codepoints it owns, basically UTF-32 ([`Utf32String::from_codepoints`]),
/// - ASCII text shared with the caller ([`SharedStr`]), so an item whose haystack is its whole
///   text stores that text once, and many items can keep their text in one chunk.
///
/// It is two words, so a matcher with millions of items spends 16 bytes per column on it: a
/// pointer, the length, and either the shared text's offset in its chunk or a tag saying
/// which of the owned kinds it is. The comparisons tell the three kinds apart.
pub struct Utf32String {
    /// The owned bytes or codepoints, or the [`Chunk`] behind a shared string.
    ptr: NonNull<u8>,
    /// The length in bytes (ASCII) or codepoints.
    len: u32,
    /// [`ASCII`] or [`UNICODE`] for owned text, otherwise the offset of shared text in its
    /// chunk, which is always below both tags.
    start: u32,
}

const ASCII: u32 = u32::MAX;
const UNICODE: u32 = u32::MAX - 1;

const _: () = assert!(mem::size_of::<Utf32String>() == 16);
const _: () = assert!(mem::size_of::<Option<Utf32String>>() == 16);

// SAFETY: a `Utf32String` owns a `Box<str>`, a `Box<[char]>` or an `Arc<Chunk>`, which are
// all `Send` and `Sync`.
unsafe impl Send for Utf32String {}
unsafe impl Sync for Utf32String {}

/// The text of a [`Utf32String`], by kind.
#[derive(PartialEq, Eq, PartialOrd, Ord, Hash)]
enum Text<'a> {
    Ascii(&'a str),
    Unicode(&'a [char]),
    Shared(&'a str),
}

#[inline]
fn len_u32(len: usize) -> u32 {
    u32::try_from(len).expect("a Utf32String is limited to u32::MAX bytes or codepoints")
}

impl Utf32String {
    /// A string of owned ASCII text.
    ///
    /// Correctness invariant: `text` must only contain valid ASCII (<=127) without `\r\n`;
    /// use a `From` impl to convert any text.
    ///
    /// # Panics
    ///
    /// If `text` is longer than `u32::MAX` bytes.
    #[inline]
    pub fn from_ascii(text: Box<str>) -> Self {
        let len = len_u32(text.len());
        // SAFETY: `Box::into_raw` never returns null.
        let ptr = unsafe { NonNull::new_unchecked(Box::into_raw(text) as *mut u8) };
        Self {
            ptr,
            len,
            start: ASCII,
        }
    }

    /// A string of owned unicode codepoints, one per grapheme.
    ///
    /// # Panics
    ///
    /// If `codepoints` holds more than `u32::MAX` codepoints.
    #[inline]
    pub fn from_codepoints(codepoints: Box<[char]>) -> Self {
        let len = len_u32(codepoints.len());
        // SAFETY: `Box::into_raw` never returns null.
        let ptr = unsafe { NonNull::new_unchecked(Box::into_raw(codepoints) as *mut u8) };
        Self {
            ptr,
            len,
            start: UNICODE,
        }
    }

    #[inline]
    fn text(&self) -> Text<'_> {
        // SAFETY: each constructor stores a pointer and length taken from a live allocation of
        // the kind `start` names, which this string owns until it is dropped.
        unsafe {
            match self.start {
                ASCII => Text::Ascii(std::str::from_utf8_unchecked(slice::from_raw_parts(
                    self.ptr.as_ptr(),
                    self.len as usize,
                ))),
                UNICODE => Text::Unicode(slice::from_raw_parts(
                    self.ptr.as_ptr() as *const char,
                    self.len as usize,
                )),
                start => {
                    let chunk = &*(self.ptr.as_ptr() as *const Chunk);
                    // `start..start + len` was in bounds and on char boundaries of the chunk
                    // when the `SharedStr` was made, and the chunk never changes.
                    Text::Shared(
                        chunk
                            .text
                            .get_unchecked(start as usize..(start + self.len) as usize),
                    )
                }
            }
        }
    }

    /// The shared text this string matches, when it shares a [`SharedStr`]'s chunk.
    #[inline]
    pub fn shared(&self) -> Option<SharedStr> {
        if self.start >= UNICODE {
            return None;
        }
        let ptr = self.ptr.as_ptr() as *const Chunk;
        // SAFETY: `ptr` came from `Arc::into_raw` and this string holds one strong count, so
        // the chunk is live; the count is raised for the new `Arc` first.
        let chunk = unsafe {
            Arc::increment_strong_count(ptr);
            Arc::from_raw(ptr)
        };
        Some(SharedStr {
            chunk,
            start: self.start,
            len: self.len,
        })
    }

    /// Returns the number of characters in this string.
    #[inline]
    pub fn len(&self) -> usize {
        self.len as usize
    }

    /// Returns whether this string is empty.
    #[inline]
    pub fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// Creates a slice with a string that contains the characters in
    /// the specified **character range**.
    #[inline]
    pub fn slice(&self, range: impl RangeBounds<usize>) -> Utf32Str<'_> {
        let start = match range.start_bound() {
            Bound::Included(&start) => start,
            Bound::Excluded(&start) => start + 1,
            Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            Bound::Included(&end) => end + 1,
            Bound::Excluded(&end) => end,
            Bound::Unbounded => self.len(),
        };
        match self.text() {
            Text::Ascii(bytes) | Text::Shared(bytes) => {
                Utf32Str::Ascii(&bytes.as_bytes()[start..end])
            }
            Text::Unicode(codepoints) => Utf32Str::Unicode(&codepoints[start..end]),
        }
    }

    /// Same as `slice` but accepts a u32 range for convenience since
    /// those are the indices returned by the matcher.
    #[inline]
    pub fn slice_u32(&self, range: impl RangeBounds<u32>) -> Utf32Str<'_> {
        let start = match range.start_bound() {
            Bound::Included(&start) => start,
            Bound::Excluded(&start) => start + 1,
            Bound::Unbounded => 0,
        };
        let end = match range.end_bound() {
            Bound::Included(&end) => end + 1,
            Bound::Excluded(&end) => end,
            Bound::Unbounded => self.len,
        };
        self.slice(start as usize..end as usize)
    }
}

impl Drop for Utf32String {
    fn drop(&mut self) {
        let ptr = self.ptr.as_ptr();
        let len = self.len as usize;
        // SAFETY: the pointer and length came from the `into_raw` of the kind `start` names,
        // and this string owns that allocation (or one strong count of the chunk).
        unsafe {
            match self.start {
                ASCII => drop(Box::from_raw(
                    std::ptr::slice_from_raw_parts_mut(ptr, len) as *mut str
                )),
                UNICODE => drop(Box::from_raw(std::ptr::slice_from_raw_parts_mut(
                    ptr as *mut char,
                    len,
                ))),
                _ => drop(Arc::from_raw(ptr as *const Chunk)),
            }
        }
    }
}

impl Clone for Utf32String {
    fn clone(&self) -> Self {
        match self.text() {
            Text::Ascii(text) => Self::from_ascii(text.into()),
            Text::Unicode(codepoints) => Self::from_codepoints(codepoints.into()),
            Text::Shared(_) => {
                let shared = self.shared();
                // `shared` is `Some` for shared text.
                Self::from_shared(shared.unwrap_or_default())
            }
        }
    }
}

impl PartialEq for Utf32String {
    #[inline]
    fn eq(&self, other: &Self) -> bool {
        self.text() == other.text()
    }
}

impl Eq for Utf32String {}

impl PartialOrd for Utf32String {
    #[inline]
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Utf32String {
    #[inline]
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.text().cmp(&other.text())
    }
}

impl Hash for Utf32String {
    #[inline]
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.text().hash(state)
    }
}

impl Default for Utf32String {
    fn default() -> Self {
        Self::from_ascii(Box::default())
    }
}

impl Utf32String {
    /// Shares `value`, which must be ASCII graphemes.
    #[inline]
    fn from_shared(value: SharedStr) -> Self {
        let SharedStr { chunk, start, len } = value;
        if start >= UNICODE {
            // Only an empty string at the very end of a 4 GiB chunk gets here.
            return Self::from_ascii(Box::default());
        }
        // SAFETY: `Arc::into_raw` never returns null.
        let ptr = unsafe { NonNull::new_unchecked(Arc::into_raw(chunk) as *mut u8) };
        Self { ptr, len, start }
    }
}

impl From<&str> for Utf32String {
    #[inline]
    fn from(value: &str) -> Self {
        if has_ascii_graphemes(value) {
            Self::from_ascii(value.into())
        } else {
            Self::from_codepoints(chars::graphemes(value).collect())
        }
    }
}

impl From<Box<str>> for Utf32String {
    fn from(value: Box<str>) -> Self {
        if has_ascii_graphemes(&value) {
            Self::from_ascii(value)
        } else {
            Self::from_codepoints(chars::graphemes(&value).collect())
        }
    }
}

/// Shares `value` when it is ASCII; otherwise converts it to codepoints like the other `From`
/// impls.
impl From<SharedStr> for Utf32String {
    fn from(value: SharedStr) -> Self {
        if value.is_ascii_graphemes() {
            Self::from_shared(value)
        } else {
            Self::from_codepoints(chars::graphemes(&value).collect())
        }
    }
}

/// Copies `value`; use [`SharedStr`] to share the text instead.
impl From<Arc<str>> for Utf32String {
    #[inline]
    fn from(value: Arc<str>) -> Self {
        (*value).into()
    }
}

impl From<String> for Utf32String {
    #[inline]
    fn from(value: String) -> Self {
        value.into_boxed_str().into()
    }
}

impl<'a> From<Cow<'a, str>> for Utf32String {
    #[inline]
    fn from(value: Cow<'a, str>) -> Self {
        match value {
            Cow::Borrowed(value) => value.into(),
            Cow::Owned(value) => value.into(),
        }
    }
}

impl fmt::Debug for Utf32String {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self.slice(..))
    }
}

impl fmt::Display for Utf32String {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.slice(..))
    }
}
