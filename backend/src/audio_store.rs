use std::path::{Path, PathBuf};

use tokio::io::{AsyncWriteExt, BufReader};
use ulid::Ulid;

use crate::audio_durability::sync_directory_chain;
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
