//! Document Chunker — intelligent text splitting for RAG embeddings.
//!
//! Ported from `sovereign_titan/documents/chunker.py`.
//! Supports three chunking strategies:
//! - **Fixed**: fixed-size chunks with overlap, breaks at sentence boundaries
//! - **Semantic**: sentence-based chunking with overlap
//! - **Code**: respects function/class boundaries (falls back to fixed)

use regex::Regex;

/// Default chunk size in characters (~128 tokens at 4 chars/token).
const DEFAULT_CHUNK_SIZE: usize = 512;

/// Default overlap between consecutive chunks.
const DEFAULT_CHUNK_OVERLAP: usize = 50;

/// A single chunk of text with metadata.
#[derive(Debug, Clone)]
pub struct Chunk {
    /// The chunk text content.
    pub text: String,
    /// Zero-based index of this chunk.
    pub index: usize,
    /// Optional metadata about the chunk.
    pub chunk_type: ChunkType,
}

/// The type/strategy that produced this chunk.
#[derive(Debug, Clone, PartialEq)]
pub enum ChunkType {
    Fixed { start_char: usize, end_char: usize },
    Semantic { sentence_count: usize },
    Code,
    Text,
}

/// Chunking strategy selector.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Strategy {
    Fixed,
    Semantic,
    Code,
}

impl Strategy {
    /// Parse a strategy from a string.
    pub fn from_str_loose(s: &str) -> Self {
        match s.to_lowercase().as_str() {
            "semantic" => Self::Semantic,
            "code" => Self::Code,
            _ => Self::Fixed,
        }
    }
}

/// Configurable document chunker.
pub struct DocumentChunker {
    chunk_size: usize,
    chunk_overlap: usize,
}

impl DocumentChunker {
    /// Create a chunker with custom sizes.
    pub fn new(chunk_size: usize, chunk_overlap: usize) -> Self {
        Self {
            chunk_size,
            chunk_overlap,
        }
    }

    /// Chunk text using the specified strategy.
    pub fn chunk_text(&self, text: &str, strategy: Strategy) -> Vec<Chunk> {
        if text.is_empty() {
            return Vec::new();
        }

        match strategy {
            Strategy::Fixed => self.chunk_fixed(text),
            Strategy::Semantic => self.chunk_semantic(text),
            Strategy::Code => self.chunk_code(text),
        }
    }

    /// Fixed-size chunking with overlap.
    ///
    /// Tries to break at sentence boundaries (`. `) when possible.
    fn chunk_fixed(&self, text: &str) -> Vec<Chunk> {
        let mut chunks = Vec::new();
        let chars: Vec<char> = text.chars().collect();
        let total = chars.len();
        let mut start = 0;
        let mut chunk_num = 0;

        while start < total {
            let mut end = (start + self.chunk_size).min(total);

            // Try to break at a sentence boundary if not at the end.
            if end < total {
                let chunk_str: String = chars[start..end].iter().collect();
                if let Some(last_period) = chunk_str.rfind(". ") {
                    if last_period > self.chunk_size / 2 {
                        end = start + last_period + 1;
                    }
                }
            }

            let chunk_text: String = chars[start..end].iter().collect();
            let trimmed = chunk_text.trim();
            if !trimmed.is_empty() {
                chunks.push(Chunk {
                    text: trimmed.to_string(),
                    index: chunk_num,
                    chunk_type: ChunkType::Fixed {
                        start_char: start,
                        end_char: end,
                    },
                });
                chunk_num += 1;
            }

            // Advance with overlap.
            start = if end <= start + self.chunk_overlap {
                end
            } else {
                end - self.chunk_overlap
            };
        }

        chunks
    }

    /// Sentence-based semantic chunking.
    ///
    /// Splits on sentence endings (`[.!?]` followed by whitespace) and
    /// groups sentences until the chunk size limit is reached.
    fn chunk_semantic(&self, text: &str) -> Vec<Chunk> {
        // Split on sentence boundaries: period/exclamation/question + whitespace.
        // Rust regex doesn't support look-behind, so we split manually.
        let sentence_re = Regex::new(r"[.!?]\s+").unwrap();
        let mut sentences = Vec::new();
        let mut last = 0;
        for m in sentence_re.find_iter(text) {
            // Include the punctuation in the sentence.
            sentences.push(&text[last..m.start() + 1]);
            last = m.end();
        }
        if last < text.len() {
            sentences.push(&text[last..]);
        }

        let mut chunks = Vec::new();
        let mut current: Vec<&str> = Vec::new();
        let mut current_len = 0;
        let mut chunk_num = 0;

        for sentence in &sentences {
            let slen = sentence.len();

            if current_len + slen > self.chunk_size && !current.is_empty() {
                let sentence_count = current.len();
                chunks.push(Chunk {
                    text: current.join(" "),
                    index: chunk_num,
                    chunk_type: ChunkType::Semantic { sentence_count },
                });
                chunk_num += 1;

                // Keep last sentence for overlap.
                if let Some(last) = current.last().copied() {
                    current = vec![last];
                    current_len = last.len();
                } else {
                    current.clear();
                    current_len = 0;
                }
            }

            current.push(sentence);
            current_len += slen;
        }

        // Add remaining.
        if !current.is_empty() {
            let sentence_count = current.len();
            let text = current.join(" ");
            if !text.trim().is_empty() {
                chunks.push(Chunk {
                    text,
                    index: chunk_num,
                    chunk_type: ChunkType::Semantic { sentence_count },
                });
            }
        }

        chunks
    }

