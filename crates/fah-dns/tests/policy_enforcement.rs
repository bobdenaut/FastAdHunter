//! p2-06: the DNS pipeline judges each query under the querying client's
//! policy.
//!
//! Drives the real `Pipeline` with real wire-format packets, so "a different
//! verdict" means a different DNS answer, not a different enum.

use std::net::{IpAddr, Ipv4Addr};
use std::str::FromStr;
use std::sync::Arc;

use fah_config::{AssignmentConfig, DnsCacheConfig, PolicyConfig, RulesConfig};
use fah_dns::{ForwardOutcome, Forwarder, Pipeline, Transport};
use fah_rules::{ListManager, PolicySet, PolicyState};
use hickory_proto::op::{Message, Query as WireQuery, ResponseCode};
use hickory_proto::rr::{Name, RecordType};
use tokio::sync::mpsc;

/// Answers everything with NOERROR and no records — enough to tell "forwarded"
/// from "blocked", which is all these tests ask.
#[derive(Clone)]
struct StubForwarder;

impl Forwarder for StubForwarder {
    async fn forward(&self, request: &Message) -> std::io::Result<ForwardOutcome> {
        let mut response = Message::response(request.metadata.id, request.metadata.op_code);
        response.metadata.response_code = ResponseCode::NoError;
        Ok(ForwardOutcome::new(response, 0))
    }
}

fn policy(id: &str, lists: Option<Vec<&str>>, assignment: AssignmentConfig) -> PolicyConfig {
    PolicyConfig {
        id: id.to_string(),
        name: None,
        lists: lists.map(|l| l.iter().map(|s| s.to_string()).collect()),
        blocking_mode: None,
        assignments: vec![assignment],
    }
}

fn always(client: &str) -> AssignmentConfig {
    AssignmentConfig {
        client: client.to_string(),
        days: None,
        start: None,
        end: None,
    }
}

/// School nights, 21:00–07:00 UTC.
fn school_nights(client: &str) -> AssignmentConfig {
    AssignmentConfig {
        client: client.to_string(),
        days: Some("mon-fri".to_string()),
        start: Some("21:00".to_string()),
        end: Some("07:00".to_string()),
    }
}

/// A manager carrying two lists: one every policy sees, one only `kids` does.
async fn manager() -> (Arc<ListManager>, tempfile::TempDir) {
    let data_dir = tempfile::tempdir().unwrap();
    let manager = Arc::new(
        ListManager::new(
            &RulesConfig {
                refresh_hours_default: 24,
                lists: vec![],
            },
            data_dir.path().to_path_buf(),
        )
        .unwrap(),
    );
    // User rules are their own list (`user-rules`), which is what the `kids`
    // policy below enables and the default policy also carries.
    manager
        .set_user_rules("||games.example.com^\n".to_string())
        .await;
    (manager, data_dir)
}

fn query(name: &str) -> Vec<u8> {
    let mut message = Message::query();
    message.add_query(WireQuery::query(
        Name::from_str(name).unwrap(),
        RecordType::A,
    ));
    message.to_vec().unwrap()
}

/// True when the reply is a synthesized block (a null-IP answer) rather than
/// the stub upstream's empty NOERROR.
fn was_blocked(reply: &[u8]) -> bool {
    !Message::from_vec(reply).unwrap().answers.is_empty()
}

fn client(last: u8) -> IpAddr {
    IpAddr::V4(Ipv4Addr::new(192, 168, 1, last))
}

/// 2026-08-03T22:00:00Z, a Monday — inside `school_nights`.
const MONDAY_EVENING: i64 = 1_785_794_400;
/// 2026-08-01T12:00:00Z, a Saturday — outside it.
const SATURDAY_NOON: i64 = 1_785_585_600;

