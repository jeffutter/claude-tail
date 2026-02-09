use anyhow::Result;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

/// A byte-level index of JSONL line start positions.
/// Scans raw bytes for newlines — no JSON parsing.
#[derive(Debug, Clone)]
pub struct LineIndex {
    /// Byte offset where each JSONL line starts. offsets[0] = 0.
    offsets: Vec<u64>,
    /// File size at last index time.
    indexed_bytes: u64,
}

impl LineIndex {
    /// Build a complete index of a JSONL file by scanning for newlines
    pub fn build(path: &Path) -> Result<Self> {
        let file = File::open(path)?;
        let file_size = file.metadata()?.len();

        if file_size == 0 {
            return Ok(Self {
                offsets: vec![0],
                indexed_bytes: 0,
            });
        }

        let mut reader = BufReader::with_capacity(64 * 1024, file);
        let mut offsets = vec![0]; // First line always starts at byte 0
        let mut current_offset = 0u64;
        let mut buffer = vec![0u8; 64 * 1024];

        loop {
            let bytes_read = reader.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }

            // Scan for newlines in this chunk
            for (i, &byte) in buffer[..bytes_read].iter().enumerate() {
                if byte == b'\n' {
                    // Record the start of the NEXT line (after this newline)
                    let next_line_offset = current_offset + i as u64 + 1;
                    offsets.push(next_line_offset);
                }
            }

            current_offset += bytes_read as u64;
        }

