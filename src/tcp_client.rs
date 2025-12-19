use anyhow::{anyhow, Result};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpStream;

use crate::models::Button;

pub enum LiveMessage {
    Ok,
    Error(String),
    Event(String),
    ButtonList(Vec<Button>),
}

pub struct LiveParser {
    buffer: Vec<u8>,
}

impl LiveParser {
    pub fn new() -> Self {
        Self { buffer: Vec::new() }
    }

    pub fn feed(&mut self, data: &[u8]) -> Option<LiveMessage> {
        self.buffer.extend_from_slice(data);

        // Look for CRLF
        let pos = self
            .buffer
            .windows(2)
            .position(|w| w == b"\r\n")?;

        let line = String::from_utf8_lossy(&self.buffer[..pos]).to_string();
        let remainder = self.buffer.split_off(pos + 2);
        self.buffer = remainder;

        if line.starts_with("BUTTON_LIST|") {
            let len: usize = line.split('|').nth(1)?.parse().ok()?;
            
            // Wait for enough data
            if self.buffer.len() < len + 2 {
                // Put the line back and wait
                let mut temp = format!("{}\r\n", line).into_bytes();
                temp.extend_from_slice(&self.buffer);
                self.buffer = temp;
                return None;
            }

            let xml = self.buffer.drain(..len).collect::<Vec<_>>();
            let _ = self.buffer.drain(..2); // trailing CRLF
            return Some(LiveMessage::ButtonList(parse_buttons(&xml)));
        }

        if line == "OK" {
            return Some(LiveMessage::Ok);
        }
        if line.starts_with("ERROR|") {
            return Some(LiveMessage::Error(line[6..].to_string()));
        }
        if line.starts_with("EVENT|") {
            return Some(LiveMessage::Event(line.to_string()));
        }

        None
    }
}

fn parse_buttons(xml: &[u8]) -> Vec<Button> {
    let xml_str = match std::str::from_utf8(xml) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };

    let doc = match roxmltree::Document::parse(xml_str) {
        Ok(d) => d,
        Err(_) => return Vec::new(),
    };

    doc.descendants()
        .filter(|n| n.has_tag_name("Button"))
        .filter_map(|n| {
            Some(Button {
                id: n.attribute("id")?.parse().ok()?,
                name: n.attribute("name")?.to_string(),
            })
        })
        .collect()
}

pub struct LightingControllerClient {
    stream: TcpStream,
    parser: LiveParser,
}

impl LightingControllerClient {
    pub async fn connect(addr: &str) -> Result<Self> {
        let stream = TcpStream::connect(addr).await?;
        Ok(Self {
            stream,
            parser: LiveParser::new(),
        })
    }

    async fn send(&mut self, cmd: &str) -> Result<()> {
        self.stream
            .write_all(format!("{}\r\n", cmd).as_bytes())
            .await?;
        Ok(())
    }

    async fn read_response(&mut self) -> Result<LiveMessage> {
        loop {
            let mut buf = [0u8; 4096];
            let n = tokio::time::timeout(
                std::time::Duration::from_secs(5),
                self.stream.read(&mut buf),
            )
            .await??;

            if n == 0 {
                return Err(anyhow!("Connection closed"));
            }

            if let Some(msg) = self.parser.feed(&buf[..n]) {
                return Ok(msg);
            }
        }
    }

    pub async fn button_list(&mut self) -> Result<Vec<Button>> {
        self.send("BUTTON_LIST").await?;
        match self.read_response().await? {
            LiveMessage::ButtonList(buttons) => Ok(buttons),
            LiveMessage::Error(e) => Err(anyhow!("Error: {}", e)),
            _ => Err(anyhow!("Unexpected response")),
        }
    }

    pub async fn button_press(&mut self, id: u32) -> Result<()> {
        self.send(&format!("BUTTON_PRESS|{}", id)).await?;
        match self.read_response().await? {
            LiveMessage::Ok => Ok(()),
            LiveMessage::Error(e) => Err(anyhow!("Error: {}", e)),
            _ => Err(anyhow!("Unexpected response")),
        }
    }

    pub async fn button_release(&mut self, id: u32) -> Result<()> {
        self.send(&format!("BUTTON_RELEASE|{}", id)).await?;
        match self.read_response().await? {
            LiveMessage::Ok => Ok(()),
            LiveMessage::Error(e) => Err(anyhow!("Error: {}", e)),
            _ => Err(anyhow!("Unexpected response")),
        }
    }

    pub async fn button_toggle(&mut self, id: u32) -> Result<()> {
        self.send(&format!("BUTTON_TOGGLE|{}", id)).await?;
        match self.read_response().await? {
            LiveMessage::Ok => Ok(()),
            LiveMessage::Error(e) => Err(anyhow!("Error: {}", e)),
            _ => Err(anyhow!("Unexpected response")),
        }
    }
}
