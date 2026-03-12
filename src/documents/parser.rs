//! Document Parser — multi-format text extraction.
//!
//! Ported from `sovereign_titan/documents/parser.py`.
//! Parses PDF, DOCX, plain text, code, CSV, and structured formats
//! into plain text with metadata.

use std::collections::HashMap;
use std::fs;
use std::io::Read;
use std::path::Path;

use tracing::{debug, warn};

/// Result of parsing a document.
#[derive(Debug, Clone)]
pub struct ParseResult {
    /// Whether parsing succeeded.
    pub success: bool,
    /// Extracted plain text.
    pub text: String,
    /// Metadata about the document.
    pub metadata: HashMap<String, String>,
    /// Error message if parsing failed.
    pub error: Option<String>,
}

impl ParseResult {
    fn ok(text: String, metadata: HashMap<String, String>) -> Self {
        Self {
            success: true,
            text,
            metadata,
            error: None,
        }
    }

    fn err(msg: impl Into<String>) -> Self {
        Self {
            success: false,
            text: String::new(),
            metadata: HashMap::new(),
            error: Some(msg.into()),
        }
    }
}

/// Supported file extensions for parsing.
const TEXT_EXTENSIONS: &[&str] = &[".txt", ".md", ".rst", ".tex"];
const CODE_EXTENSIONS: &[&str] = &[
    ".py", ".js", ".ts", ".java", ".c", ".cpp", ".h", ".css", ".html", ".rs",
];
const STRUCTURED_EXTENSIONS: &[&str] = &[".json", ".yaml", ".yml", ".xml", ".toml"];

/// Multi-format document parser.
pub struct DocumentParser;

impl DocumentParser {
    /// Parse a document at the given path into plain text.
    pub fn parse(path: &Path) -> ParseResult {
        if !path.exists() {
            return ParseResult::err("File not found");
        }

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| format!(".{}", e.to_lowercase()))
            .unwrap_or_default();

        debug!("Parsing document: {} (ext: {})", path.display(), ext);

