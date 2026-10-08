use nucleo_matcher::pattern::{CaseMatching, Normalization};

use nucleo_matcher::{Matcher, Utf32String};

use crate::pattern::{MultiPattern, Status, Variant};

#[test]
fn append() {
    let mut pat = MultiPattern::new(1);
    pat.reparse(0, "!", CaseMatching::Smart, Normalization::Smart, true);
    assert_eq!(pat.status(), Status::Update);
    pat.reparse(0, "!f", CaseMatching::Smart, Normalization::Smart, true);
    assert_eq!(pat.status(), Status::Update);
    pat.reparse(0, "!fo", CaseMatching::Smart, Normalization::Smart, true);
    assert_eq!(pat.status(), Status::Rescore);
}

#[test]
fn multi_pattern_scoring_negative_and_optional() {
    let mut matcher = Matcher::default();
    let haystack_clean = [Utf32String::from("hello world"), Utf32String::from("extra")];
    let haystack_dirty = [Utf32String::from("hello foo"), Utf32String::from("extra")];

    // 1. Purely negative query on a single column
    let mut pat = MultiPattern::new(2);
    pat.reparse(0, "!foo", CaseMatching::Smart, Normalization::Smart, false);
    assert_eq!(pat.score(&haystack_clean, &mut matcher), Some(0));
    assert_eq!(pat.score(&haystack_dirty, &mut matcher), None);

    // 2. Purely negative on col 0 + optional positive on col 1
    let mut pat2 = MultiPattern::new(2);
    pat2.configure_column(1, Variant::Optional);
    pat2.reparse(0, "!foo", CaseMatching::Smart, Normalization::Smart, false);
    pat2.reparse(1, "bar", CaseMatching::Smart, Normalization::Smart, false);
    assert_eq!(pat2.score(&haystack_clean, &mut matcher), Some(0));
    assert_eq!(pat2.score(&haystack_dirty, &mut matcher), None);

    // 3. Optional column only, non-matching -> must reject
    let mut pat3 = MultiPattern::new(2);
    pat3.configure_column(1, Variant::Optional);
    pat3.reparse(1, "nomatch", CaseMatching::Smart, Normalization::Smart, false);
    assert_eq!(pat3.score(&haystack_clean, &mut matcher), None);

    // 4. Optional column only, matching -> must accept with score > 0
    let mut pat4 = MultiPattern::new(2);
    pat4.configure_column(1, Variant::Optional);
    pat4.reparse(1, "extra", CaseMatching::Smart, Normalization::Smart, false);
    assert!(pat4.score(&haystack_clean, &mut matcher).is_some_and(|s| s > 0));

    // 5. Empty pattern across all columns -> matches with Some(0)
    let pat5 = MultiPattern::new(2);
    assert_eq!(pat5.score(&haystack_clean, &mut matcher), Some(0));
}
