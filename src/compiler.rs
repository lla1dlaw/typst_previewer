use chrono::Datelike;
use ropey::Rope;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::syntax::{FileId, Source};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World};
use typst_layout::PagedDocument;
use typst_render::{render, RenderOptions};

/// A single rendered page containing raw RGBA pixel data and dimensions.
#[derive(Clone, Debug)]
pub struct RenderedPage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

pub struct TypstCompiler {
    rope: Rope,
    filepath: PathBuf,
    library: Arc<LazyHash<Library>>,
    fontbook: Arc<LazyHash<FontBook>>,
    fonts: Arc<Vec<Font>>,
}

impl TypstCompiler {
    pub fn new() -> Self {
        let (fontbook, fonts) = Self::load_fonts();
        Self {
            rope: Rope::new(),
            filepath: PathBuf::from("main.typ"),
            library: Arc::new(LazyHash::new(Library::default())),
            fontbook: Arc::new(LazyHash::new(fontbook)),
            fonts: Arc::new(fonts),
        }
    }

    fn load_fonts() -> (FontBook, Vec<Font>) {
        let mut book = FontBook::new();
        let mut fonts = Vec::new();

        for data in typst_assets::fonts() {
            let buffer = Bytes::new(data);
            for font in Font::iter(buffer) {
                book.push(font.info().clone());
                fonts.push(font);
            }
        }

        (book, fonts)
    }

    /// Sets the entire source text and the file path.
    pub fn set_source(&mut self, content: String, filepath: String) {
        self.rope = Rope::from_str(&content);
        if !filepath.is_empty() {
            self.filepath = PathBuf::from(filepath);
        }
    }

    /// Gets the current source text as a String.
    pub fn get_source(&self) -> String {
        self.rope.to_string()
    }

    /// Applies an incremental line delta from Neovim's on_lines event.
    ///
    /// `firstline`: 0-indexed line where replacement begins
    /// `lastline`: 0-indexed line where replacement ends (exclusive)
    /// `new_lines`: slice of new line contents to insert
    pub fn apply_delta(&mut self, firstline: usize, lastline: usize, new_lines: &[String]) {
        let total_lines = self.rope.len_lines();

        let start_char = if firstline < total_lines {
            self.rope.line_to_char(firstline)
        } else {
            self.rope.len_chars()
        };

        let end_char = if lastline < total_lines {
            self.rope.line_to_char(lastline)
        } else {
            self.rope.len_chars()
        };

        // Remove the replaced line range
        if start_char < end_char {
            self.rope.remove(start_char..end_char);
        }

        // Insert new lines (if any)
        if !new_lines.is_empty() {
            let mut replacement = new_lines.join("\n");
            // If replacing within lines, preserve trailing newline
            replacement.push('\n');
            self.rope.insert(start_char, &replacement);
        }
    }

    /// Compiles the current document and renders all pages to raster RGBA pixel buffers.
    pub fn compile_and_render(&self) -> Option<Vec<RenderedPage>> {
        let source_text = self.rope.to_string();
        if source_text.trim().is_empty() {
            return Some(Vec::new());
        }

        let world = PreviewWorld {
            source: Source::detached(&source_text),
            library: &self.library,
            fontbook: &self.fontbook,
            fonts: &self.fonts,
            filepath: &self.filepath,
        };

        let result = typst::compile::<PagedDocument>(&world);
        let document = match result.output {
            Ok(doc) => doc,
            Err(diagnostics) => {
                for diag in &diagnostics {
                    eprintln!("[typst] compilation warning/error: {:?}", diag.message);
                }
                return None;
            }
        };

        let opts = RenderOptions::default();
        let pages: Vec<RenderedPage> = document
            .pages()
            .iter()
            .map(|page| {
                let pixmap = render(page, &opts);
                RenderedPage {
                    width: pixmap.width(),
                    height: pixmap.height(),
                    pixels: pixmap.take(),
                }
            })
            .collect();

        Some(pages)
    }
}

/// Minimal Typst World for the live previewer.
struct PreviewWorld<'a> {
    source: Source,
    library: &'a LazyHash<Library>,
    fontbook: &'a LazyHash<FontBook>,
    fonts: &'a [Font],
    filepath: &'a Path,
}

impl World for PreviewWorld<'_> {
    fn library(&self) -> &LazyHash<Library> {
        self.library
    }

    fn book(&self) -> &LazyHash<FontBook> {
        self.fontbook
    }

    fn main(&self) -> FileId {
        self.source.id()
    }

    fn source(&self, id: FileId) -> FileResult<Source> {
        if id == self.source.id() {
            Ok(self.source.clone())
        } else {
            let base = self.filepath.parent().unwrap_or(Path::new("."));
            let path = base.join(id.vpath().get_without_slash());
            let text = std::fs::read_to_string(&path)
                .map_err(|_| FileError::NotFound(path.clone()))?;
            Ok(Source::new(id, text))
        }
    }

    fn file(&self, id: FileId) -> FileResult<Bytes> {
        let base = self.filepath.parent().unwrap_or(Path::new("."));
        let path = base.join(id.vpath().get_without_slash());
        let data = std::fs::read(&path)
            .map_err(|_| FileError::NotFound(path))?;
        Ok(Bytes::new(data))
    }

    fn font(&self, index: usize) -> Option<Font> {
        self.fonts.get(index).cloned()
    }

    fn today(&self, _offset: Option<Duration>) -> Option<Datetime> {
        let now = chrono::Local::now();
        Datetime::from_ymd(now.year(), now.month() as u8, now.day() as u8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_delta_updates() {
        let mut compiler = TypstCompiler::new();
        compiler.set_source("line 1\nline 2\nline 3\n".to_string(), "test.typ".to_string());

        // Replace line 1 (0-indexed: index 1..2 is "line 2")
        compiler.apply_delta(1, 2, &["replacement line".to_string()]);
        assert_eq!(compiler.get_source(), "line 1\nreplacement line\nline 3\n");
    }

    #[test]
    fn test_compilation_and_rendering() {
        let mut compiler = TypstCompiler::new();
        compiler.set_source("= Test Heading\n\nHello from unit test!".to_string(), "test.typ".to_string());

        let pages = compiler.compile_and_render();
        assert!(pages.is_some());
        let pages = pages.unwrap();
        assert_eq!(pages.len(), 1);
        assert!(pages[0].width > 0);
        assert!(pages[0].height > 0);
        assert_eq!(pages[0].pixels.len(), (pages[0].width * pages[0].height * 4) as usize);
    }
}
