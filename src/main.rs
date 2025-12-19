mod action_executor;
mod models;
mod persistence;
mod tcp_client;

use action_executor::{ActionCommand, ActionExecutor, PresetMatcher};
use anyhow::Result;
use chrono::Local;
use imgui::*;
use models::*;
use persistence::{AppConfig, PresetStorage};
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;

enum ConnectionState {
    Disconnected,
    Connecting,
    Connected,
    Error(String),
}

struct MidiLog {
    entries: Vec<(String, String)>,
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
    midi_messages: HashMap<String, Vec<String>>,
    midi_learn: MidiLearnState,
    storage: PresetStorage,
    config: AppConfig,
    action_tx: mpsc::UnboundedSender<ActionCommand>,
    preset_matcher: Arc<Mutex<PresetMatcher>>,
    
    // MIDI Port Selection
    available_midi_ports: Vec<String>,
    selected_midi_port: Option<usize>,
    midi_connection_active: bool,
    
    // Controller Connection
    connection_state: ConnectionState,
    connection_address: String,
    
    // UI State
    new_preset_name: String,
    new_preset_desc: String,
    show_new_preset_modal: bool,
    pending_button_action: Option<(u32, String)>,
    action_type_selected: i32,
    action_delay: f32,
    search_filter: String,
}

impl AppState {
    fn new(
        storage: PresetStorage,
        action_tx: mpsc::UnboundedSender<ActionCommand>,
        available_midi_ports: Vec<String>,
    ) -> Result<Self> {
        let presets = storage.load().unwrap_or_default();
        let config = storage.load_config().unwrap_or_default();
        
        let preset_matcher = Arc::new(Mutex::new(PresetMatcher::new(
            presets.clone(),
            action_tx.clone(),
        )));

        // Find the last used MIDI port
        let selected_midi_port = if let Some(ref last_port) = config.last_midi_port {
            available_midi_ports.iter().position(|p| p == last_port)
        } else {
            if !available_midi_ports.is_empty() { Some(0) } else { None }
        };

        let connection_address = config.last_controller_address
            .unwrap_or_else(|| "localhost:7348".to_string());

        Ok(Self {
            presets,
            selected_preset: None,
            buttons: Vec::new(),
            midi_log: MidiLog::new(100),
            midi_messages: HashMap::new(),
            midi_learn: MidiLearnState::new(),
            storage,
            config,
            action_tx,
            preset_matcher,
            available_midi_ports,
            selected_midi_port,
            midi_connection_active: false,
            connection_state: ConnectionState::Disconnected,
            connection_address,
            new_preset_name: String::new(),
            new_preset_desc: String::new(),
            show_new_preset_modal: false,
            pending_button_action: None,
            action_type_selected: 0,
            action_delay: 0.0,
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

    fn save_config(&mut self) {
        if let Err(e) = self.storage.save_config(&self.config) {
            eprintln!("Failed to save config: {}", e);
        }
    }

    fn handle_midi_message(&mut self, msg: MidiMessage) {
        let display = msg.display_name();
        self.midi_log.add(format!("→ {}", display));

        let category = match &msg {
            MidiMessage::NoteOn(_) => "Note On",
            MidiMessage::NoteOff(_) => "Note Off",
            MidiMessage::ControlChange { .. } => "Control Change",
        };

        self.midi_messages
            .entry(category.to_string())
            .or_insert_with(Vec::new)
            .push(display.clone());

        self.midi_learn.capture(&msg);

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
                            if !self.midi_log.entries.is_empty() {
                                ui.set_scroll_here_y_with_ratio(1.0);
                            }
                        });
                }

                ui.separator();

                if ui.collapsing_header("MIDI Messages", TreeNodeFlags::DEFAULT_OPEN) {
                    ui.child_window("##midi_tree")
                        .size([0.0, 0.0])
                        .border(true)
                        .build(|| {
                            for (category, messages) in &self.midi_messages {
                                if ui.tree_node_config(category).default_open(true).build(|| {
                                    for msg in messages {
                                        ui.text(msg);
                                        
                                        if ui.is_item_hovered() {
                                            ui.tooltip_text("Drag to preset area");
                                        }
                                    }
                                }).is_some() {}
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

                ui.text("Active Preset:");
                ui.same_line();
                
                let preview = if let Some(idx) = self.selected_preset {
                    &self.presets[idx].name
                } else {
                    "None"
                };

                ui.set_next_item_width(200.0);
                if let Some(_token) = ui.begin_combo("##preset_selector", preview) {
                    for (idx, preset) in self.presets.iter().enumerate() {
                        let selected = self.selected_preset == Some(idx);
                        if ui.selectable_config(&preset.name).selected(selected).build() {
                            self.selected_preset = Some(idx);
                        }
                    }
                }

                ui.same_line();
                if ui.button("New") {
                    self.show_new_preset_modal = true;
                }

                ui.separator();

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
                                }
                            }
                            if preset.actions.is_empty() {
                                ui.text_disabled("Drag buttons here");
                            }
                        });
                }

