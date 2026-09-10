use anyhow::Result;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Voice {
    pub id: String,
    pub name: String,
    pub language: Option<String>,
    pub provider: String,
}

#[async_trait]
pub trait VoiceProvider: Send + Sync {
    async fn list_voices(&self) -> Result<Vec<Voice>>;
    async fn convert(&self, audio: Vec<f32>) -> Result<Vec<f32>>;

    /// Stream converted audio chunks to `tx` as they arrive.
    /// Default: calls convert() in one shot. Override for true streaming.
    async fn convert_stream(&self, audio: Vec<f32>, tx: mpsc::Sender<Vec<f32>>) -> Result<()> {
        let result = self.convert(audio).await?;
        tx.send(result).await.ok();
        Ok(())
    }

    fn name(&self) -> &str;
}
