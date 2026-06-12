use common::{
    Checkpoint, Chunk, ChunkId, ChunkMetadata, DEFAULT_CHUNK_SIZE, FileManifest, MissingChunks,
    TransferId,
};
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Error, ErrorKind, Read, Result, Seek, SeekFrom};
use std::path::Path;

pub fn default_chunk_size() -> u64 {
    DEFAULT_CHUNK_SIZE
}

pub fn chunk_file<P: AsRef<Path>>(path: P, chunk_size: usize) -> Result<Vec<ChunkMetadata>> {
    validate_chunk_size(chunk_size)?;

    let mut file = File::open(path)?;
    let mut chunks = Vec::new();

    let mut offset = 0u64;
    let mut chunk_id = 0u64;
    let mut buffer = vec![0u8; chunk_size];

    loop {
        let bytes_read = file.read(&mut buffer)?;

        if bytes_read == 0 {
            break;
        }

        let data = &buffer[..bytes_read];
        chunks.push(ChunkMetadata {
            id: ChunkId(chunk_id),
            offset,
            size: bytes_read as u64,
            sha256: sha256_hex(data),
        });

        offset += bytes_read as u64;
        chunk_id += 1;
    }

    Ok(chunks)
}

pub fn read_chunk<P: AsRef<Path>>(path: P, metadata: &ChunkMetadata) -> Result<Chunk> {
    let mut file = File::open(path)?;
    file.seek(SeekFrom::Start(metadata.offset))?;

    let mut data = vec![0u8; metadata.size as usize];
    file.read_exact(&mut data)?;

    Ok(Chunk {
        id: metadata.id.clone(),
        offset: metadata.offset,
        size: metadata.size,
        data,
    })
}

pub fn generate_manifest<P: AsRef<Path>>(path: P, chunk_size: usize) -> Result<FileManifest> {
    validate_chunk_size(chunk_size)?;

    let path = path.as_ref();
    let metadata = std::fs::metadata(path)?;
    let size = metadata.len();
    let total_chunks = size.div_ceil(chunk_size as u64);
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            Error::new(
                ErrorKind::InvalidInput,
                "path must include a valid file name",
            )
        })?
        .to_owned();

    Ok(FileManifest {
        name,
        size,
        chunk_size: chunk_size as u64,
        total_chunks,
        sha256: sha256_file(path)?,
    })
}

pub fn verify_chunk(chunk: &Chunk, metadata: &ChunkMetadata) -> bool {
    chunk.id == metadata.id
        && chunk.offset == metadata.offset
        && chunk.size == metadata.size
        && chunk.data.len() as u64 == metadata.size
        && sha256_hex(&chunk.data) == metadata.sha256
}

pub fn missing_chunks(
    transfer_id: TransferId,
    manifest: &FileManifest,
    checkpoint: &Checkpoint,
) -> MissingChunks {
    let completed = checkpoint
        .completed_chunks
        .iter()
        .map(|chunk| chunk.0)
        .collect::<std::collections::HashSet<_>>();

    let chunks = (0..manifest.total_chunks)
        .filter(|chunk_id| !completed.contains(chunk_id))
        .map(ChunkId)
        .collect();

    MissingChunks {
        transfer_id,
        chunks,
    }
}

pub fn sha256_hex(data: &[u8]) -> String {
    let digest = Sha256::digest(data);
    hex_lower(&digest)
}

pub fn sha256_file<P: AsRef<Path>>(path: P) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];

    loop {
        let bytes_read = file.read(&mut buffer)?;
        if bytes_read == 0 {
            break;
        }
        hasher.update(&buffer[..bytes_read]);
    }

    Ok(hex_lower(&hasher.finalize()))
}

fn validate_chunk_size(chunk_size: usize) -> Result<()> {
    if chunk_size == 0 {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "chunk size must be greater than zero",
        ));
    }

    Ok(())
}

