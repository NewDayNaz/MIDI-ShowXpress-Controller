fn main() {
    if cfg!(target_os = "windows") {
        winres::WindowsResource::new()
            .set_icon("midi_showxpress_controller.ico")
            .compile()
            .unwrap();
    }
}

