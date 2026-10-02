//! Writer and reader for the hand-rolled TOML subset the stores persist: one
//! `key = "value"` pair per line plus `[[agents]]` array headers, with the
//! escape rules shared through `raptor_core`.

use std::path::{Path, PathBuf};

use raptor_core::{escape_kv_string, unescape_kv_string};

/// Appends `key = "value"` when `value` is present; absent values write nothing.
pub(crate) fn push_toml_string(output: &mut String, key: &str, value: Option<&str>) {
    if let Some(value) = value {
        output.push_str(&format!("{key} = \"{}\"\n", escape_toml_string(value)));
    }
}

/// Appends a path as a quoted string, via its `Display` form.
pub(crate) fn push_toml_path(output: &mut String, key: &str, value: Option<&PathBuf>) {
    if let Some(value) = value {
        push_toml_string(output, key, Some(&value.display().to_string()));
    }
}

/// Escapes a value for `key = "value"` output; the inverse of
/// [`unescape_toml_string`].
pub(crate) fn escape_toml_string(value: &str) -> String {
    escape_kv_string(value)
}

/// Parses the right-hand side of a `key = value` line: it must be a quoted
/// string, whose escapes are then undone. `Err` carries the message for
/// `line_number` without a file path so each store can wrap it into its own
/// `Parse` error.
pub(crate) fn parse_quoted_value(line_number: usize, value: &str) -> Result<String, String> {
    if !value.starts_with('"') || !value.ends_with('"') || value.len() < 2 {
        return Err(format!("line {line_number} value is not a quoted string"));
    }

    unescape_toml_string(line_number, &value[1..value.len() - 1])
}

/// Undoes [`escape_toml_string`], naming `line_number` in the error message.
fn unescape_toml_string(line_number: usize, value: &str) -> Result<String, String> {
    unescape_kv_string(value).map_err(|error| format!("line {line_number} {error}"))
}

/// Returns the id encoded in a `<id>.toml` file name, or `None` for any other
/// entry found in a store directory.
pub(crate) fn id_from_metadata_path(path: &Path) -> Option<String> {
    if path.extension()?.to_str()? != "toml" {
        return None;
    }

    Some(path.file_stem()?.to_str()?.to_owned())
}
