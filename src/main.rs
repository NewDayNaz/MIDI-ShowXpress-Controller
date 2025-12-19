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
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use tokio::sync::mpsc;
use midir::MidiInputConnection;

#[derive(PartialEq)]
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
    midi_messages: HashMap<String, Vec<MidiMessage>>,
    flashing_messages: HashMap<String, f64>, // Maps display name to flash start time
    midi_learn: MidiLearnState,
    storage: PresetStorage,
    config: AppConfig,
    action_tx: mpsc::UnboundedSender<ActionCommand>,
    preset_matcher: Arc<Mutex<PresetMatcher>>,
    
    // MIDI Port Selection
    available_midi_ports: Vec<String>,
    selected_midi_port: Option<usize>,
    midi_connection_active: bool,
    midi_connection: Arc<Mutex<Option<MidiInputConnection<()>>>>,
    
    // Controller Connection
    connection_state: ConnectionState,
    connection_address: String,
    connection_password: String,
    
    // UI State
    new_preset_name: String,
    new_preset_desc: String,
    show_new_preset_modal: bool,
    show_delete_confirm_modal: bool,
    pending_delete_preset: Option<usize>,
    pending_button_action: Option<(u32, String)>,
    last_action_type: ButtonActionType,
    action_delay: f32,
    search_filter: String,
    buttons_just_updated: bool,
    
    // Button Selection State
    selected_button_indices: HashSet<usize>,
    last_clicked_button_index: Option<usize>,
}

impl AppState {
    fn new(
        storage: PresetStorage,
        action_tx: mpsc::UnboundedSender<ActionCommand>,
        available_midi_ports: Vec<String>,
        midi_connection: Arc<Mutex<Option<MidiInputConnection<()>>>>,
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

        let connection_address = config.last_controller_address.clone()
            .unwrap_or_else(|| "127.0.0.1:7348".to_string());

        let connection_password = config.last_controller_password.clone()
            .unwrap_or_else(|| String::new());

        let last_action_type = config.last_action_type
            .unwrap_or(ButtonActionType::Toggle);

        // Select the first preset if any exist
        let selected_preset = if presets.is_empty() {
            None
        } else {
            Some(0)
        };

        Ok(Self {
            presets,
            selected_preset,
            buttons: Vec::new(),
            midi_log: MidiLog::new(100),
            midi_messages: HashMap::new(),
            flashing_messages: HashMap::new(),
            midi_learn: MidiLearnState::new(),
            storage,
            config,
            action_tx,
            preset_matcher,
            available_midi_ports,
            selected_midi_port,
            midi_connection_active: false,
            midi_connection,
            connection_state: ConnectionState::Disconnected,
            connection_address,
            connection_password,
            new_preset_name: String::new(),
            new_preset_desc: String::new(),
            show_new_preset_modal: false,
            show_delete_confirm_modal: false,
            pending_delete_preset: None,
            pending_button_action: None,
            last_action_type,
            action_delay: 0.0,
            search_filter: String::new(),
            buttons_just_updated: false,
            selected_button_indices: HashSet::new(),
            last_clicked_button_index: None,
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

    fn handle_button_click(&mut self, button_idx: usize, ui: &Ui) {
        let is_shift = ui.io().key_shift;
        let is_ctrl = ui.io().key_ctrl;
        
        if is_shift {
            // Shift+Click: Select range from last clicked to current
            if let Some(last_idx) = self.last_clicked_button_index {
                let start = last_idx.min(button_idx);
                let end = last_idx.max(button_idx);
                for idx in start..=end {
                    self.selected_button_indices.insert(idx);
                }
            } else {
                // No previous click, just select this one
                self.selected_button_indices.insert(button_idx);
            }
            self.last_clicked_button_index = Some(button_idx);
        } else if is_ctrl {
            // Ctrl+Click: Toggle selection
            if self.selected_button_indices.contains(&button_idx) {
                self.selected_button_indices.remove(&button_idx);
            } else {
                self.selected_button_indices.insert(button_idx);
            }
            self.last_clicked_button_index = Some(button_idx);
        } else {
            // Regular click: Select only this button
            self.selected_button_indices.clear();
            self.selected_button_indices.insert(button_idx);
            self.last_clicked_button_index = Some(button_idx);
        }
    }

    fn handle_midi_message(&mut self, msg: MidiMessage) {
        // Clone early for storage, keep original for other uses
        let msg_for_storage = msg.clone();
        let display = msg.display_name();
        self.midi_log.add(format!("{}", display));

        let category = match &msg {
            MidiMessage::NoteOn(_) => "Note On",
            MidiMessage::NoteOff(_) => "Note Off",
            MidiMessage::ControlChange { .. } => "Control Change",
        };
        
        let messages = self.midi_messages
            .entry(category.to_string())
            .or_insert_with(Vec::new);
        
        // Only add if it doesn't already exist (check by display name)
        let display_str = display.clone();
        let already_exists = messages.iter().any(|m| m.display_name() == display_str);
        if !already_exists {
            messages.push(msg_for_storage);
        } else {
            // Flash the existing entry
            let current_time = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs_f64();
            self.flashing_messages.insert(display_str.clone(), current_time);
        }

        self.midi_learn.capture(&msg);

        if let Ok(matcher) = self.preset_matcher.lock() {
            if let Some(preset_name) = matcher.handle_midi(&msg) {
                self.midi_log.add(format!("Executing preset: {}", preset_name));
            }
        }
    }

    fn render_midi_panel(&mut self, ui: &Ui) -> Option<usize> {
        let mut port_change_request: Option<usize> = None;
        let mut pending_trigger: Option<MidiTrigger> = None;
        
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
                                ui.text_wrapped(message);
                            }
                            if !self.midi_log.entries.is_empty() {
                                ui.set_scroll_here_y_with_ratio(1.0);
                            }
                        });
                }

