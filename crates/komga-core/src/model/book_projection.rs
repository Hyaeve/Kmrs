//! Equivalent model for `BookProjection.kt` (+ `BookProjectionProfiles.kt`).

use time::OffsetDateTime;

/// The default profile when using kepubify on an epub, used in `BookProjection.profile`.
pub const KEPUB_DEFAULT: &str = "kepub_default";

/// A representation of a book file converted to a different `profile` will have a different
/// `file_size`.
#[derive(Debug, Clone, PartialEq)]
pub struct BookProjection {
    pub book_id: String,
    pub profile: String,
    pub file_size: i64,
    pub created_date: OffsetDateTime,
    pub last_modified_date: OffsetDateTime,
}
