fn main() {
    #[cfg(target_os = "linux")]
    stage_linux_vlc();
    tauri_build::build();
}

#[cfg(target_os = "linux")]
fn stage_linux_vlc() {
    let lib = std::path::Path::new("vlc-runtime/libvlc.so.5");
    println!("cargo:rerun-if-changed=vlc-runtime/libvlc.so.5");
    println!("cargo:rerun-if-changed=../scripts/stage-linux-vlc.sh");
    if lib.exists() {
        return;
    }
    let script = std::path::Path::new("../scripts/stage-linux-vlc.sh");
    if !script.exists() {
        return;
    }
    let _ = std::process::Command::new("bash").arg(script).status();
}
