use winit::event::MouseScrollDelta;
use crate::compiler::RenderedPage;

const PAGE_GAP_PX: f32 = 24.0;
const PADDING_TOP_PX: f32 = 24.0;
const PADDING_BOTTOM_PX: f32 = 48.0;

#[derive(Debug, Clone)]
pub struct PageLayout {
    pub index: usize,
    /// Top-left NDC coordinates (x, y) and NDC dimensions (width, height)
    /// rect = [ndc_x, ndc_y, ndc_w, ndc_h]
    pub ndc_rect: [f32; 4],
}

pub struct Viewport {
    pub scroll_y: f32,
    pub zoom: f32,
    pub window_width: u32,
    pub window_height: u32,
    /// Base dimensions of rendered pages (width, height)
    page_sizes: Vec<(f32, f32)>,
    /// Precomputed Y start position of each page in unscaled document coordinates
    page_y_offsets: Vec<f32>,
    total_document_height: f32,
}

impl Viewport {
    pub fn new() -> Self {
        Self {
            scroll_y: 0.0,
            zoom: 1.0,
            window_width: 800,
            window_height: 1000,
            page_sizes: Vec::new(),
            page_y_offsets: Vec::new(),
            total_document_height: 0.0,
        }
    }

    pub fn set_window_size(&mut self, width: u32, height: u32) {
        self.window_width = width.max(1);
        self.window_height = height.max(1);
        self.clamp_scroll();
    }

    /// Updates page layout when new pages are compiled.
    pub fn set_pages(&mut self, pages: &[RenderedPage]) {
        self.page_sizes.clear();
        self.page_y_offsets.clear();

        let mut current_y = PADDING_TOP_PX;
        for page in pages {
            // Rendered at 2.0x DPI; base CSS-equivalent points is width / 2.0
            let base_w = (page.width as f32) / 2.0;
            let base_h = (page.height as f32) / 2.0;

            self.page_sizes.push((base_w, base_h));
            self.page_y_offsets.push(current_y);
            current_y += base_h + PAGE_GAP_PX;
        }

        self.total_document_height = current_y + PADDING_BOTTOM_PX;
        self.clamp_scroll();
    }

    pub fn handle_scroll(&mut self, delta: MouseScrollDelta) {
        match delta {
            MouseScrollDelta::LineDelta(_, y) => {
                // Typical mouse wheel line is ~50px
                self.scroll_y -= y * 50.0;
            }
            MouseScrollDelta::PixelDelta(pos) => {
                self.scroll_y -= pos.y as f32;
            }
        }
        self.clamp_scroll();
    }

    pub fn handle_zoom(&mut self, factor: f32) {
        let old_zoom = self.zoom;
        self.zoom = (self.zoom * factor).clamp(0.25, 4.0);

        // Adjust scroll to keep the viewport approximately centered
        if old_zoom > 0.0 {
            self.scroll_y = (self.scroll_y / old_zoom) * self.zoom;
        }
        self.clamp_scroll();
    }

    pub fn clamp_scroll(&mut self) {
        let scaled_total_height = self.total_document_height * self.zoom;
        let max_scroll = (scaled_total_height - self.window_height as f32).max(0.0);
        self.scroll_y = self.scroll_y.clamp(0.0, max_scroll);
    }

    /// Computes which pages are currently visible and their NDC rects for shader rendering.
    pub fn compute_visible_page_layouts(&self) -> Vec<PageLayout> {
        let win_w = self.window_width as f32;
        let win_h = self.window_height as f32;
        let mut layouts = Vec::new();

        let viewport_top = self.scroll_y;
        let viewport_bottom = self.scroll_y + win_h;

        for (i, &(base_w, base_h)) in self.page_sizes.iter().enumerate() {
            let page_top_px = self.page_y_offsets[i] * self.zoom;
            let page_h_px = base_h * self.zoom;
            let page_bottom_px = page_top_px + page_h_px;

            // Check if page intersects the visible window viewport
            if page_bottom_px < viewport_top || page_top_px > viewport_bottom {
                continue;
            }

            let page_w_px = base_w * self.zoom;
            // Center horizontally
            let page_x_px = ((win_w - page_w_px) / 2.0).max(0.0);
            let page_y_in_window = page_top_px - self.scroll_y;

            // Convert pixel coordinates to NDC [-1.0, 1.0]
            let ndc_x = (page_x_px / win_w) * 2.0 - 1.0;
            let ndc_y = 1.0 - (page_y_in_window / win_h) * 2.0;
            let ndc_w = (page_w_px / win_w) * 2.0;
            let ndc_h = (page_h_px / win_h) * 2.0;

            layouts.push(PageLayout {
                index: i,
                ndc_rect: [ndc_x, ndc_y, ndc_w, ndc_h],
            });
        }

        layouts
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_viewport_scroll_clamp() {
        let mut vp = Viewport::new();
        vp.set_window_size(800, 600);

        let pages = vec![
            RenderedPage { width: 1000, height: 1600, pixels: vec![] },
        ];
        vp.set_pages(&pages);

        // Document height should be > 600
        assert!(vp.total_document_height > 600.0);

        // Scroll cannot be negative
        vp.handle_scroll(MouseScrollDelta::LineDelta(0.0, 10.0));
        assert_eq!(vp.scroll_y, 0.0);

        // Scroll cannot exceed document bottom
        vp.handle_scroll(MouseScrollDelta::LineDelta(0.0, -1000.0));
        let max_scroll = vp.total_document_height * vp.zoom - 600.0;
        assert!((vp.scroll_y - max_scroll).abs() < 0.001);
    }
}
