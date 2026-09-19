//! Error codes: corresponds to `ERRORCODES.md` at the repo root; `message` is the `ERR_xxxx` code itself.

/// Business error; the HTTP layer maps it to 400 + message=code (the equivalent of Java's `CodedException`).
#[derive(Debug, thiserror::Error)]
#[error("{0}")]
pub struct CodedError(pub &'static str);

pub mod codes {
    pub const ERR_1000: &str = "ERR_1000"; // File not accessible
    pub const ERR_1001: &str = "ERR_1001"; // Unsupported media type
    pub const ERR_1002: &str = "ERR_1002"; // Encrypted RAR
    pub const ERR_1003: &str = "ERR_1003"; // Solid RAR
    pub const ERR_1004: &str = "ERR_1004"; // Multi-volume RAR
    pub const ERR_1005: &str = "ERR_1005"; // Unknown exception during analysis
    pub const ERR_1006: &str = "ERR_1006"; // No pages
    pub const ERR_1007: &str = "ERR_1007"; // Some entries have unknown type (entry names attached to message)
    pub const ERR_1008: &str = "ERR_1008"; // Extraction failed
    pub const ERR_1009: &str = "ERR_1009"; // A read list with that name already exists
    pub const ERR_1015: &str = "ERR_1015"; // ComicRack reading list deserialization failed
    pub const ERR_1016: &str = "ERR_1016"; // Library root not accessible
    pub const ERR_1017: &str = "ERR_1017"; // Scanned directory already belongs to another library
    pub const ERR_1018: &str = "ERR_1018"; // Import: file does not exist
    pub const ERR_1019: &str = "ERR_1019"; // Import: file already in library
    pub const ERR_1020: &str = "ERR_1020"; // Import: file does not belong to the series
    pub const ERR_1021: &str = "ERR_1021"; // Import: target already exists
    pub const ERR_1022: &str = "ERR_1022"; // Scan after import failed
    pub const ERR_1023: &str = "ERR_1023"; // Book already in read list
    pub const ERR_1024: &str = "ERR_1024"; // OAuth2: no email attribute
    pub const ERR_1025: &str = "ERR_1025"; // OAuth2: no local user and auto-creation not enabled
    pub const ERR_1026: &str = "ERR_1026"; // OIDC: email not verified
    pub const ERR_1027: &str = "ERR_1027"; // OIDC: no email_verified claim
    pub const ERR_1028: &str = "ERR_1028"; // OIDC: no email claim
    pub const ERR_1029: &str = "ERR_1029"; // CBL missing Book element
    pub const ERR_1030: &str = "ERR_1030"; // CBL missing Name
    pub const ERR_1031: &str = "ERR_1031"; // CBL missing series+number
    pub const ERR_1032: &str = "ERR_1032"; // EPUB media type error
    pub const ERR_1033: &str = "ERR_1033"; // Entry missing
    pub const ERR_1034: &str = "ERR_1034"; // Duplicate API key comment
    pub const ERR_1035: &str = "ERR_1035"; // Failed to get EPUB TOC
    pub const ERR_1036: &str = "ERR_1036"; // Failed to get EPUB landmarks
    pub const ERR_1037: &str = "ERR_1037"; // Failed to get EPUB page list
    pub const ERR_1038: &str = "ERR_1038"; // Failed to get EPUB divina
    pub const ERR_1039: &str = "ERR_1039"; // Failed to get EPUB positions
}