        match ext.as_str() {
            ".pdf" => Self::parse_pdf(path),
            ".docx" => Self::parse_docx(path),
            ".csv" => Self::parse_csv(path),
            e if TEXT_EXTENSIONS.contains(&e) => Self::parse_text(path),
            e if CODE_EXTENSIONS.contains(&e) => Self::parse_code(path),
            e if STRUCTURED_EXTENSIONS.contains(&e) => Self::parse_structured(path),
            _ => {
                // Fallback: try as plain text.
                Self::parse_text(path)
            }
        }
    }

    /// Parse a plain text file.
    fn parse_text(path: &Path) -> ParseResult {
        match fs::read_to_string(path) {
            Ok(text) => {
                let mut meta = HashMap::new();
                meta.insert("type".into(), "text".into());
                Self::add_file_meta(&mut meta, path);
                ParseResult::ok(text, meta)
            }
            Err(e) => {
                // Try with lossy encoding.
                match fs::read(path) {
                    Ok(bytes) => {
                        let text = String::from_utf8_lossy(&bytes).to_string();
                        let mut meta = HashMap::new();
                        meta.insert("type".into(), "text".into());
                        meta.insert("encoding".into(), "lossy".into());
                        Self::add_file_meta(&mut meta, path);
                        ParseResult::ok(text, meta)
                    }
                    Err(_) => ParseResult::err(format!("Failed to read file: {e}")),
                }
            }
        }
    }

    /// Parse a code file with language detection.
    fn parse_code(path: &Path) -> ParseResult {
        let text = match fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => return ParseResult::err(format!("Failed to read code file: {e}")),
        };

        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");

        let language = match ext {
            "py" => "python",
            "js" => "javascript",
            "ts" => "typescript",
            "java" => "java",
            "c" => "c",
            "cpp" => "cpp",
            "h" => "c",
            "css" => "css",
            "html" => "html",
            "rs" => "rust",
            _ => "unknown",
        };

        let line_count = text.lines().count();
        let mut meta = HashMap::new();
        meta.insert("type".into(), "code".into());
        meta.insert("language".into(), language.into());
        meta.insert("line_count".into(), line_count.to_string());
        Self::add_file_meta(&mut meta, path);

        ParseResult::ok(text, meta)
    }

    /// Parse structured data (JSON, YAML, XML, TOML).
    fn parse_structured(path: &Path) -> ParseResult {
        match fs::read_to_string(path) {
            Ok(text) => {
                let mut meta = HashMap::new();
                meta.insert("type".into(), "structured".into());
                Self::add_file_meta(&mut meta, path);
                ParseResult::ok(text, meta)
            }
            Err(e) => ParseResult::err(format!("Failed to read structured file: {e}")),
        }
    }

    /// Parse a CSV file.
    fn parse_csv(path: &Path) -> ParseResult {
        match fs::read_to_string(path) {
            Ok(text) => {
                let row_count = text.lines().count();
                let mut meta = HashMap::new();
                meta.insert("type".into(), "csv".into());
                meta.insert("row_count".into(), row_count.to_string());
                Self::add_file_meta(&mut meta, path);
                ParseResult::ok(text, meta)
            }
            Err(e) => ParseResult::err(format!("Failed to read CSV: {e}")),
        }
    }

    /// Parse a PDF file using the `pdf-extract` crate.
    fn parse_pdf(path: &Path) -> ParseResult {
        match pdf_extract::extract_text(path) {
            Ok(text) => {
                let mut meta = HashMap::new();
                meta.insert("type".into(), "pdf".into());
                Self::add_file_meta(&mut meta, path);
                ParseResult::ok(text, meta)
            }
            Err(e) => {
                warn!("PDF extraction failed for {}: {e}", path.display());
                ParseResult::err(format!("PDF extraction failed: {e}"))
            }
        }
    }

    /// Parse a DOCX file by extracting text from the ZIP archive.
    ///
    /// DOCX is a ZIP file containing XML. We read `word/document.xml`
    /// and strip XML tags to get the plain text content.
    fn parse_docx(path: &Path) -> ParseResult {
        let file = match fs::File::open(path) {
            Ok(f) => f,
            Err(e) => return ParseResult::err(format!("Cannot open DOCX: {e}")),
        };

        let mut archive = match zip::ZipArchive::new(file) {
            Ok(a) => a,
            Err(e) => return ParseResult::err(format!("Invalid DOCX (not a ZIP): {e}")),
        };

        // Read word/document.xml — the main content file.
        let mut xml_content = String::new();
        match archive.by_name("word/document.xml") {
            Ok(mut entry) => {
                if let Err(e) = entry.read_to_string(&mut xml_content) {
                    return ParseResult::err(format!("Failed to read document.xml: {e}"));
                }
            }
            Err(e) => return ParseResult::err(format!("No document.xml in DOCX: {e}")),
        }

        // Extract text: split on </w:p> for paragraph breaks, strip all XML tags.
        let text = extract_text_from_docx_xml(&xml_content);

        let paragraph_count = text.matches("\n\n").count() + 1;
        let mut meta = HashMap::new();
        meta.insert("type".into(), "docx".into());
        meta.insert("paragraph_count".into(), paragraph_count.to_string());
        Self::add_file_meta(&mut meta, path);

        ParseResult::ok(text, meta)
    }

    /// Add common file metadata.
    fn add_file_meta(meta: &mut HashMap<String, String>, path: &Path) {
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            meta.insert("filename".into(), name.into());
        }
        if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
            meta.insert("extension".into(), format!(".{ext}"));
        }
        if let Ok(file_meta) = fs::metadata(path) {
            meta.insert("size".into(), file_meta.len().to_string());
        }
    }
}

