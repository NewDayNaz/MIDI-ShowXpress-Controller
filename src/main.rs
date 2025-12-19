mod action_executor;
mod models;
mod persistence;
mod tcp_client;

use action_executor::{ActionCommand, ActionExecutor, PresetMatcher};
use anyhow::Result;
use chrono::Local;
use imgui::*;
use models::*;
use persistence::PresetStorage;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

struct MidiLog {
    entries: Vec<(String, String)>, // (timestamp, message)
    max_entries: usize,
}

impl MidiLog {
    fn new(max_entries: usize) -> Self {
        Self {
            entries: Vec::new(),
            max_entries,
        }
    }

    fn add(&mut self, message: String) {
        let timestamp = Local::now().format("%H:%M:%S%.3f").to_string();
        self.entries.push((timestamp, message));
        if self.entries.len() > self.max_entries {
            self.entries.remove(0);
        }
    }
}

struct AppState {
    presets: Vec<Preset>,
    selected_preset: Option<usize>,
    buttons: Vec<Button>,
    midi_log: MidiLog,
    midi_messages: HashMap<String, Vec<String>>, // Organized by type
    midi_learn: MidiLearnState,
    storage: PresetStorage,
    action_tx: mpsc::UnboundedSender<ActionCommand>,
    preset_matcher: Arc<Mutex<PresetMatcher>>,
    
    // UI State
    new_preset_name: String,
    new_preset_desc: String,
    show_new_preset_modal: bool,
    pending_button_action: Option<(u32, String)>,
    action_type_selected: i32,
    action_delay: f32,
    connection_address: String,
    connected: bool,
    search_filter: String,
}

impl AppState {
    fn new(
        storage: PresetStorage,
        action_tx: mpsc::UnboundedSender<ActionCommand>,
    ) -> Result<Self> {
        let presets = storage.load().unwrap_or_default();
        let preset_matcher = Arc::new(Mutex::new(PresetMatcher::new(
            presets.clone(),
            action_tx.clone(),
        )));

        Ok(Self {
            presets,
            selected_preset: None,
            buttons: Vec::new(),
            midi_log: MidiLog::new(100),
            midi_messages: HashMap::new(),
            midi_learn: MidiLearnState::new(),
            storage,
            action_tx,
            preset_matcher,
            new_preset_name: String::new(),
            new_preset_desc: String::new(),
            show_new_preset_modal: false,
            pending_button_action: None,
            action_type_selected: 0,
            action_delay: 0.0,
            connection_address: "localhost:7348".to_string(),
            connected: false,
            search_filter: String::new(),
        })
    }

    fn save_presets(&mut self) -> Result<()> {
        self.storage.save(&self.presets)?;
        if let Ok(mut matcher) = self.preset_matcher.lock() {
            matcher.update_presets(self.presets.clone());
        }
        Ok(())
    }

    fn handle_midi_message(&mut self, msg: MidiMessage) {
        let display = msg.display_name();
        self.midi_log.add(format!("→ {}", display));

        // Organize messages by type
        let category = match &msg {
            MidiMessage::NoteOn(_) => "Note On",
            MidiMessage::NoteOff(_) => "Note Off",
            MidiMessage::ControlChange { .. } => "Control Change",
        };

        self.midi_messages
            .entry(category.to_string())
            .or_insert_with(Vec::new)
            .push(display.clone());

        // Handle MIDI learn
        self.midi_learn.capture(&msg);

        // Trigger preset matching
        if let Ok(matcher) = self.preset_matcher.lock() {
            matcher.handle_midi(&msg);
            self.midi_log.add(format!("✓ Matched presets"));
        }
    }