/// The DoD scenario: one client on a policy that carries the blocklist, one
/// client on a policy that does not — same domain, same instant, two verdicts.
#[tokio::test]
async fn two_clients_on_two_policies_get_different_verdicts_for_one_domain() {
    let (rules, _data_dir) = manager().await;

    // `kids` sees the user rules; `open` names a list that does not exist, so
    // it sees nothing — a policy is a *subset* of the configured lists.
    let policies = PolicySet::from_config(
        "UTC",
        &[
            policy("kids", Some(vec!["user-rules"]), always("192.168.1.50")),
            policy("open", Some(vec!["nothing"]), always("192.168.1.51")),
        ],
    )
    .unwrap();
    rules.set_policies(policies);
    rules.recompile().await;

    let state = Arc::new(PolicyState::default());
    state.refresh(&rules.policies(), &[]);

    let (tx, mut rx) = mpsc::channel(8);
    let pipeline = Pipeline::new(
        Arc::clone(&rules),
        StubForwarder,
        10,
        &DnsCacheConfig::default(),
        tx,
    )
    .with_policies(Arc::clone(&state));

    let raw = query("games.example.com.");

    let kids = pipeline
        .handle(&raw, client(50), Transport::Tcp)
        .await
        .unwrap();
    assert!(was_blocked(&kids), "the kids policy carries the blocklist");

    let open = pipeline
        .handle(&raw, client(51), Transport::Tcp)
        .await
        .unwrap();
    assert!(
        !was_blocked(&open),
        "the open policy enables no list, so nothing blocks"
    );

    // A client with no assignment gets the default policy, which sees every
    // enabled list — so it is blocked, like it was before Policies existed.
    let unassigned = pipeline
        .handle(&raw, client(99), Transport::Tcp)
        .await
        .unwrap();
    assert!(was_blocked(&unassigned));

    // And the log says which policy decided each one.
    let decided: Vec<Option<String>> = (0..3)
        .map(|_| match rx.try_recv().unwrap() {
            fah_model::Event::Dns(event) => event.policy.as_deref().map(str::to_string),
            other => panic!("expected a DNS event, got {other:?}"),
        })
        .collect();
    assert_eq!(
        decided,
        vec![
            Some("kids".to_string()),
            Some("open".to_string()),
            // The default policy reports nothing: there was no decision to
            // describe, which is what a zero-config deployment logs.
            None,
        ]
    );
}

/// A schedule window closing changes the verdict with nothing reconfigured and
/// no restart — only the snapshot the pipeline reads was republished.
#[tokio::test]
async fn a_schedule_boundary_flips_the_verdict_without_a_restart() {
    let (rules, _data_dir) = manager().await;
    let policies = PolicySet::from_config(
        "UTC",
        &[
            policy(
                "kids",
                Some(vec!["user-rules"]),
                school_nights("192.168.1.50"),
            ),
            // Nothing else covers the client, so outside the window it falls
            // all the way to the default.
            policy("open", Some(vec!["nothing"]), always("192.168.1.51")),
        ],
    )
    .unwrap();
    rules.set_policies(policies);
    rules.recompile().await;

    let state = Arc::new(PolicyState::default());
    let (tx, _rx) = mpsc::channel(64);
    let pipeline = Pipeline::new(
        Arc::clone(&rules),
        StubForwarder,
        10,
        &DnsCacheConfig::default(),
        tx,
    )
    .with_policies(Arc::clone(&state));

    let raw = query("games.example.com.");
    let kid = client(50);

    // Inside the window the kids policy applies. Published directly at the
    // instant under test — the ticker does exactly this with the wall clock.
    state.publish(rules.policies().active_at(MONDAY_EVENING, &[]));
    assert!(was_blocked(
        &pipeline.handle(&raw, kid, Transport::Tcp).await.unwrap()
    ));

    // Outside it, the client falls to the default policy — which here also
    // blocks, so use a client whose *only* cover is the window to see the flip.
    state.publish(rules.policies().active_at(SATURDAY_NOON, &[]));
    assert_eq!(
        state.current().len(),
        1,
        "only the always-on assignment survives the closed window"
    );
    assert_eq!(
        state.current().policy_for(kid),
        fah_model::PolicyId::DEFAULT,
        "the window is shut, so no assignment covers this client"
    );
}

