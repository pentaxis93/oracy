use std::path::{Component, Path, PathBuf};

use tokio::io::{AsyncWriteExt, BufReader};
use ulid::Ulid;

use crate::storage::ChunkRecord;

pub const MAX_CHUNK_BYTES: usize = 26_214_400;
pub const MULTIPART_BODY_LIMIT_BYTES: usize = MAX_CHUNK_BYTES + 1_048_576;

pub async fn persist_chunk(
    accepted_audio_dir: &Path,
    job_id: &str,
    chunk_index: i64,
    bytes: &[u8],
) -> std::io::Result<PathBuf> {
    let chunk_dir = accepted_audio_dir.join(job_id).join("chunks");
    tokio::fs::create_dir_all(&chunk_dir).await?;
    let final_path = chunk_dir.join(format!("{chunk_index}.{}.chunk", Ulid::new()));

    let mut file = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&final_path)
        .await?;
    file.write_all(bytes).await?;
    file.flush().await?;
    file.sync_all().await?;
    drop(file);

    sync_directory_chain(accepted_audio_dir, &chunk_dir)?;
    Ok(final_path)
}

pub async fn compose_chunks(
    accepted_audio_dir: &Path,
    job_id: &str,
    audio_format: &str,
    chunks: &[ChunkRecord],
) -> std::io::Result<PathBuf> {
    let job_dir = accepted_audio_dir.join(job_id);
    tokio::fs::create_dir_all(&job_dir).await?;
    let final_path = job_dir.join(format!("accepted.{audio_format}"));
    let temp_path = job_dir.join(format!(".accepted.{}.tmp", Ulid::new()));

    let mut output = tokio::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&temp_path)
        .await?;
    for chunk in chunks {
        let input = tokio::fs::File::open(&chunk.chunk_path).await?;
        let mut input = BufReader::new(input);
        tokio::io::copy(&mut input, &mut output).await?;
    }
    output.flush().await?;
    output.sync_all().await?;
    drop(output);

    tokio::fs::rename(&temp_path, &final_path).await?;
    sync_directory_chain(accepted_audio_dir, &job_dir)?;
    Ok(final_path)
}

fn sync_directory(path: &Path) -> std::io::Result<()> {
    std::fs::File::open(path)?.sync_all()
}

fn sync_directory_chain(root: &Path, leaf: &Path) -> std::io::Result<()> {
    let relative_leaf = leaf.strip_prefix(root).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
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
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "directory sync leaf must not traverse outside the durable root",
                ));
            }
        }
    }

    Ok(())
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
