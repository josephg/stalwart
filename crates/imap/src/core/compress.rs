/*
 * SPDX-FileCopyrightText: 2020 Stalwart Labs LLC <hello@stalw.art>
 *
 * SPDX-License-Identifier: AGPL-3.0-only OR LicenseRef-SEL
 */

use flate2::{Compress, Compression, Decompress, FlushCompress, FlushDecompress};
use std::io;
use tokio::io::AsyncWriteExt;

/// Compressor for outgoing IMAP data using raw DEFLATE (RFC 1951).
pub struct DeflateCompressor {
    compress: Compress,
    buf: Vec<u8>,
}

/// Decompressor for incoming IMAP data using raw DEFLATE (RFC 1951).
pub struct DeflateDecompressor {
    decompress: Decompress,
    buf: Vec<u8>,
}

impl DeflateCompressor {
    pub fn new() -> Self {
        Self {
            // false = raw deflate (no zlib header), per RFC 4978
            compress: Compress::new(Compression::default(), false),
            buf: Vec::with_capacity(8192),
        }
    }

    pub fn compress(&mut self, input: &[u8]) -> io::Result<&[u8]> {
        self.buf.clear();

        let mut input_pos = 0;

        // Compress input data
        while input_pos < input.len() {
            let old_len = self.buf.len();
            self.buf.resize(old_len + input.len() + 1024, 0);

            let before_in = self.compress.total_in();
            let before_out = self.compress.total_out();

            self.compress
                .compress(
                    &input[input_pos..],
                    &mut self.buf[old_len..],
                    FlushCompress::None,
                )
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

            input_pos += (self.compress.total_in() - before_in) as usize;
            let produced = (self.compress.total_out() - before_out) as usize;
            self.buf.truncate(old_len + produced);
        }

        // Sync flush to ensure the peer can decompress without waiting for more data
        loop {
            let old_len = self.buf.len();
            self.buf.resize(old_len + 4096, 0);

            let before_out = self.compress.total_out();

            self.compress
                .compress(&[], &mut self.buf[old_len..], FlushCompress::Sync)
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

            let produced = (self.compress.total_out() - before_out) as usize;
            self.buf.truncate(old_len + produced);

            if produced == 0 {
                break;
            }
        }

        Ok(&self.buf)
    }
}

impl DeflateDecompressor {
    pub fn new() -> Self {
        Self {
            // false = raw deflate (no zlib header), per RFC 4978
            decompress: Decompress::new(false),
            buf: Vec::with_capacity(8192),
        }
    }

    pub fn decompress(&mut self, input: &[u8]) -> io::Result<Vec<u8>> {
        self.buf.clear();

        let mut input_pos = 0;

        loop {
            let old_len = self.buf.len();
            let avail_out = (self.buf.capacity() - old_len).max(8192);
            self.buf.resize(old_len + avail_out, 0);

            let before_in = self.decompress.total_in();
            let before_out = self.decompress.total_out();

            self.decompress
                .decompress(
                    &input[input_pos..],
                    &mut self.buf[old_len..],
                    FlushDecompress::Sync,
                )
                .map_err(|e| io::Error::new(io::ErrorKind::Other, e.to_string()))?;

            let consumed = (self.decompress.total_in() - before_in) as usize;
            let produced = (self.decompress.total_out() - before_out) as usize;
            input_pos += consumed;
            self.buf.truncate(old_len + produced);

            if consumed == 0 && produced == 0 {
                break;
            }
        }

        Ok(self.buf.clone())
    }
}

/// Wraps a writer (typically `WriteHalf<T>`) with optional DEFLATE compression.
/// Lives inside the same `Arc<Mutex<>>` as the writer to ensure atomic
/// compress-then-write operations.
pub struct SessionWriter<W: tokio::io::AsyncWrite + Unpin> {
    pub writer: W,
    compressor: Option<DeflateCompressor>,
}

impl<W: tokio::io::AsyncWrite + Unpin> SessionWriter<W> {
    pub fn new(writer: W) -> Self {
        Self {
            writer,
            compressor: None,
        }
    }

    pub fn enable_compression(&mut self) {
        self.compressor = Some(DeflateCompressor::new());
    }

    pub fn is_compressed(&self) -> bool {
        self.compressor.is_some()
    }

    pub async fn write_all(&mut self, data: &[u8]) -> io::Result<()> {
        if let Some(ref mut compressor) = self.compressor {
            let compressed = compressor.compress(data)?;
            self.writer.write_all(compressed).await
        } else {
            self.writer.write_all(data).await
        }
    }

    pub async fn flush(&mut self) -> io::Result<()> {
        self.writer.flush().await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compress_decompress_roundtrip() {
        let mut compressor = DeflateCompressor::new();
        let mut decompressor = DeflateDecompressor::new();

        let original = b"* OK [CAPABILITY IMAP4rev1 COMPRESS=DEFLATE] Stalwart ready\r\n";

        let compressed = compressor.compress(original).unwrap();
        assert_ne!(compressed.len(), 0);

        let decompressed = decompressor.decompress(compressed).unwrap();
        assert_eq!(&decompressed, original);
    }

    #[test]
    fn compress_decompress_multiple_messages() {
        let mut compressor = DeflateCompressor::new();
        let mut decompressor = DeflateDecompressor::new();

        let messages = [
            b"a1 OK LOGIN completed\r\n".as_slice(),
            b"* LIST (\\HasNoChildren) \".\" \"INBOX\"\r\n".as_slice(),
            b"a2 OK LIST completed\r\n".as_slice(),
            b"* 1 FETCH (FLAGS (\\Seen) BODY[HEADER] {256}\r\n".as_slice(),
        ];

        // Compress and decompress each message through the same stream
        for msg in &messages {
            let compressed = compressor.compress(msg).unwrap();
            let decompressed = decompressor.decompress(compressed).unwrap();
            assert_eq!(&decompressed, msg);
        }
    }

    #[test]
    fn compress_large_data() {
        let mut compressor = DeflateCompressor::new();
        let mut decompressor = DeflateDecompressor::new();

        // Generate a large, compressible message
        let mut large = Vec::new();
        for i in 0..1000 {
            large.extend_from_slice(format!("* {} FETCH (FLAGS (\\Seen))\r\n", i).as_bytes());
        }

        let compressed = compressor.compress(&large).unwrap();
        // Should actually compress well
        assert!(compressed.len() < large.len());

        let decompressed = decompressor.decompress(compressed).unwrap();
        assert_eq!(decompressed, large);
    }
}
