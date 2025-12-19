use anyhow::{anyhow, Result};
use roxmltree::Document;
use std::collections::VecDeque;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time::{timeout, Duration};

use crate::models::Button;

/// All messages TLC can send to the external client
#[derive(Debug)]
pub enum LiveMessage {
    Connected(String),
    Error(String),
    BeatOn,
    BeatOff,
    ButtonPress(String),
    ButtonRelease(String),
    FaderChange { index: u32, value: i32 },
    InterfaceChange(String),
    Bpm(f32),
    ButtonList(Vec<Button>),
    Unknown(String),
    Ok,
}

/// Parser for TCP messages from TLC
pub struct LiveParser {
    buffer: Vec<u8>,
    messages: VecDeque<LiveMessage>,
}

impl LiveParser {
    pub fn new() -> Self {
        Self {
            buffer: Vec::new(),
            messages: VecDeque::new(),
        }
    }

    /// Feed raw TCP data into parser
    pub fn feed(&mut self, data: &[u8]) {
        self.buffer.extend_from_slice(data);

        loop {
            // Handle regular line-based messages
            if let Some(pos) = self.buffer.windows(2).position(|w| w == b"\r\n") {
                let line = String::from_utf8_lossy(&self.buffer[..pos]).to_string();
                self.buffer.drain(..pos + 2);
                self.parse_line(&line);
            } else {
                // No complete line yet
                break;
            }
        }
    }

    fn parse_line(&mut self, line: &str) {
        const SEPARATOR: char = '|';

        if line.starts_with("HELLO") {
            let app_name = line.split(SEPARATOR).nth(1).unwrap_or("").to_string();
            self.messages.push_back(LiveMessage::Connected(app_name));
            return;
        }

        if line.starts_with("ERROR|") {
            self.messages.push_back(LiveMessage::Error(line[6..].to_string()));
            return;
        }

        if line.starts_with("BEAT_ON") {
            self.messages.push_back(LiveMessage::BeatOn);
            return;
        }

        if line.starts_with("BEAT_OFF") {
            self.messages.push_back(LiveMessage::BeatOff);
            return;
        }

        if line.starts_with("BUTTON_LIST|") {
            // BUTTON_LIST| contains the XML directly after the separator
            if let Some(xml_str) = line.split('|').nth(1) {
                let xml_bytes = xml_str.as_bytes();
                self.messages.push_back(LiveMessage::ButtonList(parse_buttons(xml_bytes)));
            }
            return;
        }

        if line.starts_with("BUTTON_PRESS|") {
            let name = line.split(SEPARATOR).nth(1).unwrap_or("").to_string();
            self.messages.push_back(LiveMessage::ButtonPress(name));
            return;
        }

        if line.starts_with("BUTTON_RELEASE|") {
            let name = line.split(SEPARATOR).nth(1).unwrap_or("").to_string();
            self.messages.push_back(LiveMessage::ButtonRelease(name));
            return;
        }

        if line.starts_with("FADER_CHANGE|") {
            let parts: Vec<_> = line.split(SEPARATOR).collect();
            if let (Some(idx), Some(val)) = (parts.get(1), parts.get(2)) {
                if let (Ok(idx), Ok(val)) = (idx.parse(), val.parse()) {
                    self.messages.push_back(LiveMessage::FaderChange { index: idx, value: val });
                    return;
                }
            }
        }

        if line.starts_with("INTERFACE_CHANGE|") {
            let data = line.split(SEPARATOR).nth(1).unwrap_or("").to_string();
            self.messages.push_back(LiveMessage::InterfaceChange(data));
            return;
        }

        if line.starts_with("BPM|") {
            if let Ok(bpm) = line[4..].parse::<f32>() {
                self.messages.push_back(LiveMessage::Bpm(bpm));
                return;
            }
        }

        if line == "OK" {
            self.messages.push_back(LiveMessage::Ok);
            return;
        }

        self.messages.push_back(LiveMessage::Unknown(line.to_string()));
    }

    /// Return the next parsed message, if any
    pub fn next_message(&mut self) -> Option<LiveMessage> {
        self.messages.pop_front()
    }
}

