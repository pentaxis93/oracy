use std::io;
use std::path::{Component, Path};

pub(crate) fn sync_directory(path: &Path) -> io::Result<()> {
    std::fs::File::open(path)?.sync_all()
}

pub(crate) fn sync_directory_chain(root: &Path, leaf: &Path) -> io::Result<()> {
    let relative_leaf = leaf.strip_prefix(root).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "directory sync leaf must be under the durable root",
        )
    })?;

    sync_directory(root)?;
    let mut path = root.to_path_buf();
    for component in relative_leaf.components() {
        match component {
            Component::Normal(name) => {
                path.push(name);
                sync_directory(&path)?;
            }
            Component::CurDir => {}
            _ => {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "directory sync leaf must not traverse outside the durable root",
                ));
            }
        }
    }

    Ok(())
}

pub(crate) fn sync_parent_after_entry_removal(path: &Path) -> io::Result<()> {
    let Some(parent) = path.parent() else {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "removed entry must have a parent directory",
        ));
    };

    match sync_directory(parent) {
        Ok(()) => Ok(()),
        // If another cleaner has already removed the parent directory, the
        // retained file's absence is stronger than a synced parent entry.
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

#[cfg(test)]
mod tests {
    use super::sync_directory_chain;

    #[test]
    fn sync_directory_chain_rejects_leaf_outside_root() {
        let tempdir = tempfile::TempDir::new().expect("tempdir");
        let root = tempdir.path().join("accepted-audio");
        let other = tempdir.path().join("other").join("chunks");
        std::fs::create_dir(&root).expect("create accepted audio dir");
        std::fs::create_dir_all(&other).expect("create outside leaf");

        let error = sync_directory_chain(&root, &other).expect_err("outside leaf should fail");

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidInput);
    }
}
