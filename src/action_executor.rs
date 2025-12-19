use anyhow::Result;
use std::time::Duration;
use tokio::sync::mpsc;

use crate::models::{Button, ButtonAction, ButtonActionType, MidiMessage, Preset};
use crate::tcp_client::LightingControllerClient;

pub enum ActionCommand {
    ExecutePreset(Preset),
    ExecuteSingle(ButtonAction),
    ConnectionSuccess(Vec<Button>),
    ConnectionError(String),
}

pub struct ActionExecutor {
    client: Option<LightingControllerClient>,
    rx: mpsc::UnboundedReceiver<ActionCommand>,
}

impl ActionExecutor {
    pub fn new(rx: mpsc::UnboundedReceiver<ActionCommand>) -> Self {
        Self { client: None, rx }
    }

    pub async fn connect(&mut self, addr: &str) -> Result<()> {
        self.client = Some(LightingControllerClient::connect(addr).await?);
        Ok(())
    }

    pub async fn run(&mut self) {
        while let Some(cmd) = self.rx.recv().await {
            if let Err(e) = self.handle_command(cmd).await {
                eprintln!("Action executor error: {}", e);
            }
        }
    }

    async fn handle_command(&mut self, cmd: ActionCommand) -> Result<()> {
        match cmd {
            ActionCommand::ExecutePreset(preset) => {
                self.execute_actions(&preset.actions).await?;
            }
            ActionCommand::ExecuteSingle(action) => {
                self.execute_action(&action).await?;
            }
        }
        Ok(())
    }

    async fn execute_actions(&mut self, actions: &[ButtonAction]) -> Result<()> {
        for action in actions {
            if action.delay_secs > 0.0 {
                tokio::time::sleep(Duration::from_secs_f32(action.delay_secs)).await;
            }

            self.execute_action(action).await?;
        }
        Ok(())
    }

    async fn execute_action(&mut self, action: &ButtonAction) -> Result<()> {
        let client = self
            .client
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("Not connected"))?;

        match action.action {
            ButtonActionType::Press => client.button_press(action.button_id).await?,
            ButtonActionType::Release => client.button_release(action.button_id).await?,
            ButtonActionType::Toggle => client.button_toggle(action.button_id).await?,
        }

        Ok(())
    }
}

pub struct PresetMatcher {
    presets: Vec<Preset>,
    action_tx: mpsc::UnboundedSender<ActionCommand>,
}

impl PresetMatcher {
    pub fn new(
        presets: Vec<Preset>,
        action_tx: mpsc::UnboundedSender<ActionCommand>,
    ) -> Self {
        Self { presets, action_tx }
    }

    pub fn update_presets(&mut self, presets: Vec<Preset>) {
        self.presets = presets;
    }

    pub fn handle_midi(&self, msg: &MidiMessage) {
        for preset in &self.presets {
            for trigger in &preset.triggers {
                if trigger.matches(msg) {
                    let _ = self
                        .action_tx
                        .send(ActionCommand::ExecutePreset(preset.clone()));
                    break; // Only trigger once per preset
                }
            }
        }
    }
}