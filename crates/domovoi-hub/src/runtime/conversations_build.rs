//! Starting the Operator conversation service over the store and the triage model.

use std::sync::Arc;

use domovoi_core::ports::Clock;
use domovoi_engine::PatternRedactor;
use domovoi_engine::conversations::{
    ChatModel, ConversationDeps, ConversationService, DisabledModel,
};
use domovoi_engine::triage::TriageHandle;
use domovoi_store::Store;
use tokio::task::JoinHandle;
use tokio_util::sync::CancellationToken;

use super::RuntimeError;
use crate::api::operator::live::LiveBus;
use crate::services::ConversationCommands;

/// Starts the conversation writer. Without triage the model is [`DisabledModel`], so reading
/// and managing conversations still work and a question is refused at the API.
pub(super) async fn build(
    store: &Arc<Store>,
    triage: Option<TriageHandle>,
    live: &Arc<LiveBus>,
    redactor: &Arc<PatternRedactor>,
    clock: &Arc<dyn Clock>,
    shutdown: &CancellationToken,
) -> Result<(ConversationCommands, JoinHandle<()>), RuntimeError> {
    let model: Arc<dyn ChatModel> = match triage {
        Some(handle) => Arc::new(handle),
        None => Arc::new(DisabledModel),
    };
    let (handle, task) = ConversationService::spawn(ConversationDeps {
        conversations: store.clone(),
        uow: store.clone(),
        model,
        live: live.clone(),
        redactor: redactor.clone(),
        clock: clock.clone(),
        shutdown: shutdown.clone(),
    })
    .await
    .map_err(|err| RuntimeError::Serve(format!("conversation service failed: {err}")))?;
    Ok((ConversationCommands::new(handle), task))
}
