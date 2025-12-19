#[cfg(target_os = "windows")]
fn main() {
    winres::WindowsResource::new()
        .set_icon("midi_showxpress_controller.ico")
        .compile()
        .unwrap();
}

#[cfg(not(target_os = "windows"))]
fn main() {
    // No-op for non-Windows platforms
}

