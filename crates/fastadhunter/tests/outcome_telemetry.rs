use std::net::{IpAddr, Ipv4Addr};
use std::str::FromStr;
use std::sync::Arc;

use fah_config::{DnsCacheConfig, RulesConfig};
use fah_dns::{ForwardOutcome, Forwarder, Pipeline, Transport};
use fah_metrics::Metrics;
use fah_model::Event;
use fah_rules::ListManager;
use hickory_proto::op::{Message, Query as WireQuery, ResponseCode};
use hickory_proto::rr::{Name, RecordType};

#[derive(Clone, Copy)]
enum Answer {
    Rcode(ResponseCode),
    TransportFailure,
}

#[derive(Clone)]
struct ScriptedForwarder {
    answer: Answer,
    endpoint: u8,
}

impl Forwarder for ScriptedForwarder {
    async fn forward(&self, request: &Message) -> std::io::Result<ForwardOutcome> {
        match self.answer {
            Answer::TransportFailure => Err(std::io::Error::other("upstream down")),
            Answer::Rcode(code) => {
                let mut response = Message::response(request.metadata.id, request.metadata.op_code);
                response.queries = request.queries.clone();
                response.metadata.response_code = code;
                Ok(ForwardOutcome::new(response, self.endpoint))
            }
        }
    }
}

fn encode_query(domain: &str) -> Vec<u8> {
    let mut message = Message::query();
    message.add_query(WireQuery::query(
        Name::from_str(domain).expect("valid domain"),
        RecordType::A,
    ));
    message.to_vec().expect("encodable query")
}

async fn manager(data_dir: &std::path::Path) -> Arc<ListManager> {
    Arc::new(
        ListManager::new(
            &RulesConfig {
                refresh_hours_default: 24,
                lists: vec![],
            },
            data_dir.to_path_buf(),
        )
        .expect("list manager"),
    )
}

async fn telemetry_after_one_query(answer: Answer, endpoint: u8) -> fah_model::EngineTelemetry {
    let data_dir = tempfile::tempdir().expect("tempdir");
    let (tx, mut rx) = tokio::sync::mpsc::channel(8);
    let pipeline = Pipeline::new(
        manager(data_dir.path()).await,
        ScriptedForwarder { answer, endpoint },
        10,
        &DnsCacheConfig::default(),
        fah_dns::DEFAULT_REFRESH_CLAIM_LEASE,
        tx,
    );

    pipeline
        .handle(
            &encode_query("example.com."),
            IpAddr::V4(Ipv4Addr::new(192, 168, 1, 10)),
            Transport::Tcp,
        )
        .await
        .expect("the pipeline must answer");

    let metrics = Metrics::new();
    match rx.try_recv().expect("the pipeline must emit one event") {
        Event::Dns(event) => metrics.record(&event),
        Event::Http(_) | Event::HttpsSni(_) | Event::Https(_) => {
            panic!("the DNS pipeline emitted an HTTP event")
        }
    }
    metrics.engine_telemetry()
}

#[tokio::test]
async fn a_synthesized_servfail_reaches_its_own_counter() {
    let counters = telemetry_after_one_query(Answer::TransportFailure, 0)
        .await
        .counters
        .dns;
    assert_eq!(counters.answers.servfail_synthesized, 1);
    assert_eq!(counters.answers.servfail_relayed, 0);
    assert_eq!(counters.answers.refused_relayed, 0);
    assert_eq!(counters.pass, 1);
    assert_eq!(
        counters.cache_hits + counters.cache_misses,
        counters.pass + counters.allow
    );
}

#[tokio::test]
async fn a_relayed_servfail_reaches_its_own_counter() {
    let counters = telemetry_after_one_query(Answer::Rcode(ResponseCode::ServFail), 0)
        .await
        .counters
        .dns;
    assert_eq!(counters.answers.servfail_relayed, 1);
    assert_eq!(counters.answers.servfail_synthesized, 0);
    assert_eq!(counters.answers.refused_relayed, 0);
}

#[tokio::test]
async fn a_relayed_refused_reaches_its_own_counter() {
    let counters = telemetry_after_one_query(Answer::Rcode(ResponseCode::Refused), 1)
        .await
        .counters
        .dns;
    assert_eq!(counters.answers.refused_relayed, 1);
    assert_eq!(counters.answers.servfail_synthesized, 0);
    assert_eq!(counters.answers.servfail_relayed, 0);
}

#[tokio::test]
async fn an_ordinary_answer_moves_none_of_the_failure_counters() {
    let counters = telemetry_after_one_query(Answer::Rcode(ResponseCode::NoError), 0)
        .await
        .counters
        .dns;
    assert_eq!(counters.answers, fah_model::AnswerCounters::default());
    assert_eq!(counters.pass, 1);
}
