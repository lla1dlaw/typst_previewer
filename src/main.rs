#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod compiler;
mod neovim;
mod renderer;
mod viewport;

use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc;
use winit::application::ApplicationHandler;
use winit::event::{ElementState, KeyEvent, MouseButton, MouseScrollDelta, WindowEvent};
use winit::event_loop::{ActiveEventLoop, ControlFlow, EventLoop};
use winit::keyboard::{Key, NamedKey};
use winit::window::{Window, WindowId};

use crate::compiler::TypstCompiler;
use crate::neovim::{NvimMessage, OutgoingMessage};
use crate::renderer::Renderer;
use crate::viewport::Viewport;

const DEBOUNCE_DURATION: Duration = Duration::from_millis(300);

#[derive(Debug)]
pub enum AppEvent {
    Nvim(NvimMessage),
}

struct App {
    window: Option<Arc<Window>>,
    renderer: Option<Renderer>,
    compiler: TypstCompiler,
    viewport: Viewport,
    pending_recompile: bool,
    last_edit_time: Instant,
    initial_compiled: bool,
    cursor_pos: (f32, f32),
    tx_outgoing: mpsc::UnboundedSender<OutgoingMessage>,
}

impl App {
    fn new(tx_outgoing: mpsc::UnboundedSender<OutgoingMessage>) -> Self {
        Self {
            window: None,
            renderer: None,
            compiler: TypstCompiler::new(),
            viewport: Viewport::new(),
            pending_recompile: false,
            last_edit_time: Instant::now(),
            initial_compiled: false,
            cursor_pos: (0.0, 0.0),
            tx_outgoing,
        }
    }

    fn recompile_and_update(&mut self) {
        if let Some(pages) = self.compiler.compile_and_render() {
            self.viewport.set_pages(&pages);
            if let Some(renderer) = &mut self.renderer {
                renderer.update_page_textures(&pages);
            }
        }
        self.pending_recompile = false;
        if let Some(window) = &self.window {
            window.request_redraw();
        }
    }
}

impl ApplicationHandler<AppEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.window.is_some() {
            return;
        }

        let attrs = Window::default_attributes()
            .with_title("Typst Preview")
            .with_inner_size(winit::dpi::LogicalSize::new(900.0, 1100.0));

        let window = Arc::new(event_loop.create_window(attrs).expect("Failed to create preview window"));
        let size = window.inner_size();
        self.viewport.set_window_size(size.width, size.height);

        let renderer = pollster::block_on(Renderer::new(window.clone()));
        self.renderer = Some(renderer);
        self.window = Some(window);

        // If content was received before window creation, compile it now
        if !self.initial_compiled && !self.compiler.get_source().is_empty() {
            self.initial_compiled = true;
            self.recompile_and_update();
        }
    }

    fn user_event(&mut self, _event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::Nvim(NvimMessage::InitBuffer { content, filepath }) => {
                self.compiler.set_source(content, filepath);
                if self.window.is_some() {
                    self.initial_compiled = true;
                    self.recompile_and_update();
                }
            }
            AppEvent::Nvim(NvimMessage::BufferLines {
                firstline,
                lastline,
                lines,
            }) => {
                self.compiler.apply_delta(firstline, lastline, &lines);
                self.pending_recompile = true;
                self.last_edit_time = Instant::now();
            }
        }
    }

    fn window_event(&mut self, event_loop: &ActiveEventLoop, _id: WindowId, event: WindowEvent) {
        match event {
            WindowEvent::CloseRequested => {
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.resize(size.width, size.height);
                }
                self.viewport.set_window_size(size.width, size.height);
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::CursorMoved { position, .. } => {
                self.cursor_pos = (position.x as f32, position.y as f32);
            }
            WindowEvent::MouseInput {
                state: ElementState::Pressed,
                button: MouseButton::Left,
                ..
            } => {
                // Click-to-source jump
                if let Some((page_idx, x_pt, y_pt)) =
                    self.viewport.window_to_page_coords(self.cursor_pos.0, self.cursor_pos.1)
                {
                    if let Some((filepath, line, col)) =
                        self.compiler.click_to_source(page_idx, x_pt, y_pt)
                    {
                        let _ = self.tx_outgoing.send(OutgoingMessage::JumpTo {
                            filepath,
                            line,
                            col,
                        });
                    }
                }
            }
            WindowEvent::MouseWheel { delta, .. } => {
                self.viewport.handle_scroll(delta);
                if let Some(window) = &self.window {
                    window.request_redraw();
                }
            }
            WindowEvent::KeyboardInput {
                event:
                    KeyEvent {
                        state: ElementState::Pressed,
                        logical_key,
                        ..
                    },
                ..
            } => match logical_key {
                Key::Character(c) if c == "+" || c == "=" => {
                    self.viewport.handle_zoom(1.1);
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
                Key::Character(c) if c == "-" || c == "_" => {
                    self.viewport.handle_zoom(0.9);
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
                Key::Character(c) if c == "0" => {
                    self.viewport.zoom = 1.0;
                    self.viewport.clamp_scroll();
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
                Key::Named(NamedKey::ArrowUp) => {
                    self.viewport
                        .handle_scroll(MouseScrollDelta::LineDelta(0.0, 1.0));
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
                Key::Named(NamedKey::ArrowDown) => {
                    self.viewport
                        .handle_scroll(MouseScrollDelta::LineDelta(0.0, -1.0));
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
                Key::Named(NamedKey::PageUp) => {
                    self.viewport
                        .handle_scroll(MouseScrollDelta::LineDelta(0.0, 10.0));
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
                Key::Named(NamedKey::PageDown) => {
                    self.viewport
                        .handle_scroll(MouseScrollDelta::LineDelta(0.0, -10.0));
                    if let Some(window) = &self.window {
                        window.request_redraw();
                    }
                }
                _ => {}
            },
            WindowEvent::RedrawRequested => {
                if let Some(renderer) = &mut self.renderer {
                    renderer.render(&self.viewport);
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.pending_recompile {
            let elapsed = self.last_edit_time.elapsed();
            if elapsed >= DEBOUNCE_DURATION {
                self.recompile_and_update();
            } else {
                let remaining = DEBOUNCE_DURATION - elapsed;
                event_loop.set_control_flow(ControlFlow::WaitUntil(Instant::now() + remaining));
                return;
            }
        }
        event_loop.set_control_flow(ControlFlow::Wait);
    }
}

fn main() {
    env_logger::init();

    let event_loop = EventLoop::<AppEvent>::with_user_event()
        .build()
        .expect("Failed to build event loop");

    let proxy = event_loop.create_proxy();
    let (tx_outgoing, rx_outgoing) = mpsc::unbounded_channel::<OutgoingMessage>();

    // Start background Tokio runtime for Neovim MessagePack-RPC
    std::thread::spawn(move || {
        let rt = tokio::runtime::Runtime::new().expect("Failed to create Tokio runtime");
        let (tx_incoming, mut rx_incoming) = mpsc::unbounded_channel::<NvimMessage>();

        // Forward incoming messages from mpsc channel to winit EventLoopProxy
        let proxy_clone = proxy.clone();
        rt.spawn(async move {
            while let Some(msg) = rx_incoming.recv().await {
                let _ = proxy_clone.send_event(AppEvent::Nvim(msg));
            }
        });

        rt.block_on(async {
            neovim::run_rpc(tx_incoming, rx_outgoing).await;
        });

        // When Neovim exits and closes stdio, exit the previewer process
        std::process::exit(0);
    });

    let mut app = App::new(tx_outgoing);
    event_loop.run_app(&mut app).expect("Event loop error");
}