                ui.separator();

                // MIDI Port Selector
                ui.text("MIDI Device:");
                ui.set_next_item_width(-1.0);
                let preview = if let Some(idx) = self.selected_midi_port {
                    if idx < self.available_midi_ports.len() {
                        &self.available_midi_ports[idx]
                    } else {
                        "None"
                    }
                } else {
                    "None"
                };

                if let Some(_token) = ui.begin_combo("##midi_port_selector", preview) {
                    for (idx, port_name) in self.available_midi_ports.iter().enumerate() {
                        let selected = self.selected_midi_port == Some(idx);
                        if ui.selectable_config(port_name).selected(selected).build() {
                            if self.selected_midi_port != Some(idx) {
                                port_change_request = Some(idx);
                            }
                        }
                    }
                }

                ui.separator();

                if ui.collapsing_header("MIDI Messages", TreeNodeFlags::DEFAULT_OPEN) {
                    ui.child_window("##midi_tree")
                        .size([0.0, 0.0])
                        .border(true)
                        .build(|| {
                            // Get current time for flash calculations
                            let current_time = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_secs_f64();
                            
                            // Clean up old flash entries (older than 1 second)
                            self.flashing_messages.retain(|_, flash_time| {
                                current_time - *flash_time < 1.0
                            });
                            
                            for (category, messages) in &self.midi_messages {
                                if ui.tree_node_config(category).default_open(true).build(|| {
                                    for msg in messages {
                                        let display = msg.display_name();
                                        
                                        // Check if this message is flashing
                                        let is_flashing = if let Some(flash_time) = self.flashing_messages.get(&display) {
                                            let age = (current_time - flash_time) as f32;
                                            if age < 1.0f32 {
                                                Some(age)
                                            } else {
                                                None
                                            }
                                        } else {
                                            None
                                        };
                                        
                                        // Apply flash color if flashing
                                        if let Some(age) = is_flashing {
                                            // Fade from bright yellow to normal over 1 second
                                            let intensity: f32 = 1.0f32 - age; // 1.0 -> 0.0
                                            let r: f32 = 1.0f32;
                                            let g: f32 = 0.8f32 + 0.2f32 * intensity;
                                            let b: f32 = 0.2f32 * intensity;
                                            let a: f32 = 1.0f32;
                                            let flash_color = [r, g, b, a];
                                            let _style = ui.push_style_color(StyleColor::Text, flash_color);
                                            if ui.selectable(&display) {
                                                // Handle single click if needed
                                            }
                                        } else {
                                            // Normal display
                                            if ui.selectable(&display) {
                                                // Handle single click if needed
                                            }
                                        }
                                        
                                        // Handle double-click to add as trigger
                                        if ui.is_item_hovered() && ui.is_mouse_double_clicked(MouseButton::Left) {
                                            // Collect the trigger to add (we'll process it after iteration)
                                            if let Some(trigger) = MidiTrigger::from_message(msg) {
                                                pending_trigger = Some(trigger);
                                            }
                                        }
                                        
                                        if ui.is_item_hovered() {
                                            ui.tooltip_text("Double-click to add as trigger");
                                        }
                                    }
                                }).is_some() {}
                            }
                        });
                }
            });
        
        // Process pending trigger addition after the UI rendering is complete
        if let Some(trigger) = pending_trigger {
            if let Some(preset_idx) = self.selected_preset {
                // Check for duplicate trigger
                let is_duplicate = self.presets[preset_idx].triggers
                    .iter()
                    .any(|existing_trigger| existing_trigger == &trigger);
                
                if !is_duplicate {
                    self.presets[preset_idx].triggers.push(trigger.clone());
                    let _ = self.save_presets();
                }
            }
        }
        
        port_change_request
    }

    fn render_preset_panel(&mut self, ui: &Ui) {
        ui.child_window("##preset_panel")
            .size([500.0, 0.0])
            .border(true)
            .build(|| {
                ui.text_colored([1.0, 0.8, 0.8, 1.0], "Preset Builder");
                ui.separator();

                ui.text("Preset:");
                ui.same_line();
                
                let preview = if let Some(idx) = self.selected_preset {
                    &self.presets[idx].name
                } else {
                    "None"
                };

                ui.set_next_item_width(200.0);
                if let Some(_token) = ui.begin_combo("##preset_selector", preview) {
                    // Create sorted indices by preset name
                    let mut sorted_indices: Vec<usize> = (0..self.presets.len()).collect();
                    sorted_indices.sort_by_key(|&idx| &self.presets[idx].name);
                    
                    for &sorted_idx in &sorted_indices {
                        let preset = &self.presets[sorted_idx];
                        let selected = self.selected_preset == Some(sorted_idx);
                        if ui.selectable_config(&preset.name).selected(selected).build() {
                            self.selected_preset = Some(sorted_idx);
                        }
                    }
                }

                ui.same_line();
                if ui.button("New") {
                    self.show_new_preset_modal = true;
                }

                ui.same_line();
                if let Some(idx) = self.selected_preset {
                    {
                        let _style1 = ui.push_style_color(StyleColor::Button, [0.8, 0.2, 0.2, 1.0]);
                        let _style2 = ui.push_style_color(StyleColor::ButtonHovered, [1.0, 0.3, 0.3, 1.0]);
                        let _style3 = ui.push_style_color(StyleColor::ButtonActive, [0.6, 0.1, 0.1, 1.0]);
                        if ui.button("Delete") {
                            self.pending_delete_preset = Some(idx);
                            self.show_delete_confirm_modal = true;
                        }
                    }
                } else {
                    ui.disabled(true, || {
                        ui.button("Delete");
                    });
                }

                ui.separator();

                if let Some(idx) = self.selected_preset {
                    let preset = &self.presets[idx];
                    
                    ui.text_colored([0.8, 1.0, 0.8, 1.0], &preset.name);
                    ui.text_disabled(&preset.description);
                    
                    ui.separator();
                    
                    // Run preset button - disabled if not connected
                    let is_connected = self.connection_state == ConnectionState::Connected;
                    let has_actions = !preset.actions.is_empty();
                    let can_run = is_connected && has_actions;
                    
                    ui.disabled(!can_run, || {
                        if ui.button("Run Preset") {
                            let preset_clone = preset.clone();
                            let _ = self.action_tx.send(ActionCommand::ExecutePreset(preset_clone));
                            self.midi_log.add(format!("Manually running preset: {}", preset.name));
                        }
                    });
                    
                    // Show tooltip when hovering over disabled button
                    if ui.is_item_hovered() && !can_run {
                        if !is_connected {
                            ui.tooltip_text("Connect to controller to run presets");
                        } else if !has_actions {
                            ui.tooltip_text("Add actions to this preset to run it");
                        }
                    }
                    
                    ui.separator();

                    ui.text("Triggers:");
                    ui.same_line();
                    let preset_idx = idx; // Copy the index to avoid borrowing issues
                    let has_triggers = !self.presets[preset_idx].triggers.is_empty();
                    ui.disabled(!has_triggers, || {
                        if ui.small_button("Clear Triggers") {
                            self.presets[preset_idx].triggers.clear();
                            let _ = self.save_presets();
                        }
                    });
                    let preset_idx = idx; // Copy the index to avoid borrowing issues
                    ui.child_window("##triggers")
                        .size([0.0, 150.0])
                        .border(true)
                        .build(|| {
                            // Use indices to avoid borrowing conflicts
                            let triggers_len = self.presets[preset_idx].triggers.len();
                            for i in 0..triggers_len {
                                let trigger_display = self.presets[preset_idx].triggers[i].display_name();
                                ui.bullet_text(&trigger_display);
                                ui.same_line();
                                if ui.small_button(&format!("X##trig_{}", i)) {
                                    self.presets[preset_idx].triggers.remove(i);
                                    let _ = self.save_presets();
                                    break; // Break to avoid index issues after removal
                                }
                            }
                            if self.presets[preset_idx].triggers.is_empty() {
                                ui.text_disabled("No triggers configured");
                            }
                        });

                    ui.separator();
                    ui.text("Actions:");
                    ui.same_line();
                    let preset_idx = idx; // Copy the index to avoid borrowing issues
                    let has_actions = !self.presets[preset_idx].actions.is_empty();
                    ui.disabled(!has_actions, || {
                        if ui.small_button("Clear Actions") {
                            self.presets[preset_idx].actions.clear();
                            let _ = self.save_presets();
                        }
                    });
                    
                    // Action type selector
                    ui.text("Default Action Type:");
                    ui.same_line();
                    let action_types = ["Press", "Release", "Toggle"];
                    let current_idx = match self.last_action_type {
                        ButtonActionType::Press => 0,
                        ButtonActionType::Release => 1,
                        ButtonActionType::Toggle => 2,
                    };
                    ui.set_next_item_width(100.0);
                    if let Some(_token) = ui.begin_combo("##action_type", action_types[current_idx]) {
                        for (idx, action_name) in action_types.iter().enumerate() {
                            let selected = current_idx == idx;
                            if ui.selectable_config(action_name).selected(selected).build() {
                                self.last_action_type = match idx {
                                    0 => ButtonActionType::Press,
                                    1 => ButtonActionType::Release,
                                    2 => ButtonActionType::Toggle,
                                    _ => ButtonActionType::Toggle,
                                };
                                // Save to config
                                self.config.last_action_type = Some(self.last_action_type);
                                self.save_config();
                            }
                        }
                    }
                    
                    let preset_idx = idx; // Copy the index to avoid borrowing issues
                    ui.child_window("##actions")
                        .size([0.0, 0.0])
                        .border(true)
                        .build(|| {
                            // Display actions - use indices to avoid long-lived borrows
                            const MAX_NAME_LENGTH: usize = 34;
                            let actions_len = self.presets[preset_idx].actions.len();
                            for i in 0..actions_len {
                                // Collect data we need first, then drop the borrow
                                let (button_name, current_action_type, truncated_name, button_name_len) = {
                                    let action = &self.presets[preset_idx].actions[i];
                                    let button_name_len = action.button_name.len();
                                    let truncated_name = if button_name_len > MAX_NAME_LENGTH {
                                        format!("{}", &action.button_name[..MAX_NAME_LENGTH])
                                    } else {
                                        action.button_name.clone()
                                    };
                                    (action.button_name.clone(), action.action, truncated_name, button_name_len)
                                };
                                
                                ui.bullet();
                                
                                // Action type dropdown for editing
                                let action_types = ["Press", "Release", "Toggle"];
                                let current_action_idx = match current_action_type {
                                    ButtonActionType::Press => 0,
                                    ButtonActionType::Release => 1,
                                    ButtonActionType::Toggle => 2,
                                };
                                ui.set_next_item_width(80.0);
                                if let Some(_token) = ui.begin_combo(&format!("##action_type_{}", i), action_types[current_action_idx]) {
                                    for (idx, action_name) in action_types.iter().enumerate() {
                                        let selected = current_action_idx == idx;
                                        if ui.selectable_config(action_name).selected(selected).build() {
                                            let new_action_type = match idx {
                                                0 => ButtonActionType::Press,
                                                1 => ButtonActionType::Release,
                                                2 => ButtonActionType::Toggle,
                                                _ => ButtonActionType::Toggle,
                                            };
                                            // Now we can mutably borrow since we dropped the immutable borrow
                                            self.presets[preset_idx].actions[i].action = new_action_type;
                                            let _ = self.save_presets();
                                        }
                                    }
                                }
                                
                                ui.same_line();
                                ui.text(&truncated_name);
                                
                                // Show tooltip with full name if truncated
                                if button_name_len > MAX_NAME_LENGTH && ui.is_item_hovered() {
                                    let full_action_text = format!(
                                        "{:?} {}",
                                        current_action_type, button_name
                                    );
                                    ui.tooltip_text(&full_action_text);
                                }
                                
                                ui.same_line();
                                if ui.small_button(&format!("X##act_{}", i)) {
                                    self.presets[preset_idx].actions.remove(i);
                                    let _ = self.save_presets();
                                    break; // Break to avoid index issues after removal
                                }
                            }
                            if self.presets[preset_idx].actions.is_empty() {
                                ui.text_disabled("Double-click a button to add it here");
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
                        // Automatically select the newly created preset
                        self.selected_preset = Some(self.presets.len() - 1);
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

                if self.show_delete_confirm_modal {
                    ui.open_popup("Delete Preset");
                }

                ui.popup("Delete Preset", || {
                    if let Some(idx) = self.pending_delete_preset {
                        if idx < self.presets.len() {
                            let preset_name = &self.presets[idx].name;
                            ui.text_colored([1.0, 0.8, 0.8, 1.0], "Are you sure?");
                            ui.separator();
                            ui.text(&format!("Delete preset \"{}\"?", preset_name));
                            ui.text_disabled("This action cannot be undone.");
                            ui.separator();

                            {
                                let _style1 = ui.push_style_color(StyleColor::Button, [0.8, 0.2, 0.2, 1.0]);
                                let _style2 = ui.push_style_color(StyleColor::ButtonHovered, [1.0, 0.3, 0.3, 1.0]);
                                let _style3 = ui.push_style_color(StyleColor::ButtonActive, [0.6, 0.1, 0.1, 1.0]);
                                if ui.button("Delete") {
                                    // Remove the preset
                                    self.presets.remove(idx);
                                    
                                    // Update selected_preset if needed
                                    if self.presets.is_empty() {
                                        self.selected_preset = None;
                                    } else if let Some(selected) = self.selected_preset {
                                        if selected >= idx {
                                            // If we deleted the selected preset or one before it, adjust
                                            if selected == idx {
                                                // Deleted the selected one, select the previous or first
                                                self.selected_preset = if idx > 0 {
                                                    Some(idx - 1)
                                                } else if !self.presets.is_empty() {
                                                    Some(0)
                                                } else {
                                                    None
                                                };
                                            } else {
                                                // Deleted one before selected, shift index down
                                                self.selected_preset = Some(selected - 1);
                                            }
                                        }
                                    }
                                    
                                    // Save and update matcher
                                    let _ = self.save_presets();
                                    
                                    self.show_delete_confirm_modal = false;
                                    self.pending_delete_preset = None;
                                    ui.close_current_popup();
                                }
                            }

                            ui.same_line();
                            if ui.button("Cancel") {
                                self.show_delete_confirm_modal = false;
                                self.pending_delete_preset = None;
                                ui.close_current_popup();
                            }
                        } else {
                            // Invalid index, just close
                            self.show_delete_confirm_modal = false;
                            self.pending_delete_preset = None;
                            ui.close_current_popup();
                        }
                    }
                });
            });
    }

    fn render_button_panel(&mut self, ui: &Ui, ui_tx: &mpsc::UnboundedSender<ActionCommand>) {
        ui.child_window("##button_panel")
            .size([0.0, 0.0])
            .border(true)
            .build(|| {
                ui.text_colored([0.8, 1.0, 1.0, 1.0], "Lighting Controller");
                ui.separator();

                ui.text("Controller Address:");
                ui.input_text("##address", &mut self.connection_address).build();
                if ui.is_item_deactivated_after_edit() {
                    self.config.last_controller_address = Some(self.connection_address.clone());
                    self.save_config();
                }
                
                ui.text("Controller Password:");
                ui.input_text("##password", &mut self.connection_password)
                    .password(true)
                    .build();
                if ui.is_item_deactivated_after_edit() {
                    self.config.last_controller_password = Some(self.connection_password.clone());
                    self.save_config();
                }
                
                let connecting = self.connection_state == ConnectionState::Connecting;
                let is_connected = self.connection_state == ConnectionState::Connected;

                // Connect/Disconnect button - disabled while connecting
                ui.disabled(connecting, || {
                    if is_connected {
                        // Show Disconnect button when connected
                        if ui.button("Disconnect") {
                            self.connection_state = ConnectionState::Disconnected;
                            self.buttons.clear();
                            self.selected_button_indices.clear();
                            self.last_clicked_button_index = None;
                            self.midi_log.add("Disconnected from controller".to_string());
                            let _ = self.action_tx.send(ActionCommand::Disconnect);
                        }
                    } else {
                        // Show Connect button when disconnected
                        if ui.button("Connect") {
                            // Only send connect command if not already connecting
                            self.connection_state = ConnectionState::Connecting;
                            self.midi_log.add(format!("Connecting to {}", self.connection_address));

                            let addr = self.connection_address.clone();
                            let pass = self.connection_password.clone();
                            let _ = self.action_tx.send(ActionCommand::Connect(addr, pass));
                        }
                    }
                });

                ui.separator();

                match &self.connection_state {
                    ConnectionState::Connected => {
                        // Add selected buttons button
                        let has_selection = !self.selected_button_indices.is_empty();
                        let preset_selected = self.selected_preset.is_some();
                        ui.disabled(!has_selection || !preset_selected, || {
                            if ui.button(&format!("Add Selected ({})", self.selected_button_indices.len())) {
                                if let Some(preset_idx) = self.selected_preset {
                                    let selected_indices: Vec<usize> = self.selected_button_indices.iter().copied().collect();
                                    let action_type = self.last_action_type;
                                    
                                    for button_idx in selected_indices {
                                        if button_idx < self.buttons.len() {
                                            let button_name = self.buttons[button_idx].name.clone();
                                            
                                            // Check for duplicate action (same button name and action type)
                                            let is_duplicate = self.presets[preset_idx].actions.iter()
                                                .any(|existing_action| {
                                                    existing_action.button_name == button_name
                                                        && existing_action.action == action_type
                                                });
                                            
                                            if !is_duplicate {
                                                let action = ButtonAction {
                                                    button_name,
                                                    action: action_type,
                                                    delay_secs: 0.0,
                                                };
                                                self.presets[preset_idx].actions.push(action);
                                            }
                                        }
                                    }
                                    
                                    let _ = self.save_presets();
                                    self.selected_button_indices.clear();
                                    self.last_clicked_button_index = None;
                                }
                            }
                        });
                        
                        if !has_selection && preset_selected {
                            ui.same_line();
                            ui.text_disabled("Select buttons to add");
                        }
                        
                        ui.separator();
                        
                        // Collect selection state before the closure
                        let selected_indices_clone: HashSet<usize> = self.selected_button_indices.iter().copied().collect();
                        
                        let clicked_indices = RefCell::new(Vec::new());
                        let double_clicked_data = RefCell::new(Vec::new());
                        
                        ui.child_window("##buttons")
                            .size([0.0, 0.0])
                            .border(true)
                            .build(|| {
                                let buttons_len = self.buttons.len();
                                
                                for button_idx in 0..buttons_len {
                                    // Collect button name first to avoid borrow conflicts
                                    let button_name = self.buttons[button_idx].name.clone();
                                    let button_label = &button_name;
                                    let is_selected = selected_indices_clone.contains(&button_idx);
                                    
                                    // Apply selection styling
                                    let was_clicked = if is_selected {
                                        let _style = ui.push_style_color(StyleColor::Header, [0.2, 0.5, 0.8, 0.5]);
                                        let _style2 = ui.push_style_color(StyleColor::HeaderHovered, [0.3, 0.6, 0.9, 0.7]);
                                        let _style3 = ui.push_style_color(StyleColor::HeaderActive, [0.2, 0.5, 0.8, 0.9]);
                                        
                                        ui.selectable_config(button_label).selected(true).build()
                                    } else {
                                        ui.selectable(button_label)
                                    };
                                    
                                    if was_clicked {
                                        clicked_indices.borrow_mut().push(button_idx);
                                    }
                                    
                                    // Handle double-click - collect values first to avoid borrow conflicts
                                    if ui.is_item_hovered() && ui.is_mouse_double_clicked(MouseButton::Left) {
                                        let preset_idx_opt = self.selected_preset;
                                        let action_type = self.last_action_type;
                                        double_clicked_data.borrow_mut().push((preset_idx_opt, button_name.clone(), action_type));
                                    }
                                    
                                    if ui.is_item_hovered() {
                                        ui.tooltip_text("Click to select, Shift+Click for range, Ctrl+Click to toggle, Double-click to add");
                                    }
                                }

                                if self.buttons.is_empty() {
                                    ui.text_disabled("No buttons loaded");
                                } else if self.buttons_just_updated {
                                    // Auto-scroll to the last button only when buttons are updated
                                    ui.set_scroll_here_y_with_ratio(1.0);
                                    self.buttons_just_updated = false;
                                }
                            });
                        
                        // Handle clicks after UI rendering is complete (outside the closure)
                        for button_idx in clicked_indices.into_inner() {
                            self.handle_button_click(button_idx, ui);
                        }
                        
                        // Handle double-clicks after UI rendering is complete (outside the closure)
                        for (preset_idx_opt, button_name, action_type) in double_clicked_data.into_inner() {
                            if let Some(preset_idx) = preset_idx_opt {
                                // Check for duplicate action (same button name and action type)
                                let is_duplicate = self.presets[preset_idx].actions.iter()
                                    .any(|existing_action| {
                                        existing_action.button_name == button_name
                                            && existing_action.action == action_type
                                    });
                                
                                if !is_duplicate {
                                    let action = ButtonAction {
                                        button_name,
                                        action: action_type,
                                        delay_secs: 0.0,
                                    };
                                    self.presets[preset_idx].actions.push(action);
                                    let _ = self.save_presets();
                                }
                            }
                        }
                    }
                    ConnectionState::Connecting => {
                        ui.text_disabled("Connecting...");
                    }
                    ConnectionState::Disconnected => {
                        ui.text_disabled("Not connected");
                    }
                    ConnectionState::Error(err) => {
                        let _style = ui.push_style_color(StyleColor::Text, [1.0, 0.2, 0.2, 1.0]);
                        ui.text_wrapped(&format!("Connection error: {}", err));
                    }
                }
            });
    }
}

fn connect_midi_port(
    port_idx: usize,
    available_ports: &[String],
    state: Arc<Mutex<AppState>>,
) -> Result<()> {
    // Get a reference to midi_connection Arc to avoid nested locks
    let midi_conn_arc = {
        let state_guard = state.lock().unwrap();
        Arc::clone(&state_guard.midi_connection)
    };
    
    // Disconnect existing connection
    {
        let mut conn_guard = midi_conn_arc.lock().unwrap();
        *conn_guard = None;
    }

    // Create new MIDI input
    let midi_in = midir::MidiInput::new("lighting-midi")?;
    let ports = midi_in.ports();
    
    if port_idx >= ports.len() {
        return Err(anyhow::anyhow!("Invalid port index"));
    }

    let port_name = midi_in.port_name(&ports[port_idx])
        .unwrap_or_else(|_| format!("Port {}", port_idx));
    
    let state_midi = Arc::clone(&state);
    let conn = midi_in.connect(
        &ports[port_idx],
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

    // Store the connection handle
    {
        let mut conn_guard = midi_conn_arc.lock().unwrap();
        *conn_guard = Some(conn);
    }
    
    // Update state
    {
        let mut state_guard = state.lock().unwrap();
        state_guard.selected_midi_port = Some(port_idx);
        state_guard.midi_connection_active = true;
        state_guard.midi_log.add(format!("MIDI connected to: {}", port_name));
        
        // Save to config
        if port_idx < available_ports.len() {
            state_guard.config.last_midi_port = Some(available_ports[port_idx].clone());
            state_guard.save_config();
        }
    }

    println!("MIDI connected to: {}", port_name);
    Ok(())
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

    // UI sends commands to executor:
    let (action_tx, action_rx) = mpsc::unbounded_channel::<ActionCommand>();

    // Executor sends results back to UI:
    let (ui_tx, mut ui_rx) = mpsc::unbounded_channel::<ActionCommand>();

    let ui_tx_for_executor = ui_tx.clone();
    tokio::spawn(async move {
        let mut executor = ActionExecutor::new(action_rx, ui_tx_for_executor);
        executor.run().await;
    });

    let storage = PresetStorage::new()?;
    let midi_connection = Arc::new(Mutex::new(None));
    let state = Arc::new(Mutex::new(AppState::new(
        storage,
        action_tx.clone(),
        available_midi_ports.clone(),
        Arc::clone(&midi_connection),
    )?));

    // Connect to initial port if available
    let initial_port_idx = {
        let state_guard = state.lock().unwrap();
        state_guard.selected_midi_port
    };

    if let Some(port_idx) = initial_port_idx {
        if port_idx < ports.len() {
            if let Err(e) = connect_midi_port(port_idx, &available_midi_ports, Arc::clone(&state)) {
                eprintln!("Failed to connect to MIDI port {}: {}", port_idx, e);
            }
        }
    }

    // Attempt to connect to controller on startup
    {
        let (connection_address, connection_password) = {
            let state_guard = state.lock().unwrap();
            (state_guard.connection_address.clone(), state_guard.connection_password.clone())
        };
        
        if !connection_address.is_empty() {
            // Set connection state to Connecting
            {
                let mut state_guard = state.lock().unwrap();
                state_guard.connection_state = ConnectionState::Connecting;
                state_guard.midi_log.add(format!("Connecting to {}", connection_address));
            }
            
            // Send connect command
            let addr = connection_address.clone();
            let pass = connection_password.clone();
            let _ = action_tx.send(ActionCommand::Connect(addr, pass));
        }
    }

    let event_loop = winit::event_loop::EventLoop::new();
    let window = winit::window::WindowBuilder::new()
        .with_title("MIDI ShowXpress Controller")
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
                let delta_time = now - last_frame;
                last_frame = now;

                // Check window size and reconfigure surface if needed BEFORE getting frame
                let window_size = window.inner_size();
                
                // Ensure window size is valid (at least 2x2 for rendering)
                if window_size.width < 2 || window_size.height < 2 {
                    // Window is too small, skip rendering
                    return;
                }
                
                // Check if window size changed but surface wasn't updated
                if surface_config.width != window_size.width || surface_config.height != window_size.height {
                    surface_config.width = window_size.width;
                    surface_config.height = window_size.height;
                    surface.configure(&device, &surface_config);
                    // After reconfiguring, skip this frame and wait for next redraw
                    return;
                }
                
                // Double-check surface config is valid before rendering
                if surface_config.width < 2 || surface_config.height < 2 {
                    return;
                }

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

                // Get window size in logical coordinates for imgui
                // imgui works in logical coordinates (points), so we need to convert from physical pixels
                let scale_factor = window.scale_factor();
                let window_width = window_size.width as f32 / scale_factor as f32;
                let window_height = window_size.height as f32 / scale_factor as f32;
                
                // Update imgui state before calling frame()
                // Ensure display size matches window size to prevent scissor rect issues
                {
                    let io = imgui.io_mut();
                    io.update_delta_time(delta_time);
                    io.display_size = [window_width, window_height];
                    platform
                        .prepare_frame(io, &window)
                        .expect("Failed to prepare frame");
                }
                
                let ui = imgui.frame();

                let mut port_change_request: Option<usize> = None;
                
                ui.window("MIDI ShowXpress Controller")
                    .size([window_width, window_height], Condition::Always)
                    .position([0.0, 0.0], Condition::Always)
                    .movable(false)
                    .resizable(false)
                    .build(|| {
                        if let Ok(mut state) = state.lock() {
                            // Process any connection results
                            while let Ok(cmd) = ui_rx.try_recv() {
                                match cmd {
                                    ActionCommand::ConnectionSuccess(buttons) => {
                                        let button_count = buttons.len();
                                        // Only mark as updated if the list actually changed
                                        let buttons_changed = state.buttons != buttons;
                                        if buttons_changed {
                                            state.buttons_just_updated = true;
                                        }
                                        state.buttons = buttons;
                                        // If button list is empty, we're disconnected (sent by Disconnect command)
                                        if button_count == 0 {
                                            state.connection_state = ConnectionState::Disconnected;
                                        } else {
                                            state.connection_state = ConnectionState::Connected;
                                            if buttons_changed {
                                                state.midi_log.add(format!("Connected! Loaded {} buttons", button_count));
                                            }
                                        }
                                    }
                                    ActionCommand::ConnectionError(err) => {
                                        state.connection_state = ConnectionState::Error(err.clone());
                                        state.midi_log.add(format!("Connection error: {}", err));
                                    }
                                    _ => {}
                                }
                            }

                            if let Some(new_port_idx) = state.render_midi_panel(&ui) {
                                port_change_request = Some(new_port_idx);
                            }
                            ui.same_line();
                            state.render_preset_panel(&ui);
                            ui.same_line();
                            state.render_button_panel(&ui, &ui_tx);
                        }
                    });

                // Handle MIDI port change request outside the state lock
                if let Some(new_port_idx) = port_change_request {
                    let available_ports = {
                        let state_guard = state.lock().unwrap();
                        state_guard.available_midi_ports.clone()
                    };
                    if let Err(e) = connect_midi_port(new_port_idx, &available_ports, Arc::clone(&state)) {
                        eprintln!("Failed to reconnect MIDI port {}: {}", new_port_idx, e);
                        if let Ok(mut state_guard) = state.lock() {
                            state_guard.midi_log.add(format!("Failed to connect to MIDI port: {}", e));
                        }
                    }
                }

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

#[cfg(windows)]
fn setup_console_if_needed() -> bool {
    let args: Vec<String> = std::env::args().collect();
    let show_console = args.iter().any(|arg| arg == "-console" || arg == "--console");
    
    if show_console {
        unsafe {
            use winapi::um::consoleapi::AllocConsole;
            
            if AllocConsole() != 0 {
                return true;
            }
        }
    }
    false
}

#[cfg(windows)]
fn show_error_message(title: &str, message: &str) {
    unsafe {
        use winapi::um::winuser::{MessageBoxW, MB_OK, MB_ICONERROR};
        use std::ffi::OsStr;
        use std::os::windows::ffi::OsStrExt;
        
        let title_wide: Vec<u16> = OsStr::new(title).encode_wide().chain(Some(0)).collect();
        let message_wide: Vec<u16> = OsStr::new(message).encode_wide().chain(Some(0)).collect();
        
        MessageBoxW(
            std::ptr::null_mut(),
            message_wide.as_ptr(),
            title_wide.as_ptr(),
            MB_OK | MB_ICONERROR,
        );
    }
}

#[cfg(not(windows))]
fn setup_console_if_needed() -> bool {
    // On non-Windows platforms, console is always available
    true
}

#[cfg(not(windows))]
fn show_error_message(_title: &str, message: &str) {
    eprintln!("{}", message);
}

#[tokio::main]
async fn main() {
    // Setup console if -console flag is present (Windows only)
    let has_console = setup_console_if_needed();
    
    // Ensure only one instance is running
    let instance = single_instance::SingleInstance::new("midi_showxpress_controller").unwrap();
    if !instance.is_single() {
        let error_msg = "Another instance of MIDI ShowXpress Controller is already running.\nPlease close the existing instance before starting a new one.";
        if has_console {
            eprintln!("{}", error_msg);
        } else {
            show_error_message("MIDI ShowXpress Controller", error_msg);
        }
        std::process::exit(1);
    }

    // Keep the instance guard alive for the duration of the program
    // The guard will be dropped when the program exits
    let _instance_guard = instance;

    if let Err(e) = run() {
        let error_msg = format!("Application error: {}", e);
        if has_console {
            eprintln!("{}", error_msg);
        } else {
            show_error_message("MIDI ShowXpress Controller", &error_msg);
        }
        std::process::exit(1);
    }
}