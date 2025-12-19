use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Preset {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub triggers: Vec<MidiTrigger>,
    pub actions: Vec<ButtonAction>,
}

impl Preset {
    pub fn new(name: String, description: String) -> Self {
        Self {
            id: Uuid::new_v4(),
            name,
            description,
            triggers: Vec::new(),
            actions: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MidiTrigger {
    NoteOn { channel: u8, note: u8 },
    NoteOff { channel: u8, note: u8 },
    ControlChange { channel: u8, cc: u8, value: Option<u8> },
}

impl MidiTrigger {
    pub fn from_message(msg: &MidiMessage) -> Option<Self> {
        match msg {
            MidiMessage::NoteOn(n) => Some(MidiTrigger::NoteOn {
                channel: n.channel,
                note: n.note,
            }),
            MidiMessage::NoteOff(n) => Some(MidiTrigger::NoteOff {
                channel: n.channel,
                note: n.note,
            }),
            MidiMessage::ControlChange { channel, cc, .. } => {
                Some(MidiTrigger::ControlChange {
                    channel: *channel,
                    cc: *cc,
                    value: None,
                })
            }
        }
    }

    pub fn matches(&self, msg: &MidiMessage) -> bool {
        match (self, msg) {
            (
                MidiTrigger::NoteOn { channel: c1, note: n1 },
                MidiMessage::NoteOn(MidiNote { channel: c2, note: n2, .. }),
            ) => c1 == c2 && n1 == n2,
            (
                MidiTrigger::NoteOff { channel: c1, note: n1 },
                MidiMessage::NoteOff(MidiNote { channel: c2, note: n2, .. }),
            ) => c1 == c2 && n1 == n2,
            (
                MidiTrigger::ControlChange { channel: c1, cc: cc1, value },
                MidiMessage::ControlChange { channel: c2, cc: cc2, value: v2 },
            ) => c1 == c2 && cc1 == cc2 && value.map_or(true, |v| v == *v2),
            _ => false,
        }
    }

    pub fn display_name(&self) -> String {
        match self {
            MidiTrigger::NoteOn { channel, note } => {
                format!("Note On Ch{} N{} ({})", channel, note, note_name(*note))
            }
            MidiTrigger::NoteOff { channel, note } => {
                format!("Note Off Ch{} N{} ({})", channel, note, note_name(*note))
            }
            MidiTrigger::ControlChange { channel, cc, value } => {
                if let Some(v) = value {
                    format!("CC{} Ch{} = {}", cc, channel, v)
                } else {
                    format!("CC{} Ch{} (any)", cc, channel)
                }
            }
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ButtonActionType {
    Press,
    Release,
    Toggle,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ButtonAction {
    pub button_id: u32,
    pub button_name: String,
    pub action: ButtonActionType,
    pub delay_secs: f32,
}

#[derive(Debug, Clone)]
pub struct MidiNote {
    pub channel: u8,
    pub note: u8,
    pub velocity: u8,
}

#[derive(Debug, Clone)]
pub enum MidiMessage {
    NoteOn(MidiNote),
    NoteOff(MidiNote),
    ControlChange { channel: u8, cc: u8, value: u8 },
}

impl MidiMessage {
    pub fn from_raw(data: &[u8]) -> Option<Self> {
        if data.len() < 3 {
            return None;
        }

        let status = data[0];
        let message_type = status & 0xF0;
        let channel = status & 0x0F;

        match message_type {
            0x90 => {
                // Note On
                let velocity = data[2];
                if velocity == 0 {
                    // Velocity 0 is Note Off
                    Some(MidiMessage::NoteOff(MidiNote {
                        channel,
                        note: data[1],
                        velocity: 0,
                    }))
                } else {
                    Some(MidiMessage::NoteOn(MidiNote {
                        channel,
                        note: data[1],
                        velocity,
                    }))
                }
            }
            0x80 => {
                // Note Off
                Some(MidiMessage::NoteOff(MidiNote {
                    channel,
                    note: data[1],
                    velocity: data[2],
                }))
            }
            0xB0 => {
                // Control Change
                Some(MidiMessage::ControlChange {
                    channel,
                    cc: data[1],
                    value: data[2],
                })
            }
            _ => None,
        }
    }

    pub fn display_name(&self) -> String {
        match self {
            MidiMessage::NoteOn(n) => {
                format!("Note On Ch{} N{} V{}", n.channel, n.note, n.velocity)
            }
            MidiMessage::NoteOff(n) => {
                format!("Note Off Ch{} N{} V{}", n.channel, n.note, n.velocity)
            }
            MidiMessage::ControlChange { channel, cc, value } => {
                format!("CC{} Ch{} = {}", cc, channel, value)
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct Button {
    pub id: u32,
    pub name: String,
}

pub struct MidiLearnState {
    pub active: bool,
    pub captured: Option<MidiTrigger>,
}

impl MidiLearnState {
    pub fn new() -> Self {
        Self {
            active: false,
            captured: None,
        }
    }

    pub fn capture(&mut self, msg: &MidiMessage) {
        if !self.active {
            return;
        }

        self.captured = MidiTrigger::from_message(msg);
        self.active = false;
    }
}

fn note_name(note: u8) -> &'static str {
    const NAMES: [&str; 12] = [
        "C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B",
    ];
    NAMES[(note % 12) as usize]
}