fn hex_lower(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::io::Write;
    use std::time::{SystemTime, UNIX_EPOCH};

    #[test]
    fn chunk_file_generates_fixed_chunks_with_hashes() {
        let path = write_temp_file("chunks", b"abcdefghij");

        let chunks = chunk_file(&path, 4).expect("chunk metadata");

        assert_eq!(chunks.len(), 3);
        assert_eq!(chunks[0].id, ChunkId(0));
        assert_eq!(chunks[0].offset, 0);
        assert_eq!(chunks[0].size, 4);
        assert_eq!(chunks[0].sha256, sha256_hex(b"abcd"));
        assert_eq!(chunks[2].id, ChunkId(2));
        assert_eq!(chunks[2].offset, 8);
        assert_eq!(chunks[2].size, 2);
        assert_eq!(chunks[2].sha256, sha256_hex(b"ij"));

        fs::remove_file(path).ok();
    }

    #[test]
    fn generate_manifest_supports_empty_files() {
        let path = write_temp_file("empty", b"");

        let manifest = generate_manifest(&path, 4).expect("manifest");

        assert_eq!(manifest.size, 0);
        assert_eq!(manifest.chunk_size, 4);
        assert_eq!(manifest.total_chunks, 0);
        assert_eq!(manifest.sha256, sha256_hex(b""));

        fs::remove_file(path).ok();
    }

    #[test]
    fn read_chunk_returns_requested_bytes_and_verifies() {
        let path = write_temp_file("read", b"abcdefghij");
        let chunks = chunk_file(&path, 4).expect("chunk metadata");

        let first = read_chunk(&path, &chunks[0]).expect("first chunk");
        let middle = read_chunk(&path, &chunks[1]).expect("middle chunk");
        let last = read_chunk(&path, &chunks[2]).expect("last chunk");

        assert_eq!(first.data, b"abcd");
        assert_eq!(middle.data, b"efgh");
        assert_eq!(last.data, b"ij");
        assert!(verify_chunk(&first, &chunks[0]));
        assert!(verify_chunk(&middle, &chunks[1]));
        assert!(verify_chunk(&last, &chunks[2]));

        fs::remove_file(path).ok();
    }

    #[test]
    fn verify_chunk_rejects_hash_mismatch() {
        let metadata = ChunkMetadata {
            id: ChunkId(0),
            offset: 0,
            size: 3,
            sha256: sha256_hex(b"abc"),
        };
        let chunk = Chunk {
            id: ChunkId(0),
            offset: 0,
            size: 3,
            data: b"abd".to_vec(),
        };

        assert!(!verify_chunk(&chunk, &metadata));
    }

    #[test]
    fn missing_chunks_excludes_completed_chunks() {
        let transfer_id = TransferId("transfer-1".to_owned());
        let manifest = FileManifest {
            name: "file.bin".to_owned(),
            size: 10,
            chunk_size: 2,
            total_chunks: 5,
            sha256: sha256_hex(b"abcdefghij"),
        };
        let checkpoint = Checkpoint {
            transfer_id: transfer_id.clone(),
            completed_chunks: vec![ChunkId(0), ChunkId(2), ChunkId(4)],
        };

        let missing = missing_chunks(transfer_id.clone(), &manifest, &checkpoint);

        assert_eq!(missing.transfer_id, transfer_id);
        assert_eq!(missing.chunks, vec![ChunkId(1), ChunkId(3)]);
    }

    #[test]
    fn invalid_chunk_size_is_rejected() {
        let path = write_temp_file("invalid", b"abc");

        let error = chunk_file(&path, 0).expect_err("invalid size");

        assert_eq!(error.kind(), ErrorKind::InvalidInput);

        fs::remove_file(path).ok();
    }

    #[test]
    fn manifest_chunks_and_reader_reconstruct_file() {
        let data = b"the quick brown fox jumps over the lazy dog";
        let path = write_temp_file("integration", data);

        let manifest = generate_manifest(&path, 8).expect("manifest");
        let chunks = chunk_file(&path, 8).expect("chunks");
        let mut reconstructed = Vec::new();

        for metadata in &chunks {
            let chunk = read_chunk(&path, metadata).expect("chunk");
            assert!(verify_chunk(&chunk, metadata));
            reconstructed.extend_from_slice(&chunk.data);
        }

        assert_eq!(manifest.total_chunks, chunks.len() as u64);
        assert_eq!(manifest.sha256, sha256_hex(data));
        assert_eq!(reconstructed, data);

        fs::remove_file(path).ok();
    }

    fn write_temp_file(label: &str, data: &[u8]) -> std::path::PathBuf {
        let unique = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("system time")
            .as_nanos();
        let path =
            std::env::temp_dir().join(format!("nexo-{label}-{}-{unique}.tmp", std::process::id()));
        let mut file = File::create(&path).expect("create temp file");
        file.write_all(data).expect("write temp file");
        path
    }
}
