fn main() {
    let dist_dir = std::path::Path::new("frontend/dist");
    if !dist_dir.exists() {
        let _ = std::fs::create_dir_all(dist_dir);
    }
    let index_file = dist_dir.join("index.html");
    if !index_file.exists() {
        let _ = std::fs::write(&index_file, "<!DOCTYPE html><html><body>8BitDo Micro</body></html>");
    }
    tauri_build::build();
}
