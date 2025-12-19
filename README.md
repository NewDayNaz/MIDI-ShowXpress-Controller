### Overview

MIDI ShowXpress Controller is a powerful bridge application that enables you to control ShowXpress software using MIDI devices. Connect your MIDI controller, keyboard, or any MIDI-compatible device to trigger lighting presets and button actions in real-time.

### Key Features

#### MIDI Integration
- **Full MIDI Support**: Receive and monitor MIDI messages from any MIDI device (Note On/Off, Control Change)
- **Real-time Monitoring**: Live MIDI message display with timestamped console log
- **MIDI Learn**: Visual feedback when MIDI messages are received

#### Preset Management
- **Create Custom Presets**: Build lighting presets with custom names and descriptions
- **MIDI Triggers**: Assign MIDI messages (notes, control changes) to trigger presets
- **Button Actions**: Configure multiple button actions per preset with support for:
  - Press actions
  - Release actions
  - Toggle actions

#### ShowXpress Controller Integration
- **TCP Connection**: Connect to ShowXpress controller via TCP/IP (default: 127.0.0.1:7348)
- **Button Discovery**: Automatically discovers and lists available buttons from the controller
- **Real-time Button Control**: Execute button actions directly through the interface

### System Requirements

- **Operating System**: Windows 10/11 (x64)
- **ShowXpress**: ShowXpress lighting control software with TCP server enabled
- **MIDI Device**: Any MIDI-compatible input device (controller, keyboard, etc.)
- **Network**: TCP/IP connection to ShowXpress controller (local or network)

### Getting Started

1. **Connect Your MIDI Device**: Plug in your MIDI device and ensure it's recognized.
2. **Launch the Application**: Run `midi_showxpress_controller.exe`
3. **Select MIDI Port**: Choose your MIDI input device from the MIDI Device dropdown in the MIDI Monitor panel
4. **Connect to ShowXpress**: 
   - Enter the ShowXpress controller address (default: 127.0.0.1:7348)
   - Enter the controller password if required
   - Click "Connect"
5. **Create Your First Preset**:
   - Click "New" in the Preset Builder panel
   - Enter a name and description
   - Double-click buttons in the Lighting Controller panel to add them as actions
   - Use the MIDI Monitor to view incoming MIDI messages
   - Double-click a MIDI message to add it as a trigger for the preset

### Technical Details

- **Language**: Rust
- **GUI Framework**: imgui-rs with wgpu rendering
- **MIDI Library**: midir
- **Network Protocol**: Custom TCP protocol compatible with ShowXpress TLC (The Lighting Controller)
- **Data Format**: JSON for preset storage

### Known Limitations

- Requires ShowXpress to be running with TCP server enabled
- Single MIDI input device supported at a time

### Support

For issues, feature requests, or questions, please refer to the project repository.
