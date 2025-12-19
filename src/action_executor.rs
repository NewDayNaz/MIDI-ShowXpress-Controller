use anyhow::Result;
use std::sync::Arc;
use tokio::sync::{mpsc, Mutex};
use tokio::time::Duration;

use crate::models::{Button, ButtonAction, ButtonActionType, MidiMessage, Preset};
use crate::tcp_client::LightingControllerClient;

pub enum ActionCommand {
    ExecutePreset(Preset),
    ExecuteSingle(ButtonAction),
    ConnectionSuccess(Vec<Button>),
    ConnectionError(String),
    Connect(String, String),
    Disconnect,
}

pub struct ActionExecutor {
    client: Option<Arc<Mutex<LightingControllerClient>>>,
    rx: mpsc::UnboundedReceiver<ActionCommand>,
    tx: mpsc::UnboundedSender<ActionCommand>,
}

impl ActionExecutor {
    pub fn new(
        rx: mpsc::UnboundedReceiver<ActionCommand>,
        tx: mpsc::UnboundedSender<ActionCommand>,
    ) -> Self {
        Self { client: None, rx, tx }
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
            ActionCommand::Connect(addr, password) => {
                match LightingControllerClient::connect(&addr, &password).await {
                    Ok(client) => {
                        let client_ref = Arc::new(Mutex::new(client));
                        self.client = Some(Arc::clone(&client_ref));

                        let tx_clone = self.tx.clone();
                        let client_ref_clone = Arc::clone(&client_ref);

                        // Immediately fetch button list
                        match client_ref_clone.lock().await.button_list().await {
                            Ok(buttons) => {
                                let _ = tx_clone.send(ActionCommand::ConnectionSuccess(buttons));
                            }
                            Err(e) => {
                                let _ = tx_clone.send(ActionCommand::ConnectionError(e.to_string()));
                            }
                        }

                        // Start periodic refresh
                        tokio::spawn(async move {
                            loop {
                                tokio::time::sleep(Duration::from_secs(10)).await;

                                let mut client_guard = client_ref.lock().await;
                                if let Err(e) = client_guard
                                    .button_list()
                                    .await
                                    .map(|buttons| {
                                        let _ = tx_clone.send(ActionCommand::ConnectionSuccess(buttons));
                                    })
                                {
                                    let _ = tx_clone.send(ActionCommand::ConnectionError(e.to_string()));
                                }
                            }
                        });
                    }
                    Err(e) => {
                        let _ = self.tx.send(ActionCommand::ConnectionError(e.to_string()));
                    }
                }
            }

            ActionCommand::ExecutePreset(preset) => {
                // Wait for preset delay before executing actions
                if preset.delay_secs > 0.0 {
                    tokio::time::sleep(Duration::from_secs_f32(preset.delay_secs)).await;
                }
                self.execute_actions(&preset.actions).await?;
            }

            ActionCommand::ExecuteSingle(action) => {
                self.execute_action(&action).await?;
            }

            ActionCommand::Disconnect => {
                // Clear the client connection
                self.client = None;
                // Notify UI that we've disconnected
                let _ = self.tx.send(ActionCommand::ConnectionSuccess(Vec::new()));
            }

            ActionCommand::ConnectionSuccess(_) | ActionCommand::ConnectionError(_) => {
                println!("Connection event handled by UI thread");
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
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Not connected"))?;

        let mut client = client.lock().await;

        // Use button_name instead of numeric ID
        let button_name = &action.button_name;

        match action.action {
            ButtonActionType::Press => client.button_press(button_name).await?,
            ButtonActionType::Release => client.button_release(button_name).await?,
            ButtonActionType::Toggle => client.button_toggle(button_name).await?,
        }

        Ok(())
    }
}

pub struct PresetMatcher {
    presets: Vec<Preset>,
    action_tx: mpsc::UnboundedSender<ActionCommand>,
}

impl PresetMatcher {
    pub fn new(presets: Vec<Preset>, action_tx: mpsc::UnboundedSender<ActionCommand>) -> Self {
        Self { presets, action_tx }
    }

    pub fn update_presets(&mut self, presets: Vec<Preset>) {
        self.presets = presets;
    }

    pub fn handle_midi(&self, msg: &MidiMessage) -> Option<String> {
        for preset in &self.presets {
            for trigger in &preset.triggers {
                if trigger.matches(msg) {
                    let _ = self
                        .action_tx
                        .send(ActionCommand::ExecutePreset(preset.clone()));
                    return Some(preset.name.clone()); // Return preset name for logging
                }
            }
        }
        None
    }
}
