//! The engine as an SEI host sees it: the binary is launched, spoken to on
//! its standard input, and read on its standard output, line by line.
//!
//! What is checked: the opening and its `done`; the error classes and their
//! paths; the safety-net `info` before the `done`; the infinite search ended
//! by the next request, with `ping` answered meanwhile; `roots`; the fatal
//! envelope errors and the exit codes; and complete random games on the nine
//! pairings, every `best` validated by `sashite-sanki-engine` — the same
//! checks an SEI host makes before it plays a move.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects
)]

use std::io::{BufRead, BufReader, Write};
use std::process::{Child, ChildStdin, Command, Stdio};

use sashite_sanki_engine::domain::outcome::Verdict;
use sashite_sanki_engine::domain::variant::Variant;
use sashite_sanki_engine::pmn;
use sashite_sanki_engine::position::Position;
use sashite_sanki_engine::{engine, rules};
use serde_json::{json, Value};

const RULES: &str = "sashite.sanki.kernel/1";

/// A running engine.
struct Engine {
    child: Child,
    stdin: Option<ChildStdin>,
    stdout: BufReader<std::process::ChildStdout>,
}

impl Engine {
    fn launch() -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_sanki-sei-random-engine"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .expect("the engine launches");
        let stdin = child.stdin.take();
        let stdout = BufReader::new(child.stdout.take().unwrap());
        Self {
            child,
            stdin,
            stdout,
        }
    }

    /// Sends one request line.
    fn send(&mut self, request: &Value) {
        let stdin = self.stdin.as_mut().expect("input open");
        stdin.write_all(request.to_string().as_bytes()).unwrap();
        stdin.write_all(b"\n").unwrap();
        stdin.flush().unwrap();
    }

    /// Sends a raw line, as written.
    fn send_raw(&mut self, line: &str) {
        let stdin = self.stdin.as_mut().expect("input open");
        stdin.write_all(line.as_bytes()).unwrap();
        stdin.write_all(b"\n").unwrap();
        stdin.flush().unwrap();
    }

    /// Reads one event line.
    fn read(&mut self) -> Value {
        let mut line = String::new();
        let n = self.stdout.read_line(&mut line).unwrap();
        assert!(n > 0, "the engine closed its output");
        assert!(line.ends_with('\n'), "a line ends with LF");
        serde_json::from_str(line.trim_end()).expect("an I-JSON object")
    }

    /// Reads events until the terminal event of `id`; returns every event read.
    fn until_terminal(&mut self, id: i64) -> Vec<Value> {
        let mut events = Vec::new();
        loop {
            let event = self.read();
            let terminal =
                event["re"] == json!(id) && (event["ev"] == "done" || event["ev"] == "error");
            events.push(event);
            if terminal {
                return events;
            }
        }
    }

    /// Sends `request`, returns its terminal event and the events before it.
    fn ask(&mut self, request: &Value) -> (Value, Vec<Value>) {
        self.send(request);
        let id = request["id"].as_i64().unwrap();
        let mut events = self.until_terminal(id);
        let terminal = events.pop().unwrap();
        (terminal, events)
    }

    /// Opens the session: `hello` with version 1.
    fn open(&mut self) -> Value {
        let (done, _) = self.ask(&json!({"id": 1, "op": "hello", "versions": [1]}));
        assert_eq!(done["ev"], "done");
        assert_eq!(done["version"], 1);
        done
    }

    /// Closes the input and waits; returns the exit status.
    fn close(mut self) -> std::process::ExitStatus {
        drop(self.stdin.take());
        self.child.wait().unwrap()
    }
}

fn search(id: i64, position: &str, moves: &[String], extra: Value) -> Value {
    let mut request =
        json!({"id": id, "op": "search", "rules": RULES, "position": position, "moves": moves});
    if let Value::Object(extra) = extra {
        for (k, v) in extra {
            request[k] = v;
        }
    }
    request
}

fn timed() -> Value {
    json!({"clock": {"own": {"deadline": 1000, "remaining": 60000}, "overhead": 50}})
}

// ---- opening and small requests ----------------------------------------------

