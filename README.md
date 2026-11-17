# typst-previewer

A real-time, native GPU-accelerated Typst document previewer for Neovim written in Rust.

Unlike browser-based or webview previewers (Tauri / Electron), `typst-previewer` bypasses HTML/DOM and WebSockets entirely. It renders Typst documents directly to raster bitmaps in-memory using `typst-render` and uploads them to a native `winit` + `wgpu` window in milliseconds.

```
Neovim buffer edit (stdio msgpack-RPC)
  -> ropey (O(log N) line delta patching)
  -> typst::compile (incremental memoized compile)
  -> typst-render (tiny-skia rasterization)
  -> wgpu (GPU texture upload & quad presentation)
```

## Features

- **Blazing Fast**: Sub-10ms render latency after debounce. No webview, no JavaScript, no IPC serialization.
- **Cross-Platform**: Works natively on Linux (Wayland & X11), macOS (Metal), and Windows (DirectX 12 / Vulkan).
- **Self-Contained**: Embedded Typst default fonts (`typst-assets`) guarantees documents render out-of-the-box.
- **Incremental Line Updates**: Listens to Neovim's `nvim_buf_attach` `on_lines` events over stdio RPC.
- **Multi-Page Virtualization**: Smooth scrolling with scroll bounds clamping and automatic horizontal page centering.
- **Keyboard Zoom & Navigation**:
  - `+` / `=` : Zoom in
  - `-` / `_` : Zoom out
  - `0` : Reset zoom to 100%
  - `↑` / `↓` / `PageUp` / `PageDown` : Scroll
  - Mouse Wheel : Smooth scroll

## Installation

### 1. Build the Binary

Prerequisites: Rust toolchain (1.80+) and Cargo.

```bash
git clone https://github.com/lla1dlaw/typst_previewer.git
cd typst_previewer
cargo build --release
```

### 2. Neovim Plugin Setup

Using [lazy.nvim](https://github.com/folke/lazy.nvim):

```lua
{
  "lla1dlaw/typst_previewer",
  build = "cargo build --release",
  ft = "typst",
  config = function()
    -- Optional keymaps:
    vim.keymap.set("n", "<leader>tp", "<cmd>TypstPreviewToggle<CR>", { desc = "Toggle Typst Preview" })
  end,
}
```

## Commands

| Command | Description |
|---|---|
| `:TypstPreview` | Open the preview window for the current buffer |
| `:TypstPreviewClose` | Close the preview window |
| `:TypstPreviewToggle` | Toggle the preview window |

## Architecture

- **Neovim Bridge (`lua/typst_previewer/init.lua`)**: Spawns the compiled binary as a child process via `vim.fn.jobstart({ rpc = true })` over standard I/O pipes. When Neovim exits, the child process closes automatically.
- **RPC Handler (`src/neovim.rs`)**: Async MessagePack-RPC listener using `nvim-rs` running on a background Tokio runtime. Dispatches deltas to the main GUI thread via `winit::event_loop::EventLoopProxy`.
- **Compiler (`src/compiler.rs`)**: Embeds `typst` compiler and `typst-assets` fonts implementing the `typst::World` trait. Maintains text in a `ropey::Rope` for fast incremental patching.
- **Renderer (`src/renderer.rs`)**: High-performance `wgpu` 24 rendering pipeline. Draws page quads using custom WGSL shaders with linear sampling and automatic DPI scaling.
- **Viewport (`src/viewport.rs`)**: Tracks window dimensions, virtual document height, scroll clamping, and NDC coordinate transforms.

## License

MIT / Apache 2.0