                if self.show_new_preset_modal {
                    ui.open_popup("New Preset");
                }

                ui.popup("New Preset", || {
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

                ui.text("Controller Address:");
                ui.input_text("##address", &mut self.connection_address).build();
                
                if ui.button("Connect") {
                    self.connected = true;
                    self.midi_log.add("Connected to controller".to_string());
                }

                ui.separator();

                if self.connected {
                    ui.child_window("##buttons")
                        .size([0.0, 0.0])
                        .border(true)
                        .build(|| {
                            for button in &self.buttons {
                                if ui.selectable(&format!("{} ({})", button.name, button.id)) {
                                }
                                
                                if ui.is_item_hovered() {
                                    ui.tooltip_text("Drag to preset area");
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

fn run() -> Result<()> {
    let midi_in = midir::MidiInput::new("lighting-midi")?;
    let ports = midi_in.ports();
    
    let available_midi_ports: Vec<String> = ports
        .iter()
        .map(|p| midi_in.port_name(p).unwrap_or_else(|_| "Unknown".to_string()))
        .collect();
    
    println!("Available MIDI ports:");
    for (i, port_name) in available_midi_ports.iter().enumerate() {
        println!("  {}: {}", i, port_name);
    }

    let (action_tx, action_rx) = mpsc::unbounded_channel();
    let action_rx_ui = action_tx.clone();

    tokio::spawn(async move {
        let mut executor = ActionExecutor::new(action_rx);
        executor.run().await;
    });

    let storage = PresetStorage::new()?;
    let state = Arc::new(Mutex::new(AppState::new(storage, action_tx.clone(), available_midi_ports.clone())?));

    if !ports.is_empty() {
        let port_name = midi_in.port_name(&ports[0]).unwrap_or_default();
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
        println!("MIDI connected to: {}", port_name);
    }

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

    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        dx12_shader_compiler: Default::default(),
    });
    let surface = unsafe { instance.create_surface(&window) }.unwrap();
    let adapter = pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
        power_preference: wgpu::PowerPreference::default(),
        compatible_surface: Some(&surface),
        force_fallback_adapter: false,
    }))
    .unwrap();

    let (device, queue) = pollster::block_on(adapter.request_device(
        &wgpu::DeviceDescriptor {
            label: None,
            features: wgpu::Features::empty(),
            limits: wgpu::Limits::default(),
        },
        None,
    ))
    .unwrap();

    let surface_caps = surface.get_capabilities(&adapter);
    let surface_format = surface_caps.formats.iter()
        .copied()
        .find(|f| f.is_srgb())
        .unwrap_or(surface_caps.formats[0]);

    let mut surface_config = wgpu::SurfaceConfiguration {
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        format: surface_format,
        width: window.inner_size().width,
        height: window.inner_size().height,
        present_mode: wgpu::PresentMode::Fifo,
        alpha_mode: surface_caps.alpha_modes[0],
        view_formats: vec![],
    };
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

                ui.window("Lighting MIDI Controller")
                    .size([1200.0, 800.0], Condition::FirstUseEver)
                    .position([0.0, 0.0], Condition::FirstUseEver)
                    .build(|| {
                        if let Ok(mut state) = state.lock() {
                            // Process any connection results
                            while let Ok(cmd) = action_rx_ui.try_recv() {
                                match cmd {
                                    ActionCommand::ConnectionSuccess(buttons) => {
                                        state.buttons = buttons;
                                        state.connection_state = ConnectionState::Connected;
                                        state.midi_log.add(format!("Connected! Loaded {} buttons", state.buttons.len()));
                                    }
                                    ActionCommand::ConnectionError(err) => {
                                        state.connection_state = ConnectionState::Error(err.clone());
                                        state.midi_log.add(format!("Connection error: {}", err));
                                    }
                                    _ => {}
                                }
                            }

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

#[tokio::main]
async fn main() {
    if let Err(e) = run() {
        eprintln!("Application error: {}", e);
        std::process::exit(1);
    }
}