    fn render_midi_panel(&mut self, ui: &Ui) {
        ui.child_window("##midi_panel")
            .size([300.0, 0.0])
            .border(true)
            .build(|| {
                ui.text_colored([0.8, 0.8, 1.0, 1.0], "MIDI Monitor");
                ui.separator();

                // Log section
                if ui.collapsing_header("Console Log", TreeNodeFlags::DEFAULT_OPEN) {
                    ui.child_window("##midi_log")
                        .size([0.0, 200.0])
                        .border(true)
                        .build(|| {
                            for (timestamp, message) in &self.midi_log.entries {
                                ui.text_colored([0.7, 0.7, 0.7, 1.0], timestamp);
                                ui.same_line();
                                ui.text(message);
                            }
                            if self.midi_log.entries.len() > 0 {
                                ui.set_scroll_here_y_with_ratio(1.0);
                            }
                        });
                }

                ui.separator();

                // MIDI message tree
                if ui.collapsing_header("MIDI Messages", TreeNodeFlags::DEFAULT_OPEN) {
                    ui.child_window("##midi_tree")
                        .size([0.0, 0.0])
                        .border(true)
                        .build(|| {
                            for (category, messages) in &self.midi_messages {
                                if ui.tree_node_config(category).default_open(true).build() {
                                    for msg in messages {
                                        ui.text(msg);
                                        
                                        // Drag source (simplified for skeleton)
                                        if ui.is_item_hovered() {
                                            ui.set_tooltip("Drag to preset area");
                                        }
                                    }
                                }
                            }
                        });
                }
            });
    }

    fn render_preset_panel(&mut self, ui: &Ui) {
        ui.child_window("##preset_panel")
            .size([400.0, 0.0])
            .border(true)
            .build(|| {
                ui.text_colored([1.0, 0.8, 0.8, 1.0], "Preset Builder");
                ui.separator();

                // Preset selector
                ui.text("Active Preset:");
                ui.same_line();
                
                let preview = if let Some(idx) = self.selected_preset {
                    &self.presets[idx].name
                } else {
                    "None"
                };

                ui.set_next_item_width(200.0);
                if ui.begin_combo("##preset_selector", preview) {
                    for (idx, preset) in self.presets.iter().enumerate() {
                        let selected = self.selected_preset == Some(idx);
                        if ui.selectable_config(&preset.name).selected(selected).build() {
                            self.selected_preset = Some(idx);
                        }
                    }
                    ui.end_combo();
                }

                ui.same_line();
                if ui.button("New") {
                    self.show_new_preset_modal = true;
                }

                ui.separator();

                // Show selected preset details
                if let Some(idx) = self.selected_preset {
                    let preset = &self.presets[idx];
                    
                    ui.text_colored([0.8, 1.0, 0.8, 1.0], &preset.name);
                    ui.text_disabled(&preset.description);
                    ui.separator();

                    ui.text("Triggers:");
                    ui.child_window("##triggers")
                        .size([0.0, 150.0])
                        .border(true)
                        .build(|| {
                            for (i, trigger) in preset.triggers.iter().enumerate() {
                                ui.bullet_text(&trigger.display_name());
                                ui.same_line();
                                if ui.small_button(&format!("X##trig_{}", i)) {
                                    // Remove trigger - needs mutable borrow handling
                                }
                            }
                            if preset.triggers.is_empty() {
                                ui.text_disabled("Drag MIDI messages here");
                            }
                        });

                    ui.separator();
                    ui.text("Actions:");
                    ui.child_window("##actions")
                        .size([0.0, 0.0])
                        .border(true)
                        .build(|| {
                            for (i, action) in preset.actions.iter().enumerate() {
                                ui.bullet();
                                ui.text(&format!(
                                    "{:?} {} (delay: {:.2}s)",
                                    action.action, action.button_name, action.delay_secs
                                ));
                                ui.same_line();
                                if ui.small_button(&format!("X##act_{}", i)) {
                                    // Remove action
                                }
                            }
                            if preset.actions.is_empty() {
                                ui.text_disabled("Drag buttons here");
                            }
                        });
                }

                // New preset modal
                if self.show_new_preset_modal {
                    ui.open_popup("New Preset");
                }

                ui.popup_modal("New Preset")
                    .always_auto_resize(true)
                    .build(ui, || {
                        ui.text("Name:");
                        ui.input_text("##name", &mut self.new_preset_name).build();
                        
                        ui.text("Description:");
                        ui.input_text("##desc", &mut self.new_preset_desc).build();

                        if ui.button("Create") {
                            let preset = Preset::new(
                                self.new_preset_name.clone(),
                                self.new_preset_desc.clone(),
                            );
                            self.presets.push(preset);
                            let _ = self.save_presets();
                            
                            self.new_preset_name.clear();
                            self.new_preset_desc.clear();
                            self.show_new_preset_modal = false;
                            ui.close_current_popup();
                        }

                        ui.same_line();
                        if ui.button("Cancel") {
                            self.show_new_preset_modal = false;
                            ui.close_current_popup();
                        }
                    });
            });
    }