/// Parse BUTTON_LIST XML into Button structs
fn parse_buttons(xml: &[u8]) -> Vec<Button> {
    let xml_str = match std::str::from_utf8(xml) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let doc = match Document::parse(xml_str) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };

    doc.descendants()
        .filter(|n| n.has_tag_name("button"))
        .filter_map(|n| {
            Some(Button {
                id: n.attribute("index")?.parse().ok()?,
                name: n.text().unwrap_or("").trim().to_string(),
            })
        })
        .collect()
}

/// TCP client for Lighting Controller
pub struct LightingControllerClient {
    stream: TcpStream,
    parser: LiveParser,
}

impl LightingControllerClient {
    /// Connect and perform HELLO handshake
    pub async fn connect(addr: &str, password: &str) -> Result<Self> {
        let mut stream = TcpStream::connect(addr).await?;
        let mut parser = LiveParser::new();

        // Send HELLO immediately
        let hello = format!("HELLO|LightingMIDI|{}\r\n", password);
        stream.write_all(hello.as_bytes()).await?;

        // Wait for HELLO or ERROR
        'handshake: loop {
            let mut buf = [0u8; 1024];
            let n = timeout(Duration::from_secs(5), stream.read(&mut buf)).await??;
            if n == 0 {
                return Err(anyhow!("Connection closed"));
            }

            parser.feed(&buf[..n]);
            while let Some(msg) = parser.next_message() {
                match msg {
                    LiveMessage::Connected(_) => break 'handshake,
                    LiveMessage::Error(e) => return Err(anyhow!("HELLO failed: {}", e)),
                    _ => continue,
                }
            }
        }

        Ok(Self { stream, parser })
    }

    async fn send(&mut self, cmd: &str) -> Result<()> {
        self.stream
            .write_all(format!("{}\r\n", cmd).as_bytes())
            .await?;
        Ok(())
    }

    /// Read next parsed message from TLC
    pub async fn read_message(&mut self) -> Result<LiveMessage> {
        loop {
            if let Some(msg) = self.parser.next_message() {
                 // If server is asking for BPM, reply with default 120 if not set
                if let LiveMessage::Bpm(_) = &msg {
                    // ignore incoming value; just respond with current/default BPM
                    self.send_bpm().await?;
                }
                return Ok(msg);
            }

            let mut buf = [0u8; 4096];
            let n = timeout(Duration::from_secs(5), self.stream.read(&mut buf)).await??;
            if n == 0 {
                return Err(anyhow!("Connection closed"));
            }
            self.parser.feed(&buf[..n]);
        }
    }

    pub async fn send_bpm(&mut self) -> Result<()> {
        self.send(&format!("BPM|{}", 120.0)).await
    }

    /// Request and retrieve button list
    pub async fn button_list(&mut self) -> Result<Vec<Button>> {
        self.send("BUTTON_LIST").await?;
        loop {
            match self.read_message().await? {
                LiveMessage::ButtonList(list) => return Ok(list),
                LiveMessage::Error(e) => return Err(anyhow!("Error: {}", e)),
                _ => continue,
            }
        }
    }

    pub async fn button_press(&mut self, name: &str) -> Result<()> {
        self.send(&format!("BUTTON_PRESS|{}", name)).await?;
        loop {
            match self.read_message().await? {
                LiveMessage::Ok => return Ok(()),
                LiveMessage::Error(e) => return Err(anyhow!("Error: {}", e)),
                _ => continue,
            }
        }
    }

    pub async fn button_release(&mut self, name: &str) -> Result<()> {
        self.send(&format!("BUTTON_RELEASE|{}", name)).await?;
        loop {
            match self.read_message().await? {
                LiveMessage::Ok => return Ok(()),
                LiveMessage::Error(e) => return Err(anyhow!("Error: {}", e)),
                _ => continue,
            }
        }
    }

    pub async fn button_toggle(&mut self, name: &str) -> Result<()> {
        self.send(&format!("CUE|{}", name)).await?;
        loop {
            match self.read_message().await? {
                LiveMessage::Ok => return Ok(()),
                LiveMessage::Error(e) => return Err(anyhow!("Error: {}", e)),
                _ => continue,
            }
        }
    }
}
