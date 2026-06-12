use common::{ChunkId, ChunkMetadata};
use std::fs::File;
use std::io::{Read, Result};
use std::path::Path;

pub fn chunk_file<P: AsRef<Path>>(path: P, chunk_size: usize) -> Result<Vec<ChunkMetadata>> {
    let mut file = File::open(path)?;
    let mut chunks = Vec::new();

    let mut offset = 0u64;
    let mut chunk_id = 0u64;

    loop {
        let mut buffer = vec![0u8; chunk_size];
        let bytes_read = file.read(&mut buffer)?;

        if bytes_read == 0 {
            break;
        }

        chunks.push(ChunkMetadata {
            id: ChunkId(chunk_id),
            offset,
            size: bytes_read as u64,
        });

        offset += bytes_read as u64;
        chunk_id += 1;
    }

    Ok(chunks)
}
