//! Path-safety checks shared by every store: a base directory must be non-empty
//! with no `..` component, and every derived child path must stay under its
//! base. `Err` carries the offending path so each store can wrap it into its
//! own `UnsafeBasePath` variant.

use std::path::{Component, Path, PathBuf};

pub(crate) fn validate_base_dir(base_dir: &Path) -> Result<(), PathBuf> {
    if base_dir.as_os_str().is_empty()
        || base_dir
            .components()
            .any(|component| matches!(component, Component::ParentDir))
    {
        return Err(base_dir.to_path_buf());
    }

    Ok(())
}

pub(crate) fn validate_child_path(base_dir: &Path, child_path: &Path) -> Result<(), PathBuf> {
    if child_path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
        || !child_path.starts_with(base_dir)
    {
        return Err(child_path.to_path_buf());
    }

    Ok(())
}