/// The `open` policy is the one that makes a boundary observable: inside the
/// window it blocks, outside it does not.
#[tokio::test]
async fn a_client_scheduled_onto_a_permissive_policy_is_unblocked_only_in_the_window() {
    let (rules, _data_dir) = manager().await;
    // The default policy carries the blocklist. `open` carries nothing, and is
    // assigned only during school nights — so the client is *unblocked* inside
    // the window and blocked outside it.
    let policies = PolicySet::from_config(
        "UTC",
        &[policy(
            "open",
            Some(vec!["nothing"]),
            school_nights("192.168.1.50"),
        )],
    )
    .unwrap();
    rules.set_policies(policies);
    rules.recompile().await;

    let state = Arc::new(PolicyState::default());
    let (tx, _rx) = mpsc::channel(64);
    let pipeline = Pipeline::new(
        Arc::clone(&rules),
        StubForwarder,
        10,
        &DnsCacheConfig::default(),
        tx,
    )
    .with_policies(Arc::clone(&state));

    let raw = query("games.example.com.");
    let kid = client(50);

    state.publish(rules.policies().active_at(MONDAY_EVENING, &[]));
    assert!(
        !was_blocked(&pipeline.handle(&raw, kid, Transport::Tcp).await.unwrap()),
        "inside the window the permissive policy applies"
    );

    state.publish(rules.policies().active_at(SATURDAY_NOON, &[]));
    assert!(
        was_blocked(&pipeline.handle(&raw, kid, Transport::Tcp).await.unwrap()),
        "outside it the client falls back to the default policy, which blocks"
    );
}

/// A `$client` rule applies to the client it names and nobody else.
#[tokio::test]
async fn a_client_scoped_rule_only_blocks_that_client() {
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
    rules
        .set_user_rules("||games.example.com^$client=192.168.1.50\n".to_string())
        .await;

    let (tx, _rx) = mpsc::channel(8);
    let pipeline = Pipeline::new(
        Arc::clone(&rules),
        StubForwarder,
        10,
        &DnsCacheConfig::default(),
        tx,
    );

    let raw = query("games.example.com.");
    assert!(was_blocked(
        &pipeline
            .handle(&raw, client(50), Transport::Tcp)
            .await
            .unwrap()
    ));
    assert!(!was_blocked(
        &pipeline
            .handle(&raw, client(51), Transport::Tcp)
            .await
            .unwrap()
    ));
}

/// A rule naming a client by name fires once the registry knows that name —
/// the snapshot carries it, so the pipeline never looks a name up per query.
#[tokio::test]
async fn a_name_scoped_rule_follows_the_named_client() {
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
    rules
        .set_user_rules("||games.example.com^$client=laptop\n".to_string())
        .await;
    assert!(
        rules.matcher().has_named_client_scopes(),
        "the ruleset must report that a name could matter"
    );

    let state = Arc::new(PolicyState::default());
    let (tx, _rx) = mpsc::channel(8);
    let pipeline = Pipeline::new(
        Arc::clone(&rules),
        StubForwarder,
        10,
        &DnsCacheConfig::default(),
        tx,
    )
    .with_policies(Arc::clone(&state));

    let raw = query("games.example.com.");

    // Nobody named yet: the rule names a client that does not exist.
    assert!(!was_blocked(
        &pipeline
            .handle(&raw, client(50), Transport::Tcp)
            .await
            .unwrap()
    ));

    // The registry now knows .50 as "laptop".
    state.refresh(&rules.policies(), &[(client(50), Arc::from("laptop"))]);
    assert!(was_blocked(
        &pipeline
            .handle(&raw, client(50), Transport::Tcp)
            .await
            .unwrap()
    ));
    assert!(!was_blocked(
        &pipeline
            .handle(&raw, client(51), Transport::Tcp)
            .await
            .unwrap()
    ));
}
