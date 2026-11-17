# typst-previewer

A real-time, native GPU-accelerated Typst document previewer for Neovim written in Rust.

Unlike browser-based or webview previewers (Tauri / Electron), `typst-previewer` bypasses HTML/DOM and WebSockets entirely. It renders Typst documents directly to raster bitmaps in-memory using `typst-render` and uploads them to a native `winit` + `wgpu` window in milliseconds.

```
Neovim buffer edit (stdio msgpack-RPC)
  -> ropey (O(log N) line delta patching)
  -> typst::compile (incremental memoized compile)
  -> typst-render (tiny-skia direct rasterization)
  -> wgpu 24 (GPU texture upload & quad presentation)
  <-- Click-to-source jump (stdio msgpack-RPC to Neovim cursor)
```

## Features

- **Blazing Fast**: Sub-10ms render latency after debounce. Zero webview, zero JavaScript, zero IPC serialization.
- **Click-to-Source Jump**: Click anywhere on a page in the preview window to immediately jump Neovim's cursor to that exact line and column in the source document.
- **Cross-Platform**: Works natively on Linux (Wayland & X11), macOS (Metal), and Windows (DirectX 12 / Vulkan).
- **Self-Contained**: Embedded standard Typst fonts (`typst-assets`) guarantees documents render out-of-the-box without system font setup.
- **Incremental Line Updates**: Listens to Neovim's `nvim_buf_attach` `on_lines` events over stdio RPC.
- **Multi-Page Layout**: Smooth scrolling with scroll bounds clamping and automatic horizontal page centering.
- **Keyboard Zoom & Navigation**:
  - `+` / `=` : Zoom in
  - `-` / `_` : Zoom out
  - `0` : Reset zoom to 100%
  - `↑` / `↓` / `PageUp` / `PageDown` : Scroll
  - Left Mouse Click : Jump to source in Neovim
  - Mouse Wheel : Smooth scroll

## Installation

### Using [lazy.nvim](https://github.com/folke/lazy.nvim)

Add this to your lazy plugins (e.g. in `lua/plugins/typst.lua`):

```lua
{
  "lla1dlaw/typst_previewer",
  build = "cargo build --release",
  ft = "typst",
  opts = {
    keymaps = {
      toggle = "<leader>tp", -- Keymap to toggle preview
      open = "<leader>to",   -- Keymap to open preview
      close = "<leader>tc",  -- Keymap to close preview
    },
  },
}
```

Lazy.nvim will automatically clone the repository and run `cargo build --release`.

### Manual Build

If you prefer building manually:

```bash
git clone https://github.com/lla1dlaw/typst_previewer.git
cd typst_previewer
cargo build --release
```

Then add the folder to your `runtimepath` or package manager.

## Configuration

Default options passed to `setup()`:

```lua
require("typst_previewer").setup({
  -- Optional path to the binary if installed in a custom location.
  -- By default, it auto-discovers:
  -- 1. plugin target/release/typst-previewer
  -- 2. plugin target/debug/typst-previewer
  -- 3. system PATH (typst-previewer)
  binary_path = nil,

  keymaps = {
    toggle = "<leader>tp",
    open = nil,
    close = nil,
  },
})
```

## Commands

| Command | Description |
|---|---|
| `:TypstPreview` | Open the preview window for the current buffer |
| `:TypstPreviewClose` | Close the preview window |
| `:TypstPreviewToggle` | Toggle the preview window |

## Architecture

- **Neovim Bridge (`lua/typst_previewer/init.lua`)**: Spawns the compiled binary as a child process via `vim.fn.jobstart({ rpc = true })` over standard I/O pipes. When Neovim exits, the child process closes automatically.
- **RPC Handler (`src/neovim.rs`)**: Async MessagePack-RPC listener and sender using `nvim-rs` running on a background Tokio runtime. Dispatches deltas to the main GUI thread via `winit::event_loop::EventLoopProxy`, and executes `exec_lua` on Neovim when click-to-source events occur.
- **Compiler (`src/compiler.rs`)**: Embeds `typst` compiler and `typst-assets` fonts implementing the `typst::World` trait. Performs frame hit-testing to map page `(x, y)` coordinates to syntax `Span`s and byte offsets.
- **Renderer (`src/renderer.rs`)**: High-performance `wgpu` 24 rendering pipeline. Draws page quads using custom WGSL shaders with linear sampling and automatic DPI scaling.
- **Viewport (`src/viewport.rs`)**: Tracks window dimensions, virtual document height, scroll clamping, NDC coordinate transforms, and window-to-page coordinate hit-testing.

## License

MIT / Apache 2.0
