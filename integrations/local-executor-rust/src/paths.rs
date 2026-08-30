use crate::{ExecutorError, RootId, ScopedPath};
use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug)]
pub struct AllowedRoot {
    id: RootId,
    canonical_path: PathBuf,
}

impl AllowedRoot {
    pub fn new(id: RootId, path: impl AsRef<Path>) -> Result<Self, ExecutorError> {
        let canonical_path = fs::canonicalize(path)
            .map_err(|_| ExecutorError::InvalidConfiguration("root does not exist"))?;
        if !canonical_path.is_dir() {
            return Err(ExecutorError::InvalidConfiguration(
                "root is not a directory",
            ));
        }
        Ok(Self { id, canonical_path })
    }

    pub fn id(&self) -> &RootId {
        &self.id
    }

    pub fn canonical_path(&self) -> &Path {
        &self.canonical_path
    }
}

#[derive(Clone, Debug)]
pub struct PathPolicy {
    roots: HashMap<RootId, PathBuf>,
}

impl PathPolicy {
    pub fn new(roots: Vec<AllowedRoot>) -> Result<Self, ExecutorError> {
        if roots.is_empty() {
            return Err(ExecutorError::InvalidConfiguration(
                "at least one allowed root is required",
            ));
        }
        let mut indexed = HashMap::new();
        for root in roots {
            if indexed.insert(root.id, root.canonical_path).is_some() {
                return Err(ExecutorError::InvalidConfiguration("duplicate root id"));
            }
        }
        Ok(Self { roots: indexed })
    }

    pub fn resolve_existing_file(&self, scoped: &ScopedPath) -> Result<PathBuf, ExecutorError> {
        scoped.validate()?;
        let root = self
            .roots
            .get(&scoped.root)
            .ok_or(ExecutorError::UnknownRoot)?;
        let candidate =
            fs::canonicalize(root.join(&scoped.relative)).map_err(|_| ExecutorError::PathDenied)?;
        if !candidate.starts_with(root) || !candidate.is_file() {
            return Err(ExecutorError::PathDenied);
        }
        Ok(candidate)
    }

    pub fn resolve_output(&self, scoped: &ScopedPath) -> Result<PathBuf, ExecutorError> {
        scoped.validate()?;
        let root = self
            .roots
            .get(&scoped.root)
            .ok_or(ExecutorError::UnknownRoot)?;
        let relative_parent = scoped.relative.parent().unwrap_or_else(|| Path::new(""));
        let parent =
            fs::canonicalize(root.join(relative_parent)).map_err(|_| ExecutorError::PathDenied)?;
        if !parent.starts_with(root) || !parent.is_dir() {
            return Err(ExecutorError::PathDenied);
        }
        let file_name = scoped
            .relative
            .file_name()
            .ok_or(ExecutorError::PathDenied)?;
        Ok(parent.join(file_name))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[cfg(unix)]
    #[test]
    fn rejects_symlink_parent_that_escapes_root() {
        use std::os::unix::fs::symlink;

        let root = tempdir().unwrap();
        let outside = tempdir().unwrap();
        symlink(outside.path(), root.path().join("escape")).unwrap();
        let root_id = RootId::new("test").unwrap();
        let policy = PathPolicy::new(vec![
            AllowedRoot::new(root_id.clone(), root.path()).unwrap(),
        ])
        .unwrap();
        let output = ScopedPath::new(root_id, "escape/out.mp4").unwrap();
        assert!(matches!(
            policy.resolve_output(&output),
            Err(ExecutorError::PathDenied)
        ));
    }
}
