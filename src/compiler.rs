use chrono::Datelike;
use ropey::Rope;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use typst::diag::{FileError, FileResult};
use typst::foundations::{Bytes, Datetime, Duration};
use typst::syntax::{FileId, Source, Span};
use typst::text::{Font, FontBook};
use typst::utils::LazyHash;
use typst::{Library, LibraryExt, World, WorldExt};
use typst_layout::PagedDocument;
use typst_library::layout::{Abs, Frame, FrameItem, Point};
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
    document: Option<PagedDocument>,
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
            document: None,
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

    /// Gets the current file path.
    pub fn get_filepath(&self) -> String {
        self.filepath.to_string_lossy().to_string()
    }

    /// Applies an incremental line delta from Neovim's on_lines event.
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
            replacement.push('\n');
            self.rope.insert(start_char, &replacement);
        }
    }

    /// Compiles the current document and renders all pages to raster RGBA pixel buffers.
    pub fn compile_and_render(&mut self) -> Option<Vec<RenderedPage>> {
        let source_text = self.rope.to_string();
        if source_text.trim().is_empty() {
            self.document = None;
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

        self.document = Some(document);
        Some(pages)
    }

    /// Click-to-source jump: given page index and coordinates (in typographic points),
    /// finds the corresponding source location (file, 1-indexed line, 0-indexed column).
    pub fn click_to_source(&self, page_index: usize, x_pt: f32, y_pt: f32) -> Option<(String, usize, usize)> {
        let doc = self.document.as_ref()?;
        let page = doc.pages().get(page_index)?;

        let click_pt = Point::new(Abs::pt(x_pt as f64), Abs::pt(y_pt as f64));
        let mut best: Option<HitCandidate> = None;
        hit_test_frame(&page.frame, click_pt, &mut best);

        let candidate = best?;
        // Ignore clicks that are too far from any content (> 150 points)
        if candidate.dist_sq > 150.0 * 150.0 {
            return None;
        }

        let world = PreviewWorld {
            source: Source::detached(&self.rope.to_string()),
            library: &self.library,
            fontbook: &self.fontbook,
            fonts: &self.fonts,
            filepath: &self.filepath,
        };

        let range = world.range(candidate.span)?;
        let byte_offset = range.start.min(self.rope.len_bytes());
        let line = self.rope.byte_to_line(byte_offset);
        let line_start_byte = self.rope.line_to_byte(line);
        let col = byte_offset.saturating_sub(line_start_byte);

        Some((self.get_filepath(), line + 1, col))
    }
}

struct HitCandidate {
    span: Span,
    dist_sq: f64,
}

fn hit_test_frame(
    frame: &Frame,
    point: Point,
    best: &mut Option<HitCandidate>,
) {
    for (pos, item) in frame.items() {
        match item {
            FrameItem::Text(text) => {
                let mut current_x = pos.x;
                for glyph in &text.glyphs {
                    let glyph_w = glyph.x_advance.at(text.size);
                    let glyph_mid_x = current_x + glyph_w / 2.0;
                    let glyph_mid_y = pos.y - text.size / 2.0;

                    let dx = (point.x - glyph_mid_x).to_pt();
                    let dy = (point.y - glyph_mid_y).to_pt();
                    let dist_sq = dx * dx + dy * dy;

                    if best.as_ref().map_or(true, |b| dist_sq < b.dist_sq) {
                        if glyph.span.0.id().is_some() {
                            *best = Some(HitCandidate {
                                span: glyph.span.0,
                                dist_sq,
                            });
                        }
                    }
                    current_x += glyph_w;
                }
            }
            FrameItem::Shape(_, span) => {
                if span.id().is_some() {
                    let dx = (point.x - pos.x).to_pt();
                    let dy = (point.y - pos.y).to_pt();
                    let dist_sq = dx * dx + dy * dy;
                    if best.as_ref().map_or(true, |b| dist_sq < b.dist_sq) {
                        *best = Some(HitCandidate {
                            span: *span,
                            dist_sq,
                        });
                    }
                }
            }
            FrameItem::Image(_, size, span) => {
                if span.id().is_some() {
                    let mid_x = pos.x + size.x / 2.0;
                    let mid_y = pos.y + size.y / 2.0;
                    let dx = (point.x - mid_x).to_pt();
                    let dy = (point.y - mid_y).to_pt();
                    let dist_sq = dx * dx + dy * dy;
                    if best.as_ref().map_or(true, |b| dist_sq < b.dist_sq) {
                        *best = Some(HitCandidate {
                            span: *span,
                            dist_sq,
                        });
                    }
                }
            }
            FrameItem::Group(group) => {
                let local_point = point - *pos;
                let inv = group.transform.invert().unwrap_or_default();
                let child_point = local_point.transform(inv);
                hit_test_frame(&group.frame, child_point, best);
            }
            _ => {}
        }
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

    #[test]
    fn test_click_to_source_jump() {
        let mut compiler = TypstCompiler::new();
        let content = "= Heading\n\nFirst paragraph content here.\n\nSecond paragraph content here.";
        compiler.set_source(content.to_string(), "doc.typ".to_string());

        let pages = compiler.compile_and_render();
        assert!(pages.is_some());

        // In a typical A4/Letter Typst page, margins are ~72pt (1 inch).
        // The top heading is near x = 72..200, y = 72..100.
        let hit = compiler.click_to_source(0, 80.0, 80.0);
        assert!(hit.is_some());
        let (file, line, col) = hit.unwrap();
        assert_eq!(file, "doc.typ");
        // Line should be line 1 (the heading)
        assert_eq!(line, 1);
        assert!(col <= 9);
    }
}