#[test]
fn ping_hello_configure() {
    let mut e = Engine::launch();
    let (done, _) = e.ask(&json!({"id": 0, "op": "ping"}));
    assert_eq!(done, json!({"re": 0, "ev": "done"}));
    let hello = e.open();
    assert_eq!(hello["versions"], json!([1]));
    assert_eq!(hello["engine"]["name"], "sanki-sei-random-engine");
    assert!(hello["rules"][RULES].is_object());
    assert!(hello["features"]["roots"].is_object());
    assert_eq!(hello["options"]["seed"]["type"], "int");
    let (done, _) = e.ask(&json!({"id": 2, "op": "configure", "options": {"seed": 7}}));
    assert_eq!(done["ev"], "done");
    let (err, _) = e.ask(&json!({"id": 3, "op": "configure", "options": {"threads": 2}}));
    assert_eq!(err["code"], "invalid");
    assert_eq!(err["path"], "/options/threads");
    let (err, _) = e.ask(&json!({"id": 4, "op": "configure", "options": {"seed": -1}}));
    assert_eq!(
        (err["code"].as_str(), err["path"].as_str()),
        (Some("invalid"), Some("/options/seed"))
    );
    let (err, _) = e.ask(&json!({"id": 5, "op": "ping", "extra": 1}));
    assert_eq!(err["code"], "invalid");
    let (err, _) = e.ask(&json!({"id": 6, "op": "dance"}));
    assert_eq!(
        (err["code"].as_str(), err["path"].as_str()),
        (Some("invalid"), Some("/op"))
    );
    assert!(e.close().success());
}

#[test]
fn no_common_version_leaves_the_session_closed() {
    let mut e = Engine::launch();
    let (done, _) = e.ask(&json!({"id": 1, "op": "hello", "versions": [2]}));
    assert_eq!(done["ev"], "done");
    assert!(done.get("version").is_none());
    // Still awaiting hello: a search is an envelope error, fatal.
    e.send(&search(
        2,
        &initial(Variant::Chess, Variant::Chess),
        &[],
        timed(),
    ));
    let error = e.read();
    assert_eq!(error["ev"], "error");
    assert!(error.get("re").is_none());
    assert!(!e.close().success());
}

// ---- envelope errors ----------------------------------------------------------------