/// Extract plain text from DOCX XML content.
///
/// Splits on paragraph end tags (`</w:p>`) for paragraph breaks,
/// then strips all remaining XML tags.
fn extract_text_from_docx_xml(xml: &str) -> String {
    // Replace paragraph end markers with double newlines.
    let with_breaks = xml.replace("</w:p>", "\n\n");

    // Strip all XML tags.
    let mut result = String::with_capacity(with_breaks.len() / 2);
    let mut in_tag = false;
    for ch in with_breaks.chars() {
        match ch {
            '<' => in_tag = true,
            '>' => in_tag = false,
            _ if !in_tag => result.push(ch),
            _ => {}
        }
    }

    // Clean up: collapse multiple newlines, trim.
    let mut cleaned = String::new();
    let mut prev_empty = false;
    for line in result.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            if !prev_empty {
                cleaned.push('\n');
                prev_empty = true;
            }
        } else {
            if prev_empty && !cleaned.is_empty() {
                cleaned.push('\n');
            }
            cleaned.push_str(trimmed);
            cleaned.push('\n');
            prev_empty = false;
        }
    }

    cleaned.trim().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn test_parse_nonexistent() {
        let result = DocumentParser::parse(Path::new("/nonexistent/file.txt"));
        assert!(!result.success);
        assert!(result.error.unwrap().contains("not found"));
    }

    #[test]
    fn test_parse_text_file() {
        let dir = std::env::temp_dir().join("titan_parser_test");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("test.txt");
        fs::write(&path, "Hello world\nSecond line").unwrap();

        let result = DocumentParser::parse(&path);
        assert!(result.success);
        assert!(result.text.contains("Hello world"));
        assert_eq!(result.metadata.get("type").unwrap(), "text");
        assert_eq!(result.metadata.get("filename").unwrap(), "test.txt");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_parse_code_file() {
        let dir = std::env::temp_dir().join("titan_parser_code");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("example.py");
        fs::write(&path, "def hello():\n    print('hi')\n").unwrap();

        let result = DocumentParser::parse(&path);
        assert!(result.success);
        assert_eq!(result.metadata.get("language").unwrap(), "python");
        assert_eq!(result.metadata.get("type").unwrap(), "code");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_parse_csv_file() {
        let dir = std::env::temp_dir().join("titan_parser_csv");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("data.csv");
        fs::write(&path, "name,age\nAlice,30\nBob,25\n").unwrap();

        let result = DocumentParser::parse(&path);
        assert!(result.success);
        assert_eq!(result.metadata.get("type").unwrap(), "csv");
        assert_eq!(result.metadata.get("row_count").unwrap(), "3");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_extract_docx_xml() {
        let xml = r#"<w:p><w:r><w:t>Hello</w:t></w:r></w:p><w:p><w:r><w:t>World</w:t></w:r></w:p>"#;
        let text = extract_text_from_docx_xml(xml);
        assert!(text.contains("Hello"));
        assert!(text.contains("World"));
    }

    #[test]
    fn test_parse_structured_json() {
        let dir = std::env::temp_dir().join("titan_parser_json");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("config.json");
        fs::write(&path, r#"{"key": "value"}"#).unwrap();

        let result = DocumentParser::parse(&path);
        assert!(result.success);
        assert_eq!(result.metadata.get("type").unwrap(), "structured");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_parse_docx_invalid() {
        let dir = std::env::temp_dir().join("titan_parser_docx_bad");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("bad.docx");
        fs::write(&path, "not a real docx").unwrap();

        let result = DocumentParser::parse(&path);
        assert!(!result.success);
        assert!(result.error.unwrap().contains("not a ZIP"));

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_parse_docx_valid() {
        // Create a minimal valid DOCX (ZIP with word/document.xml).
        let dir = std::env::temp_dir().join("titan_parser_docx_ok");
        let _ = fs::create_dir_all(&dir);
        let path = dir.join("test.docx");

        let file = fs::File::create(&path).unwrap();
        let mut zip = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("word/document.xml", options).unwrap();
        zip.write_all(
            br#"<?xml version="1.0"?><w:document><w:body><w:p><w:r><w:t>Test content</w:t></w:r></w:p></w:body></w:document>"#,
        ).unwrap();
        zip.finish().unwrap();

        let result = DocumentParser::parse(&path);
        assert!(result.success);
        assert!(result.text.contains("Test content"));
        assert_eq!(result.metadata.get("type").unwrap(), "docx");

        let _ = fs::remove_dir_all(&dir);
    }
}