    /// Code-aware chunking that respects function/class boundaries.
    ///
    /// Detects `def`, `fn`, `class`, `async def`, `pub fn`, etc. and
    /// keeps each definition as a separate chunk. Falls back to fixed
    /// chunking if no definitions are found.
    fn chunk_code(&self, text: &str) -> Vec<Chunk> {
        // Pattern to match function/class definitions across multiple languages.
        let pattern = Regex::new(
            r"(?m)^((?:pub\s+)?(?:async\s+)?(?:fn|def|class|function|func)\s+[^\n]+\n(?:[ \t]+[^\n]*\n)*)"
        ).unwrap();

        let matches: Vec<_> = pattern.find_iter(text).collect();

        if matches.is_empty() {
            return self.chunk_fixed(text);
        }

        let mut chunks = Vec::new();
        let mut last_end = 0;
        let mut chunk_num = 0;

        for m in &matches {
            // Add any text before this match.
            if m.start() > last_end {
                let pre_text = text[last_end..m.start()].trim();
                if !pre_text.is_empty() {
                    chunks.push(Chunk {
                        text: pre_text.to_string(),
                        index: chunk_num,
                        chunk_type: ChunkType::Text,
                    });
                    chunk_num += 1;
                }
            }

            let func_text = m.as_str().trim();

            // If the definition is too long, sub-chunk it.
            if func_text.len() > self.chunk_size * 2 {
                let sub_chunks = self.chunk_fixed(func_text);
                for mut sc in sub_chunks {
                    sc.index = chunk_num;
                    sc.chunk_type = ChunkType::Code;
                    chunks.push(sc);
                    chunk_num += 1;
                }
            } else {
                chunks.push(Chunk {
                    text: func_text.to_string(),
                    index: chunk_num,
                    chunk_type: ChunkType::Code,
                });
                chunk_num += 1;
            }

            last_end = m.end();
        }

        // Add remaining text.
        if last_end < text.len() {
            let remaining = text[last_end..].trim();
            if !remaining.is_empty() {
                chunks.push(Chunk {
                    text: remaining.to_string(),
                    index: chunk_num,
                    chunk_type: ChunkType::Text,
                });
            }
        }

        chunks
    }
}

impl Default for DocumentChunker {
    fn default() -> Self {
        Self::new(DEFAULT_CHUNK_SIZE, DEFAULT_CHUNK_OVERLAP)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_text() {
        let chunker = DocumentChunker::default();
        let chunks = chunker.chunk_text("", Strategy::Fixed);
        assert!(chunks.is_empty());
    }

    #[test]
    fn test_fixed_single_chunk() {
        let chunker = DocumentChunker::new(1000, 50);
        let text = "This is a short document.";
        let chunks = chunker.chunk_text(text, Strategy::Fixed);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].text, text);
    }

    #[test]
    fn test_fixed_multiple_chunks() {
        let chunker = DocumentChunker::new(50, 10);
        let text = "This is sentence one. This is sentence two. This is sentence three. This is sentence four. Done.";
        let chunks = chunker.chunk_text(text, Strategy::Fixed);
        assert!(chunks.len() > 1);
        // All text should be covered.
        for chunk in &chunks {
            assert!(!chunk.text.is_empty());
        }
    }

    #[test]
    fn test_fixed_sentence_boundary() {
        let chunker = DocumentChunker::new(60, 10);
        let text = "First sentence here. Second sentence here. Third sentence follows.";
        let chunks = chunker.chunk_text(text, Strategy::Fixed);
        // Should break at a period when possible.
        assert!(chunks.len() >= 1);
    }

    #[test]
    fn test_semantic_chunking() {
        let chunker = DocumentChunker::new(50, 10);
        let text = "First sentence. Second sentence. Third sentence. Fourth sentence. Fifth sentence.";
        let chunks = chunker.chunk_text(text, Strategy::Semantic);
        assert!(chunks.len() > 1);
        for chunk in &chunks {
            assert!(!chunk.text.is_empty());
            if let ChunkType::Semantic { sentence_count } = chunk.chunk_type {
                assert!(sentence_count > 0);
            } else {
                panic!("Expected semantic chunk type");
            }
        }
    }

    #[test]
    fn test_code_chunking() {
        let code = "\
# module header
import os

def hello():
    print('hello')
    return True

def world():
    print('world')
    return False

# footer
";
        let chunker = DocumentChunker::new(512, 50);
        let chunks = chunker.chunk_text(code, Strategy::Code);
        assert!(chunks.len() >= 2);
        // At least one code chunk should exist.
        assert!(chunks.iter().any(|c| c.chunk_type == ChunkType::Code));
    }

    #[test]
    fn test_code_fallback_to_fixed() {
        let text = "No function definitions here. Just plain text content.";
        let chunker = DocumentChunker::new(512, 50);
        let chunks = chunker.chunk_text(text, Strategy::Code);
        assert!(!chunks.is_empty());
    }

    #[test]
    fn test_strategy_from_str() {
        assert_eq!(Strategy::from_str_loose("fixed"), Strategy::Fixed);
        assert_eq!(Strategy::from_str_loose("semantic"), Strategy::Semantic);
        assert_eq!(Strategy::from_str_loose("code"), Strategy::Code);
        assert_eq!(Strategy::from_str_loose("SEMANTIC"), Strategy::Semantic);
        assert_eq!(Strategy::from_str_loose("unknown"), Strategy::Fixed);
    }

    #[test]
    fn test_chunk_indices_sequential() {
        let chunker = DocumentChunker::new(30, 5);
        let text = "Word one. Word two. Word three. Word four. Word five. Word six.";
        let chunks = chunker.chunk_text(text, Strategy::Fixed);
        for (i, chunk) in chunks.iter().enumerate() {
            assert_eq!(chunk.index, i);
        }
    }
}