#[test]
fn envelope_errors_are_fatal() {
    for (name, lines) in [
        (
            "id not increasing",
            vec![
                r#"{"id":1,"op":"hello","versions":[1]}"#,
                r#"{"id":1,"op":"ping"}"#,
            ],
        ),
        (
            "duplicate key",
            vec![
                r#"{"id":1,"op":"hello","versions":[1]}"#,
                r#"{"id":2,"op":"ping","op":"ping"}"#,
            ],
        ),
        (
            "hello twice",
            vec![
                r#"{"id":1,"op":"hello","versions":[1]}"#,
                r#"{"id":2,"op":"hello","versions":[1]}"#,
            ],
        ),
        ("no op", vec![r#"{"id":1}"#]),
        ("not an object", vec![r#"[1,2]"#]),
        ("a fraction", vec![r#"{"id":1.5,"op":"ping"}"#]),
        ("negative id", vec![r#"{"id":-1,"op":"ping"}"#]),
    ] {
        let mut e = Engine::launch();
        for line in &lines[..lines.len() - 1] {
            e.send_raw(line);
            e.read();
        }
        e.send_raw(lines[lines.len() - 1]);
        let error = e.read();
        assert_eq!(error["ev"], "error", "{name}");
        assert_eq!(error["code"], "invalid", "{name}");
        assert!(
            error.get("re").is_none(),
            "{name}: fatal errors carry no `re`"
        );
        assert!(!e.close().success(), "{name}: non-zero exit");
    }
}

#[test]
fn blank_lines_and_cr_are_tolerated() {
    let mut e = Engine::launch();
    e.send_raw("");
    e.send_raw("   ");
    e.send_raw("{\"id\":0,\"op\":\"ping\"}\r");
    assert_eq!(e.read(), json!({"re": 0, "ev": "done"}));
    assert!(e.close().success());
}

// ---- search: errors and their classes ---------------------------------------------

fn initial(first: Variant, second: Variant) -> String {
    rules::initial_position(first, second).unwrap().to_feen()
}

#[test]
fn search_error_classes() {
    let mut e = Engine::launch();
    e.open();
    let start = initial(Variant::Chess, Variant::Ogi);
    let cases: Vec<(Value, &str, &str)> = vec![
        (
            search(
                10,
                &start,
                &[],
                json!({"clock": {"own": {"deadline": 1, "remaining": 1}}, "colour": 1}),
            ),
            "invalid",
            "/colour",
        ),
        (
            json!({"id": 11, "op": "search", "position": start}),
            "invalid",
            "/rules",
        ),
        (
            search(12, &start, &["e2-e4".into(), "*e5".into()], timed()),
            "invalid",
            "/moves/1",
        ),
        (
            search(13, &start, &[], json!({"limits": {}})),
            "invalid",
            "/limits",
        ),
        (
            search(
                14,
                &start,
                &[],
                json!({"clock": {"own": {"deadline": 1, "remaining": 1}, "opp": {"deadline": 1, "remaining": 1}}}),
            ),
            "invalid",
            "/clock/opp/deadline",
        ),
        (
            search(15, &start, &[], json!({"clock": {"own": {"remaining": 1}}})),
            "invalid",
            "/clock/own/deadline",
        ),
        (
            search(16, &start, &[], json!({"counters": {"ply": 0}})),
            "invalid",
            "/counters/ply",
        ),
        (
            search(17, &start, &[], json!({"roots": []})),
            "invalid",
            "/roots",
        ),
        (
            json!({"id": 18, "op": "search", "rules": "other/1", "position": start}),
            "unsupported",
            "/rules",
        ),
        (
            search(19, "not a feen", &[], timed()),
            "invalid",
            "/position",
        ),
        (
            search(
                20,
                "rnbqk^bnr/pppppppp/8/8/8/8/PPPPPPPP/RNBQK^BNR / M/m",
                &[],
                timed(),
            ),
            "unsupported",
            "/position",
        ),
        (
            search(21, &start, &["e2-e5".into()], timed()),
            "illegal",
            "/moves/0",
        ),
        (
            search(22, &start, &["e2-e4".into(), "e8-e7".into()], timed()),
            "illegal",
            "/moves/1",
        ),
        (
            search(
                23,
                &start,
                &[
                    "e2-e4".into(),
                    "f7-f5".into(),
                    "d1-h5".into(),
                    "a7-a5".into(),
                ],
                timed(),
            ),
            "illegal",
            "/moves/3",
        ),
        (
            search(24, &start, &[], json!({"roots": ["e2-e4", "e2-e5"]})),
            "illegal",
            "/roots/1",
        ),
        (
            search(25, &start, &[], json!({"roots": ["e2-e4", "e2-e4"]})),
            "invalid",
            "/roots/1",
        ),
    ];
    for (request, code, path) in cases {
        let id = request["id"].clone();
        let (err, before) = e.ask(&request);
        assert!(before.is_empty(), "{id}: no event before an attached error");
        assert_eq!(err["ev"], "error", "{id}");
        assert_eq!(err["code"], code, "{id}: {err}");
        assert_eq!(err["path"], path, "{id}: {err}");
        assert_eq!(err["re"], id);
    }
    assert!(e.close().success());
}

#[test]
fn a_non_canonical_spelling_is_illegal() {
    let mut e = Engine::launch();
    e.open();
    // Castling spelled as a quiet move: legal content, wrong string.
    let position = "4k^3/8/8/8/8/8/8/4K^2+R / W/w";
    let (err, _) = e.ask(&search(2, position, &["e1-g1".into()], timed()));
    assert_eq!(
        (err["code"].as_str(), err["path"].as_str()),
        (Some("illegal"), Some("/moves/0"))
    );
    let (done, _) = e.ask(&search(3, position, &["e1~g1".into()], timed()));
    assert_eq!(done["ev"], "done");
    assert!(e.close().success());
}

#[test]
fn moves_after_a_terminal_position_are_illegal_and_a_terminal_search_answers_null() {
    let mut e = Engine::launch();
    e.open();
    // Fool's mate, chess against chess.
    let start = initial(Variant::Chess, Variant::Chess);
    let mate: Vec<String> = ["f2-f3", "e7-e5", "g2-g4", "d8-h4"]
        .map(String::from)
        .into();
    let (done, before) = e.ask(&search(2, &start, &mate, timed()));
    assert_eq!(done["ev"], "done");
    assert_eq!(done["best"], Value::Null);
    assert!(before.is_empty(), "no info on a terminal position");
    let mut after = mate.clone();
    after.push("e1-f2".into());
    let (err, _) = e.ask(&search(3, &start, &after, timed()));
    assert_eq!(
        (err["code"].as_str(), err["path"].as_str()),
        (Some("illegal"), Some("/moves/4"))
    );
    let (err, _) = e.ask(&search(4, &start, &mate, json!({"roots": ["e1-f2"]})));
    assert_eq!(err["code"], "illegal");
    assert!(e.close().success());
}

// ---- search: results ---------------------------------------------------------------

#[test]
fn info_precedes_done_and_best_is_legal_and_canonical() {
    let mut e = Engine::launch();
    e.open();
    let start = initial(Variant::Ogi, Variant::Xiongqi);
    let (done, before) = e.ask(&search(2, &start, &[], timed()));
    assert_eq!(before.len(), 1);
    assert_eq!(before[0]["ev"], "info");
    let best = done["best"].as_str().unwrap();
    assert_eq!(before[0]["variations"][0]["pv"][0], best);
    assert_eq!(done["variations"][0]["pv"][0], best);
    let position = Position::parse(&start).unwrap();
    let mv = pmn::parse_canonical(&position, best).expect("best is legal and canonical");
    assert!(engine::legal_moves(&position).contains(&mv));
    assert!(e.close().success());
}

#[test]
fn roots_restrict_the_choice() {
    let mut e = Engine::launch();
    e.open();
    let start = initial(Variant::Chess, Variant::Chess);
    for id in 2..12 {
        let (done, _) = e.ask(&search(
            id,
            &start,
            &[],
            json!({"roots": ["a2-a3", "h2-h4"], "limits": {"depth": 1}}),
        ));
        let best = done["best"].as_str().unwrap();
        assert!(best == "a2-a3" || best == "h2-h4", "{best}");
    }
    assert!(e.close().success());
}

#[test]
fn an_infinite_search_ends_with_the_next_request_and_ping_is_answered_meanwhile() {
    let mut e = Engine::launch();
    e.open();
    let start = initial(Variant::Xiongqi, Variant::Chess);
    e.send(&search(2, &start, &[], json!({})));
    let info = e.read();
    assert_eq!(
        (info["re"].as_i64(), info["ev"].as_str()),
        (Some(2), Some("info"))
    );
    e.send(&json!({"id": 3, "op": "ping"}));
    assert_eq!(
        e.read(),
        json!({"re": 3, "ev": "done"}),
        "ping answered during the search"
    );
    e.send(&json!({"id": 4, "op": "cancel"}));
    let done = e.read();
    assert_eq!(
        (done["re"].as_i64(), done["ev"].as_str()),
        (Some(2), Some("done")),
        "the search ends first"
    );
    assert_eq!(done["best"], info["variations"][0]["pv"][0]);
    assert_eq!(e.read(), json!({"re": 4, "ev": "done"}));
    // A second infinite search, ended by another search.
    e.send(&search(5, &start, &[], json!({})));
    e.read();
    e.send(&search(6, &start, &[], timed()));
    assert_eq!(e.read()["re"], 5);
    let events = e.until_terminal(6);
    assert_eq!(events.last().unwrap()["ev"], "done");
    // An infinite search left open at the end of the input: exit 0.
    e.send(&search(7, &start, &[], json!({})));
    e.read();
    assert!(e.close().success());
}

#[test]
fn a_seeded_fresh_search_replays() {
    let first = seeded_bests(7);
    let second = seeded_bests(7);
    assert_eq!(first, second);
    let third = seeded_bests(8);
    assert_ne!(
        first, third,
        "another seed gives another sequence (with overwhelming probability)"
    );
}

fn seeded_bests(seed: u32) -> Vec<String> {
    let mut e = Engine::launch();
    e.open();
    let (done, _) = e.ask(&json!({"id": 2, "op": "configure", "options": {"seed": seed}}));
    assert_eq!(done["ev"], "done");
    let start = initial(Variant::Ogi, Variant::Ogi);
    let mut bests = Vec::new();
    let mut moves: Vec<String> = Vec::new();
    for i in 0..12 {
        let mut extra = timed();
        if i == 0 {
            extra["fresh"] = json!(true);
        }
        let (done, _) = e.ask(&search(3 + i, &start, &moves, extra));
        let best = done["best"].as_str().unwrap().to_owned();
        moves.push(best.clone());
        bests.push(best);
    }
    assert!(e.close().success());
    bests
}

#[test]
fn random_games_on_the_nine_pairings() {
    let variants = [Variant::Chess, Variant::Ogi, Variant::Xiongqi];
    let mut e = Engine::launch();
    e.open();
    let mut id = 2;
    for first in variants {
        for second in variants {
            let start = initial(first, second);
            let mut position = Position::parse(&start).unwrap();
            let mut moves: Vec<String> = Vec::new();
            for _ in 0..80 {
                let (done, before) = e.ask(&search(id, &start, &moves, timed()));
                id += 1;
                assert_eq!(done["ev"], "done", "{done}");
                let Some(best) = done["best"].as_str() else {
                    // Terminal: the engine agrees with the rules.
                    assert!(
                        !matches!(engine::status(&position), Verdict::Ongoing)
                            || moves.len() >= 100
                    );
                    break;
                };
                assert_eq!(before.len(), 1, "one info, the safety net");
                let mv =
                    pmn::parse_canonical(&position, best).unwrap_or_else(|e| panic!("{best}: {e}"));
                assert!(
                    engine::legal_moves(&position).contains(&mv),
                    "{best} is legal"
                );
                position = engine::apply(&position, &mv).unwrap();
                moves.push(best.to_owned());
            }
        }
    }
    assert!(e.close().success());
}

// ---- precedence, content numbers, encoding ----------------------------------------

#[test]
fn one_defect_is_reported_in_seis_order() {
    let mut e = Engine::launch();
    e.open();
    let start = initial(Variant::Chess, Variant::Ogi);
    // invalid before unsupported: a malformed move beside unknown rules.
    let (err, _) = e.ask(
        &json!({"id": 2, "op": "search", "rules": "other/1", "position": start, "moves": ["e2e4"]}),
    );
    assert_eq!(
        (err["code"].as_str(), err["path"].as_str()),
        (Some("invalid"), Some("/moves/0"))
    );
    // invalid before unsupported: a malformed FEEN beside unknown rules.
    let (err, _) = e.ask(&json!({"id": 3, "op": "search", "rules": "other/1", "position": "nope"}));
    assert_eq!(
        (err["code"].as_str(), err["path"].as_str()),
        (Some("invalid"), Some("/position"))
    );
    // invalid before illegal: a drop without its piece after an illegal move.
    let (err, _) = e.ask(&search(4, &start, &["e2-e5".into(), "*e5".into()], timed()));
    assert_eq!(
        (err["code"].as_str(), err["path"].as_str()),
        (Some("invalid"), Some("/moves/1"))
    );
    // invalid before illegal: a repeated root on a terminal position.
    let mate: Vec<String> = ["f2-f3", "e7-e5", "g2-g4", "d8-h4"]
        .map(String::from)
        .into();
    let cc = initial(Variant::Chess, Variant::Chess);
    let (err, _) = e.ask(&search(5, &cc, &mate, json!({"roots": ["a2-a3", "a2-a3"]})));
    assert_eq!(
        (err["code"].as_str(), err["path"].as_str()),
        (Some("invalid"), Some("/roots/1"))
    );
    let (err, _) = e.ask(&search(6, &cc, &mate, json!({"roots": ["a2-a3"]})));
    assert_eq!(
        (err["code"].as_str(), err["path"].as_str()),
        (Some("illegal"), Some("/roots/0"))
    );
    // unsupported before illegal: unknown rules beside an illegal move.
    let (err, _) = e.ask(&json!({"id": 7, "op": "search", "rules": "other/1", "position": start, "moves": ["e2-e5"]}));
    assert_eq!(
        (err["code"].as_str(), err["path"].as_str()),
        (Some("unsupported"), Some("/rules"))
    );
    assert!(e.close().success());
}

#[test]
fn a_fraction_in_a_content_field_is_attached_not_fatal() {
    let mut e = Engine::launch();
    e.open();
    let start = initial(Variant::Chess, Variant::Chess);
    e.send_raw(&format!(
        r#"{{"id":2,"op":"search","rules":"{RULES}","position":"{start}","limits":{{"depth":1.5}}}}"#
    ));
    let err = e.read();
    assert_eq!(
        (
            err["re"].as_i64(),
            err["code"].as_str(),
            err["path"].as_str()
        ),
        (Some(2), Some("invalid"), Some("/limits/depth"))
    );
    e.send_raw(r#"{"id":3,"op":"hello","versions":[1e0]}"#);
    let err = e.read();
    assert_eq!(err["ev"], "error");
    assert!(
        err.get("re").is_none(),
        "a second hello is an envelope error whatever its fields"
    );
    assert!(!e.close().success());
}

#[test]
fn invalid_encoding_is_fatal() {
    let mut e = Engine::launch();
    e.open();
    let stdin = e.stdin.as_mut().unwrap();
    stdin
        .write_all(b"{\"id\":2,\"op\":\"ping\",\"x\":\"\xff\xfe\"}\n")
        .unwrap();
    stdin.flush().unwrap();
    let err = e.read();
    assert_eq!(err["ev"], "error");
    assert!(err.get("re").is_none());
    assert!(!e.close().success());
}

// ---- the endings that depend on history -----------------------------------------------

#[test]
fn move_limit_move_cap_and_repetition() {
    let mut e = Engine::launch();
    e.open();
    let start = initial(Variant::Chess, Variant::Chess);
    let mut id = 2;
    let mut ask = |e: &mut Engine, moves: &[&str], counters: Value| -> Value {
        let moves: Vec<String> = moves.iter().map(|m| (*m).to_owned()).collect();
        let mut extra = timed();
        extra["counters"] = counters;
        let (done, _) = e.ask(&search(id, &start, &moves, extra));
        id += 1;
        assert_eq!(done["ev"], "done", "{done}");
        done["best"].clone()
    };
    // Move limit: 100 half-moves without an irreversible move.
    assert_eq!(ask(&mut e, &[], json!({"halfmove": 100})), Value::Null);
    assert!(ask(&mut e, &[], json!({"halfmove": 99})).is_string());
    assert_eq!(
        ask(&mut e, &["g1-f3"], json!({"halfmove": 99})),
        Value::Null,
        "a knight move is reversible"
    );
    assert!(
        ask(&mut e, &["e2-e4"], json!({"halfmove": 99})).is_string(),
        "a pawn move resets the counter"
    );
    // Move cap: 600 half-moves played, i.e. the next ply would be the 601st.
    assert!(ask(&mut e, &[], json!({"ply": 600})).is_string());
    assert_eq!(ask(&mut e, &[], json!({"ply": 601})), Value::Null);
    assert_eq!(ask(&mut e, &["e2-e4"], json!({"ply": 600})), Value::Null);
    // Repetition: the initial position occurs a third time after two knight shuffles.
    let shuffle = ["g1-f3", "g8-f6", "f3-g1", "f6-g8"];
    let twice: Vec<&str> = shuffle.iter().chain(shuffle.iter()).copied().collect();
    assert!(ask(&mut e, &twice[..7], json!({})).is_string());
    assert_eq!(ask(&mut e, &twice, json!({})), Value::Null);
    let mut nine: Vec<String> = twice.iter().map(|m| (*m).to_owned()).collect();
    nine.push("e2-e4".into());
    let (err, _) = e.ask(&search(id, &start, &nine, timed()));
    assert_eq!(
        (err["code"].as_str(), err["path"].as_str()),
        (Some("illegal"), Some("/moves/8"))
    );
    assert!(e.close().success());
}

// ---- hello, the permanent core --------------------------------------------------------

#[test]
fn hello_tolerates_and_refuses_what_it_should() {
    let mut e = Engine::launch();
    let (done, _) = e.ask(
        &json!({"id": 1, "op": "hello", "versions": [3, 1], "host": "nope", "future": {"x": 1}}),
    );
    assert_eq!(
        done["version"], 1,
        "unknown fields and a malformed host are ignored; 1 is common"
    );
    assert!(e.close().success());
    for versions in [json!([]), json!([0]), json!("1"), json!([1.5])] {
        let mut e = Engine::launch();
        let (err, _) = e.ask(&json!({"id": 1, "op": "hello", "versions": versions}));
        assert_eq!(err["code"], "invalid", "{versions}");
        // Still awaiting hello: another hello is allowed.
        let (done, _) = e.ask(&json!({"id": 2, "op": "hello", "versions": [1]}));
        assert_eq!(done["version"], 1);
        assert!(e.close().success());
    }
}
