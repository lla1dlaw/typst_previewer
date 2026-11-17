use async_trait::async_trait;
use nvim_rs::{create::tokio as create, Handler, Neovim};
use rmpv::Value;
use tokio::sync::mpsc;

/// Configuration options passed from Neovim Lua to the previewer.
#[derive(Debug, Clone)]
pub struct AppConfig {
    pub auto_resize: bool,
    pub max_width: Option<f32>,
    pub max_height: Option<f32>,
    pub scale_factor: Option<f32>,
    pub system_fonts: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            auto_resize: true,
            max_width: Some(1400.0),
            max_height: Some(1100.0),
            scale_factor: None,
            system_fonts: true,
        }
    }
}

/// Messages dispatched from the Neovim RPC reader to the main GUI thread.
#[derive(Debug)]
pub enum NvimMessage {
    /// Initial buffer content, file path, and configuration.
    InitBuffer {
        content: String,
        filepath: String,
        config: AppConfig,
    },
    /// Incremental delta line update from nvim_buf_attach.
    BufferLines {
        firstline: usize,
        lastline: usize,
        lines: Vec<String>,
    },
}

/// Outgoing messages from GUI thread to Neovim over RPC.
#[derive(Debug)]
pub enum OutgoingMessage {
    /// Request Neovim to move cursor to source location.
    JumpTo {
        filepath: String,
        line: usize,
        col: usize,
    },
    /// Compilation diagnostic message to notify user in Neovim.
    Diagnostic {
        message: String,
    },
}

#[derive(Clone)]
struct NvimHandler {
    tx: mpsc::UnboundedSender<NvimMessage>,
}

#[async_trait]
impl Handler for NvimHandler {
    type Writer = tokio_util::compat::Compat<tokio::fs::File>;

    async fn handle_notify(
        &self,
        name: String,
        args: Vec<Value>,
        _neovim: Neovim<Self::Writer>,
    ) {
        crate::logger::log(&format!("handle_notify: name={}, args len={}", name, args.len()));
        match name.as_str() {
            "init_buffer" => {
                if let Some(map) = args.first().and_then(|v| v.as_map()) {
                    let content = find_str(map, "content").unwrap_or_default().to_string();
                    let filepath = find_str(map, "filepath").unwrap_or_default().to_string();

                    let mut config = AppConfig::default();
                    if let Some(cfg_map) = find_map(map, "config") {
                        if let Some(v) = find_bool(cfg_map, "auto_resize") {
                            config.auto_resize = v;
                        }
                        if let Some(v) = find_f64(cfg_map, "max_width") {
                            config.max_width = Some(v as f32);
                        }
                        if let Some(v) = find_f64(cfg_map, "max_height") {
                            config.max_height = Some(v as f32);
                        }
                        if let Some(v) = find_f64(cfg_map, "scale_factor") {
                            config.scale_factor = Some(v as f32);
                        }
                        if let Some(v) = find_bool(cfg_map, "system_fonts") {
                            config.system_fonts = v;
                        }
                    }

                    let _ = self.tx.send(NvimMessage::InitBuffer {
                        content,
                        filepath,
                        config,
                    });
                }
            }
            "buffer_lines" => {
                if let Some(map) = args.first().and_then(|v| v.as_map()) {
                    let firstline = find_usize(map, "firstline").unwrap_or(0);
                    let lastline = find_usize(map, "lastline").unwrap_or(0);
                    let lines = find_string_array(map, "lines");

                    let _ = self.tx.send(NvimMessage::BufferLines {
                        firstline,
                        lastline,
                        lines,
                    });
                }
            }
            other => {
                eprintln!("[nvim-rpc] unhandled notification: {}", other);
            }
        }
    }
}

/// Runs the MessagePack-RPC event loop over stdio.
/// Blocks until Neovim closes the stdin pipe.
pub async fn run_rpc(
    tx_incoming: mpsc::UnboundedSender<NvimMessage>,
    mut rx_outgoing: mpsc::UnboundedReceiver<OutgoingMessage>,
) {
    let handler = NvimHandler { tx: tx_incoming };
    match create::new_parent(handler).await {
        Ok((neovim, io_handler)) => {
            let nvim_client = neovim.clone();

            // Spawn outgoing message writer task
            tokio::spawn(async move {
                while let Some(msg) = rx_outgoing.recv().await {
                    match msg {
                        OutgoingMessage::JumpTo { filepath, line, col } => {
                            let args = vec![
                                Value::from(filepath),
                                Value::from(line as u64),
                                Value::from(col as u64),
                            ];
                            let lua_code = "require('typst_previewer').jump_to(...)";
                            if let Err(e) = nvim_client.exec_lua(lua_code, args).await {
                                eprintln!("[nvim-rpc] jump_to exec_lua error: {:?}", e);
                            }
                        }
                        OutgoingMessage::Diagnostic { message } => {
                            let args = vec![Value::from(message)];
                            let lua_code = "vim.schedule(function() vim.notify('Typst: ' .. ..., vim.log.levels.WARN) end)";
                            let _ = nvim_client.exec_lua(lua_code, args).await;
                        }
                    }
                }
            });

            if let Err(e) = io_handler.await {
                eprintln!("[nvim-rpc] IO loop closed: {:?}", e);
            }
        }
        Err(e) => {
            eprintln!("[nvim-rpc] failed to initialize stdio RPC: {:?}", e);
        }
    }
}

// --- Msgpack parsing helpers ---

fn find_str<'a>(map: &'a [(Value, Value)], key: &str) -> Option<&'a str> {
    map.iter()
        .find(|(k, _)| k.as_str() == Some(key) || k.as_slice() == Some(key.as_bytes()))
        .and_then(|(_, v)| v.as_str().or_else(|| v.as_slice().and_then(|b| std::str::from_utf8(b).ok())))
}

fn find_bool(map: &[(Value, Value)], key: &str) -> Option<bool> {
    map.iter()
        .find(|(k, _)| k.as_str() == Some(key) || k.as_slice() == Some(key.as_bytes()))
        .and_then(|(_, v)| v.as_bool())
}

fn find_f64(map: &[(Value, Value)], key: &str) -> Option<f64> {
    map.iter()
        .find(|(k, _)| k.as_str() == Some(key) || k.as_slice() == Some(key.as_bytes()))
        .and_then(|(_, v)| v.as_f64().or_else(|| v.as_u64().map(|n| n as f64)).or_else(|| v.as_i64().map(|n| n as f64)))
}

fn find_map<'a>(map: &'a [(Value, Value)], key: &str) -> Option<&'a [(Value, Value)]> {
    map.iter()
        .find(|(k, _)| k.as_str() == Some(key) || k.as_slice() == Some(key.as_bytes()))
        .and_then(|(_, v)| v.as_map().map(|m| m.as_slice()))
}

fn find_usize(map: &[(Value, Value)], key: &str) -> Option<usize> {
    map.iter()
        .find(|(k, _)| k.as_str() == Some(key) || k.as_slice() == Some(key.as_bytes()))
        .and_then(|(_, v)| v.as_u64().map(|n| n as usize).or_else(|| v.as_i64().map(|n| n as usize)))
}

fn find_string_array(map: &[(Value, Value)], key: &str) -> Vec<String> {
    map.iter()
        .find(|(k, _)| k.as_str() == Some(key) || k.as_slice() == Some(key.as_bytes()))
        .and_then(|(_, v)| v.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|v| v.as_str().or_else(|| v.as_slice().and_then(|b| std::str::from_utf8(b).ok())).map(String::from))
                .collect()
        })
        .unwrap_or_default()
}
