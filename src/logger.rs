use std::fs::OpenOptions;
use std::io::Write;

#[allow(dead_code)]
pub fn log(msg: &str) {
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open("/tmp/typst_previewer.log") {
        let millis = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis())
            .unwrap_or(0);
        let _ = writeln!(f, "[{}] {}", millis, msg);
    }
}
