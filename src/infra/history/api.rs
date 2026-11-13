use super::{HistoryCommand, PromptHistory, TelemetrySnapshot};

impl PromptHistory {
    pub fn record_telemetry(&self, conversation_id: &str, snapshot: TelemetrySnapshot) {
        if let Some(ref tx) = self.write_tx {
            let _ = tx.send(HistoryCommand::RecordTelemetry {
                conversation_id: conversation_id.to_string(),
                snapshot,
            });
        }
    }
}
