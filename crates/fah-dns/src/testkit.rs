use std::sync::Arc;

use fah_config::{DnsCacheConfig, RulesConfig};
use fah_rules::ListManager;
use hickory_proto::op::Message;

use crate::cache::DEFAULT_REFRESH_CLAIM_LEASE;
use crate::pipeline::Pipeline;
use crate::upstream::{ForwardOutcome, Forwarder};

#[derive(Clone)]
pub(crate) struct NullForwarder;

impl Forwarder for NullForwarder {
    async fn forward(&self, query: &Message) -> std::io::Result<ForwardOutcome> {
        Ok(ForwardOutcome::new(
            Message::response(query.metadata.id, query.metadata.op_code),
            0,
        ))
    }
}

pub(crate) fn pipeline() -> (Arc<Pipeline<NullForwarder>>, tempfile::TempDir) {
    let data_dir = tempfile::tempdir().unwrap();
    let rules = Arc::new(
        ListManager::new(
            &RulesConfig {
                refresh_hours_default: 24,
                lists: vec![],
            },
            data_dir.path().to_path_buf(),
        )
        .unwrap(),
    );
    let (events, _drop_receiver) = tokio::sync::mpsc::channel(8);
    let pipeline = Arc::new(Pipeline::new(
        rules,
        NullForwarder,
        10,
        &DnsCacheConfig::default(),
        DEFAULT_REFRESH_CLAIM_LEASE,
        events,
    ));
    (pipeline, data_dir)
}
