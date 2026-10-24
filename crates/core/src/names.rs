use crate::{Error, Result};
use unicode_normalization::UnicodeNormalization;

// Experimental, versioned rule, not a claim of identical NTFS/APFS collation.
// Preserve the input display name; compare NFC + Rust Unicode lowercase + NFC.
pub(crate) fn key(name: &str) -> Result<String> {
    if name.is_empty()
        || name.len() > 255
        || name.ends_with(['.', ' '])
        || name
            .chars()
            .any(|c| c.is_control() || "\\/:*?\"<>|".contains(c))
    {
        return Err(Error::InvalidName);
    }
    let key: String = name.nfc().flat_map(char::to_lowercase).nfc().collect();
    let base = key.split('.').next().unwrap_or("");
    if key == "."
        || key == ".."
        || matches!(base, "con" | "prn" | "aux" | "nul")
        || ((base.starts_with("com") || base.starts_with("lpt"))
            && matches!(
                base.strip_prefix("com")
                    .or_else(|| base.strip_prefix("lpt")),
                Some("1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³")
            ))
    {
        return Err(Error::InvalidName);
    }
    Ok(key)
}
