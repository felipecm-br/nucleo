pub use nucleo_matcher::pattern::{Atom, AtomKind, CaseMatching, Normalization, Pattern};
use nucleo_matcher::{Matcher, Utf32String};

#[cfg(test)]
mod tests;

#[derive(Debug, PartialEq, Eq, Clone, Copy, PartialOrd, Ord, Default)]
pub(crate) enum Status {
    #[default]
    Unchanged,
    Update,
    Rescore,
}

/// Matching behavior variant for a column in a [`MultiPattern`].
#[derive(Debug, PartialEq, Eq, Clone, Copy, PartialOrd, Ord, Default)]
pub enum Variant {
    /// Standard matching behavior: any specified pattern for this column must match.
    #[default]
    Default,
    /// Optional column matching behavior.
    ///
    /// If patterns are specified across multiple columns, a failure to match this column
    /// does not reject the item (its score contribution simply falls back to 0).
    /// However, if this is the only column with a pattern specified, it functions as a filter
    /// and must match for the item to be accepted.
    Optional,
}

#[derive(Debug)]
pub struct MultiPattern {
    cols: Vec<(Pattern, Status, Variant)>,
}

impl Clone for MultiPattern {
    fn clone(&self) -> Self {
        Self {
            cols: self.cols.clone(),
        }
    }

    fn clone_from(&mut self, source: &Self) {
        self.cols.clone_from(&source.cols)
    }
}

impl MultiPattern {
    /// Creates a multi pattern with `columns` empty column patterns.
    pub fn new(columns: usize) -> Self {
        Self {
            cols: vec![Default::default(); columns],
        }
    }

    pub fn configure_column(&mut self, index: usize, variant: Variant) {
        self.cols[index].2 = variant
    }

    /// Reparses a column. By specifying `append` the caller promises that text passed
    /// to the previous `reparse` invocation is a prefix of `new_text`. This enables
    /// additional optimizations but can lead to missing matches if an incorrect value
    /// is passed.
    pub fn reparse(
        &mut self,
        column: usize,
        new_text: &str,
        case_matching: CaseMatching,
        normalization: Normalization,
        append: bool,
    ) {
        let old_status = self.cols[column].1;
        if append
            && old_status != Status::Rescore
            && self.cols[column]
                .0
                .atoms
                .last()
                .is_none_or(|last| !last.negative)
        {
            self.cols[column].1 = Status::Update;
        } else {
            self.cols[column].1 = Status::Rescore;
        }
        self.cols[column]
            .0
            .reparse(new_text, case_matching, normalization);
    }

    pub fn column_pattern(&self, column: usize) -> &Pattern {
        &self.cols[column].0
    }

    pub(crate) fn status(&self) -> Status {
        self.cols
            .iter()
            .map(|x| x.1)
            .max()
            .unwrap_or(Status::Unchanged)
    }

    pub(crate) fn reset_status(&mut self) {
        for (_, status, ..) in &mut self.cols {
            *status = Status::Unchanged
        }
    }

    pub fn score(&self, haystack: &[Utf32String], matcher: &mut Matcher) -> Option<u32> {
        // TODO: column weights as a feature?
        let mut score = 0;
        let mut matched_non_empty = false;
        let mut has_non_empty = false;
        for ((pattern, _, variant), haystack) in self.cols.iter().zip(haystack) {
            let res = pattern.score(haystack.slice(..), matcher);
            if !pattern.atoms.is_empty() {
                has_non_empty = true;
                if res.is_some() {
                    matched_non_empty = true;
                }
            }
            let fallback = matches!(variant, Variant::Optional).then_some(0);
            score += res.or(fallback)?;
        }
        (score > 0 || matched_non_empty || !has_non_empty).then_some(score)
    }

    pub fn is_empty(&self) -> bool {
        self.cols.iter().all(|(pat, ..)| pat.atoms.is_empty())
    }
}