    fn render_button_panel(&mut self, ui: &Ui) {
        ui.child_window("##button_panel")
            .size([0.0, 0.0])
            .border(true)
            .build(|| {
                ui.text_colored([0.8, 1.0, 1.0, 1.0], "Lighting Buttons");
                ui.separator();

                // Connection controls
                ui.text("Controller Address:");
                ui.input_text("##address", &mut self.connection_address).build();
                
                if ui.button("Connect") {
                    self.connected = true;
                    self.midi_log.add("Connected to controller".to_string());
                    // In production: spawn async task to connect and fetch buttons
                }

                ui.separator();

                // Button list
                if self.connected {
                    ui.child_window("##buttons")
                        .size([0.0, 0.0])
                        .border(true)
                        .build(|| {
                            for button in &self.buttons {
                                if ui.selectable(&format!("{} ({})", button.name, button.id)) {
                                    // Trigger drag
                                }
                                
                                if ui.is_item_hovered() {
                                    ui.set_tooltip("Drag to preset area");
                                }
                            }
                            
                            if self.buttons.is_empty() {
                                ui.text_disabled("No buttons loaded");
                            }
                        });
                } else {
                    ui.text_disabled("Not connected");
                }
            });
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize MIDI
    let midi_in = midir::MidiInput::new("lighting-midi")?;
    let ports = midi_in.ports();
    
    println!("Available MIDI ports:");
    for (i, port) in ports.iter().enumerate() {
        println!("  {}: {}", i, midi_in.port_name(port).unwrap_or_default());
    }

    // Create action executor channel
    let (action_tx, action_rx) = mpsc::unbounded_channel();

    // Spawn action executor
    tokio::spawn(async move {
        let mut executor = ActionExecutor::new(action_rx);
        // In production: connect here
        // executor.connect("localhost:7348").await.ok();
        executor.run().await;
    });

    // Initialize persistence
    let storage = PresetStorage::new()?;

    // Initialize app state
    let state = Arc::new(Mutex::new(AppState::new(storage, action_tx.clone())?));

    // Set up MIDI callback
    if !ports.is_empty() {
        let state_midi = Arc::clone(&state);
        let _midi_conn = midi_in.connect(
            &ports[0],
            "midi-listener",
            move |_timestamp, message, _| {
                if let Some(midi_msg) = MidiMessage::from_raw(message) {
                    if let Ok(mut state) = state_midi.lock() {
                        state.handle_midi_message(midi_msg);
                    }
                }
            },
            (),
        )?;
        println!("MIDI connected to: {}", midi_in.port_name(&ports[0])?);
    }

    // Initialize ImGui
    let event_loop = winit::event_loop::EventLoop::new();
    let window = winit::window::WindowBuilder::new()
        .with_title("Lighting MIDI Controller")
        .with_inner_size(winit::dpi::LogicalSize::new(1200.0, 800.0))
        .build(&event_loop)?;

    let mut imgui = imgui::Context::create();
    let mut platform = imgui_winit_support::WinitPlatform::init(&mut imgui);
    platform.attach_window(
        imgui.io_mut(),
        &window,
        imgui_winit_support::HiDpiMode::Default,
    );

    imgui.set_ini_filename(None);

    // Set up fonts
    let hidpi_factor = window.scale_factor();
    let font_size = (13.0 * hidpi_factor) as f32;
    imgui.io_mut().font_global_scale = (1.0 / hidpi_factor) as f32;

    imgui
        .fonts()
        .add_font(&[FontSource::DefaultFontData {
            config: Some(imgui::FontConfig {
                oversample_h: 1,
                pixel_snap_h: true,
                size_pixels: font_size,
                ..Default::default()
            }),
        }]);

    // Set up wgpu
    let instance = wgpu::Instance::default();
    let surface = unsafe { instance.create_surface(&window) }.unwrap();
    let adapter = instance
        .request_adapter(&wgpu::RequestAdapterOptions {
            power_preference: wgpu::PowerPreference::default(),
            compatible_surface: Some(&surface),
            force_fallback_adapter: false,
        })
        .await
        .unwrap();

    let (device, queue) = adapter
        .request_device(
            &wgpu::DeviceDescriptor {
                label: None,
                features: wgpu::Features::empty(),
                limits: wgpu::Limits::default(),
            },
            None,
        )
        .await
        .unwrap();

    let mut surface_config = surface
        .get_default_config(&adapter, window.inner_size().width, window.inner_size().height)
        .unwrap();
    surface.configure(&device, &surface_config);

    let mut renderer = imgui_wgpu::Renderer::new(
        &mut imgui,
        &device,
        &queue,
        imgui_wgpu::RendererConfig {
            texture_format: surface_config.format,
            ..Default::default()
        },
    );

    let mut last_frame = std::time::Instant::now();

    // Main event loop
    event_loop.run(move |event, _, control_flow| {
        *control_flow = winit::event_loop::ControlFlow::Poll;

        match event {
            winit::event::Event::WindowEvent {
                event: winit::event::WindowEvent::Resized(size),
                ..
            } => {
                surface_config.width = size.width.max(1);
                surface_config.height = size.height.max(1);
                surface.configure(&device, &surface_config);
            }
            winit::event::Event::WindowEvent {
                event: winit::event::WindowEvent::CloseRequested,
                ..
            } => {
                *control_flow = winit::event_loop::ControlFlow::Exit;
            }
            winit::event::Event::MainEventsCleared => {
                window.request_redraw();
            }
            winit::event::Event::RedrawRequested(_) => {
                let now = std::time::Instant::now();
                imgui.io_mut().update_delta_time(now - last_frame);
                last_frame = now;

                let frame = match surface.get_current_texture() {
                    Ok(frame) => frame,
                    Err(e) => {
                        eprintln!("Surface error: {:?}", e);
                        return;
                    }
                };

                let view = frame
                    .texture
                    .create_view(&wgpu::TextureViewDescriptor::default());

                platform
                    .prepare_frame(imgui.io_mut(), &window)
                    .expect("Failed to prepare frame");
                
                let ui = imgui.frame();

                // Main window
                ui.window("Lighting MIDI Controller")
                    .size([1200.0, 800.0], Condition::FirstUseEver)
                    .position([0.0, 0.0], Condition::FirstUseEver)
                    .build(|| {
                        if let Ok(mut state) = state.lock() {
                            // Three-column layout
                            state.render_midi_panel(&ui);
                            ui.same_line();
                            state.render_preset_panel(&ui);
                            ui.same_line();
                            state.render_button_panel(&ui);
                        }
                    });

                let mut encoder = device
                    .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

                platform.prepare_render(&ui, &window);
                let draw_data = imgui.render();

                let mut rpass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &view,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                            store: true,
                        },
                    })],
                    depth_stencil_attachment: None,
                });

                renderer
                    .render(draw_data, &queue, &device, &mut rpass)
                    .expect("Rendering failed");

                drop(rpass);
                queue.submit(Some(encoder.finish()));
                frame.present();
            }
            event => {
                platform.handle_event(imgui.io_mut(), &window, &event);
            }
        }
    });
}
