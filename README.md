# MIDI ShowXpress Controller

[![Rust](https://img.shields.io/badge/rust-1.70+-orange.svg)](https://www.rust-lang.org/)
[![License](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Platform](https://img.shields.io/badge/platform-Windows-lightgrey.svg)](https://www.microsoft.com/windows)

A powerful bridge application that enables you to control ShowXpress lighting software using MIDI devices. Connect your MIDI controller, keyboard, or any MIDI-compatible device to trigger lighting presets and button actions in real-time.

## Table of Contents

- [Features](#features)
- [System Requirements](#system-requirements)
- [Installation](#installation)
- [Getting Started](#getting-started)
- [Usage](#usage)
- [Building from Source](#building-from-source)
- [Technical Details](#technical-details)
- [Known Limitations](#known-limitations)
- [Contributing](#contributing)
- [License](#license)

## Features

### MIDI Integration
- **Full MIDI Support**: Receive and monitor MIDI messages from any MIDI device (Note On/Off, Control Change)
- **Real-time Monitoring**: Live MIDI message display with timestamped console log
- **MIDI Learn**: Visual feedback when MIDI messages are received
- **Automatic Port Detection**: Automatically detects and lists available MIDI input devices

### Preset Management
- **Create Custom Presets**: Build lighting presets with custom names and descriptions
- **MIDI Triggers**: Assign MIDI messages (notes, control changes) to trigger presets
- **Button Actions**: Configure multiple button actions per preset with support for:
  - Press actions
  - Release actions
  - Toggle actions
- **Preset Organization**: Sort and manage multiple presets with intuitive interface

### ShowXpress Controller Integration
- **TCP Connection**: Connect to ShowXpress controller via TCP/IP (default: 127.0.0.1:7348)
- **Password Support**: Secure connection with password authentication
- **Button Discovery**: Automatically discovers and lists available buttons from the controller
- **Real-time Button Control**: Execute button actions directly through the interface
- **Automatic Reconnection**: Periodic button list refresh to stay in sync with controller

### User Interface
- **Modern GUI**: Clean, intuitive interface built with immediate mode GUI
- **Three-Panel Layout**: MIDI Monitor, Preset Builder, and Lighting Controller panels
- **Interactive Button Selection**: Click, Shift+Click, Ctrl+Click, and double-click support
- **Visual Feedback**: Color-coded MIDI messages with flash indicators

### Data Persistence
- **Automatic Saving**: Presets and configuration are automatically saved
- **Configuration Persistence**: Remembers last MIDI port, controller address, and settings
- **JSON Storage**: Human-readable preset storage format

## System Requirements

- **Operating System**: Windows 10/11 (x64)
- **ShowXpress**: ShowXpress lighting control software with TCP server enabled
- **MIDI Device**: Any MIDI-compatible input device (controller, keyboard, etc.)
- **Network**: TCP/IP connection to ShowXpress controller (local or network)

## Installation

### Pre-built Binary

1. Download the latest release from the [Releases](https://github.com/yourusername/MIDI-ShowXpress-Controller/releases) page
2. Extract the archive to your desired location
3. Run `midi_showxpress_controller.exe`

### Building from Source

See [Building from Source](#building-from-source) section below.

## Getting Started

1. **Connect Your MIDI Device**: Plug in your MIDI device and ensure it's recognized by Windows
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

## Building from Source

### Prerequisites

- [Rust](https://www.rust-lang.org/tools/install) (1.70 or later)
- [Cargo](https://doc.rust-lang.org/cargo/) (comes with Rust)

### Build Steps

```bash
# Clone the repository
git clone https://github.com/yourusername/MIDI-ShowXpress-Controller.git
cd MIDI-ShowXpress-Controller

# Build in release mode
cargo build --release

# The executable will be in target/release/midi_showxpress_controller.exe
```

## Known Limitations

- Requires ShowXpress to be running with TCP server enabled
- Single MIDI input device supported at a time

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request. For major changes, please open an issue first to discuss what you would like to change.

1. Fork the repository
2. Create your feature branch (`git checkout -b feature/AmazingFeature`)
3. Commit your changes (`git commit -m 'Add some AmazingFeature'`)
4. Push to the branch (`git push origin feature/AmazingFeature`)
5. Open a Pull Request

## License

This project is licensed under the MIT License - see the [LICENSE](LICENSE) file for details.
