fn main() {
    if cfg!(target_os = "windows") {
        // Set Windows subsystem (no console by default)
        println!("cargo:rustc-link-arg=/SUBSYSTEM:WINDOWS");
        
        winres::WindowsResource::new()
            .set_icon("midi_showxpress_controller.ico")
            .compile()
            .unwrap();
    }
}