        Ok(Self {
            offsets,
            indexed_bytes: file_size,
        })
    }

    /// Update index incrementally by scanning from indexed_bytes to EOF.
    /// Detects file truncation and triggers full re-index if needed.
    /// Returns the count of new lines discovered.
    pub fn update(&mut self, path: &Path) -> Result<usize> {
        let file = File::open(path)?;
        let file_size = file.metadata()?.len();

        // Detect truncation
        if file_size < self.indexed_bytes {
            // File was truncated - rebuild index from scratch
            *self = Self::build(path)?;
            return Ok(self.offsets.len().saturating_sub(1));
        }

        // No new data
        if file_size == self.indexed_bytes {
            return Ok(0);
        }

        // Seek to where we left off
        let mut file = file;
        file.seek(SeekFrom::Start(self.indexed_bytes))?;
        let mut reader = BufReader::with_capacity(64 * 1024, file);

        let mut current_offset = self.indexed_bytes;
        let mut buffer = vec![0u8; 64 * 1024];
        let initial_line_count = self.offsets.len();

        loop {
            let bytes_read = reader.read(&mut buffer)?;
            if bytes_read == 0 {
                break;
            }

            // Scan for newlines in this chunk
            for (i, &byte) in buffer[..bytes_read].iter().enumerate() {
                if byte == b'\n' {
                    let next_line_offset = current_offset + i as u64 + 1;
                    self.offsets.push(next_line_offset);
                }
            }

            current_offset += bytes_read as u64;
        }

        self.indexed_bytes = file_size;
        let new_lines = self.offsets.len() - initial_line_count;
        Ok(new_lines)
    }

    /// Total number of lines indexed
    pub fn line_count(&self) -> usize {
        // offsets[0] = 0 (start of first line)
        // If there's at least one byte, we have at least one line
        // Each newline adds another line to offsets
        if self.indexed_bytes == 0 {
            0
        } else {
            // offsets.len() includes the initial 0, so:
            // - Empty file: offsets = [0], indexed_bytes = 0 → 0 lines
            // - One line no newline: offsets = [0], indexed_bytes > 0 → 1 line
            // - One line with newline: offsets = [0, N], indexed_bytes > 0 → 1 line (newline starts line 2)
            // We count lines by: how many line starts can yield valid content?
            // If last offset == indexed_bytes, that's an empty trailing line (don't count it)
            let count = self.offsets.len();
            if count > 1 && self.offsets[count - 1] == self.indexed_bytes {
                // Last "line start" is at EOF with no content → don't count it
                count - 1
            } else {
                // Either single line with content, or all line starts have content
                count
            }
        }
    }

    /// Get the byte range for a single line (start inclusive, end exclusive)
    pub fn line_byte_range(&self, line: usize) -> Option<(u64, u64)> {
        if line >= self.line_count() {
            return None;
        }

        let start = self.offsets[line];
        let end = if line + 1 < self.offsets.len() {
            // Next line exists - end at the newline (before next line start)
            self.offsets[line + 1].saturating_sub(1)
        } else {
            // Last line - end at EOF
            self.indexed_bytes
        };

        Some((start, end))
    }

    /// Get the byte range for a range of lines [start..end)
    pub fn range_byte_range(&self, start: usize, end: usize) -> Option<(u64, u64)> {
        if start >= end || start >= self.line_count() {
            return None;
        }

        let actual_end = end.min(self.line_count());
        let start_byte = self.offsets[start];

        let end_byte = if actual_end < self.offsets.len() {
            // End line exists - end at the newline before next line
            self.offsets[actual_end].saturating_sub(1)
        } else {
            // Range extends to or past last line - end at EOF
            self.indexed_bytes
        };

        Some((start_byte, end_byte))
    }

    /// Get the total indexed bytes (file size at last index)
    pub fn indexed_bytes(&self) -> u64 {
        self.indexed_bytes
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;

    #[test]
    fn test_empty_file() {
        let file = NamedTempFile::new().unwrap();
        let index = LineIndex::build(file.path()).unwrap();

        assert_eq!(index.line_count(), 0);
        assert_eq!(index.indexed_bytes(), 0);
        assert_eq!(index.line_byte_range(0), None);
    }

    #[test]
    fn test_single_line_no_newline() {
        let mut file = NamedTempFile::new().unwrap();
        write!(file, "hello world").unwrap();
        file.flush().unwrap();

        let index = LineIndex::build(file.path()).unwrap();

        assert_eq!(index.line_count(), 1);
        assert_eq!(index.indexed_bytes(), 11);
        assert_eq!(index.line_byte_range(0), Some((0, 11)));
        assert_eq!(index.line_byte_range(1), None);
    }

    #[test]
    fn test_single_line_with_newline() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "hello world").unwrap();
        file.flush().unwrap();

        let index = LineIndex::build(file.path()).unwrap();

        assert_eq!(index.line_count(), 1);
        assert_eq!(index.indexed_bytes(), 12);
        assert_eq!(index.line_byte_range(0), Some((0, 11)));
    }

    #[test]
    fn test_multiple_lines() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "line 1").unwrap();
        writeln!(file, "line 2").unwrap();
        writeln!(file, "line 3").unwrap();
        file.flush().unwrap();

        let index = LineIndex::build(file.path()).unwrap();

        assert_eq!(index.line_count(), 3);
        assert_eq!(index.line_byte_range(0), Some((0, 6)));
        assert_eq!(index.line_byte_range(1), Some((7, 13)));
        assert_eq!(index.line_byte_range(2), Some((14, 20)));
        assert_eq!(index.line_byte_range(3), None);
    }

    #[test]
    fn test_range_byte_range() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "line 1").unwrap();
        writeln!(file, "line 2").unwrap();
        writeln!(file, "line 3").unwrap();
        file.flush().unwrap();

        let index = LineIndex::build(file.path()).unwrap();

        assert_eq!(index.range_byte_range(0, 2), Some((0, 13)));
        assert_eq!(index.range_byte_range(1, 3), Some((7, 20)));
        assert_eq!(index.range_byte_range(0, 3), Some((0, 20)));
        assert_eq!(index.range_byte_range(2, 2), None); // Empty range
        assert_eq!(index.range_byte_range(3, 4), None); // Start out of bounds
    }

    #[test]
    fn test_incremental_update() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "line 1").unwrap();
        file.flush().unwrap();

        let mut index = LineIndex::build(file.path()).unwrap();
        assert_eq!(index.line_count(), 1);

        // Append more lines
        writeln!(file, "line 2").unwrap();
        writeln!(file, "line 3").unwrap();
        file.flush().unwrap();

        let new_lines = index.update(file.path()).unwrap();
        assert_eq!(new_lines, 2);
        assert_eq!(index.line_count(), 3);
        assert_eq!(index.line_byte_range(2), Some((14, 20)));
    }

    #[test]
    fn test_truncation_detection() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "line 1").unwrap();
        writeln!(file, "line 2").unwrap();
        writeln!(file, "line 3").unwrap();
        file.flush().unwrap();

        let path = file.path().to_path_buf();
        let mut index = LineIndex::build(&path).unwrap();
        assert_eq!(index.line_count(), 3);

        // Truncate file using std::fs
        std::fs::OpenOptions::new()
            .write(true)
            .open(&path)
            .unwrap()
            .set_len(7)
            .unwrap(); // Keep only first line + newline

        let new_lines = index.update(&path).unwrap();
        assert_eq!(index.line_count(), 1);
        // Returns the count of lines in rebuilt index
        assert!(new_lines <= 1);
    }

    #[test]
    fn test_no_new_data() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "line 1").unwrap();
        file.flush().unwrap();

        let mut index = LineIndex::build(file.path()).unwrap();
        let new_lines = index.update(file.path()).unwrap();

        assert_eq!(new_lines, 0);
        assert_eq!(index.line_count(), 1);
    }

    #[test]
    fn test_utf8_safe() {
        let mut file = NamedTempFile::new().unwrap();
        writeln!(file, "hello 世界").unwrap();
        writeln!(file, "🎉🎊").unwrap();
        file.flush().unwrap();

        let index = LineIndex::build(file.path()).unwrap();
        assert_eq!(index.line_count(), 2);

        // Byte ranges should be correct even with multi-byte UTF-8
        let (start, end) = index.line_byte_range(0).unwrap();
        let mut file = File::open(file.path()).unwrap();
        let mut buffer = vec![0u8; (end - start) as usize];
        file.seek(SeekFrom::Start(start)).unwrap();
        file.read_exact(&mut buffer).unwrap();
        let content = String::from_utf8(buffer).unwrap();
        assert_eq!(content, "hello 世界");
    }
}
