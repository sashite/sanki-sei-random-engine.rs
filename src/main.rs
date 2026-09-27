//! `sanki-sei-random-engine` — a complete engine for the Sashité Engine
//! Interface (SEI 1.0.0, <https://sashite.dev/specs/sei/1.0.0/>) that plays
//! Sanki (`sashite.sanki.kernel/1`) by picking a legal move at random.
//!
//! It exists to be forked: everything an SEI engine must do — the JSON Lines
//! transport, the envelope, `hello`, `ping`, `configure`, `search`, `cancel`,
//! the errors and their classes, the rules document's canonical FEEN and PMN —
//! is here and tested, and the only thing it does badly is choose the move.
//! Replace [`search::choose`] with a search of your own and keep the rest.
//!
//! The process is single-threaded: every request is answered in microseconds,
//! so reading the input line by line is enough to answer `ping` "at its
//! receipt" and to interrupt an infinite search with the next request. An
//! engine that thinks for seconds needs a reader thread; see the README.

#![forbid(unsafe_code)]

mod protocol;
mod rng;
mod search;

use std::io::{self, BufRead, Write};
use std::process::ExitCode;

use protocol::{Envelope, Event, Request};

/// Lines longer than this are refused as an envelope error (SEI §5 asks every
/// receiver to accept at least 1 MiB; we accept four).
const MAX_LINE_BYTES: usize = 4 * 1024 * 1024;

/// The engine's whole state: what `hello` and `configure` established, and
/// the search left open by an infinite `search`.
struct Engine {
    /// The last `id` of a request whose envelope was valid.
    last_id: Option<i64>,
    /// Whether a `hello` has succeeded.
    opened: bool,
    /// The effective configuration (§8.3): the `seed` option.
    seed: u32,
    /// The random generator, reseeded by `fresh` searches.
    rng: rng::SplitMix64,
    /// An infinite search waiting for the next request to end it.
    pending: Option<search::Pending>,
}

fn main() -> ExitCode {
    let stdin = io::stdin();
    let stdout = io::stdout();
    let mut out = stdout.lock();
    let mut engine = Engine {
        last_id: None,
        opened: false,
        seed: 0,
        rng: rng::SplitMix64::from_entropy(),
        pending: None,
    };
    let mut line = String::new();
    loop {
        line.clear();
        // `read_line` reads to the LF whatever the length: the bound is
        // checked after, and a line beyond it is an envelope error.
        match stdin.lock().read_line(&mut line) {
            Ok(0) => return ExitCode::SUCCESS, // end of input: the host closed the session
            Ok(_) => {}
            Err(_) => {
                return fatal(
                    engine.pending.take(),
                    &mut out,
                    "invalid",
                    "invalid encoding",
                )
            }
        }
        let text = line.trim_end_matches(['\n', '\r']);
        if text.trim().is_empty() {
            continue;
        }
        if line.len() > MAX_LINE_BYTES {
            return fatal(
                engine.pending.take(),
                &mut out,
                "invalid",
                "line longer than 4 MiB",
            );
        }
        match protocol::parse(text, engine.last_id, engine.opened) {
            Err(Envelope(message)) => {
                return fatal(engine.pending.take(), &mut out, "invalid", &message)
            }
            Ok(request) => {
                engine.last_id = Some(request.id);
                if let Err(()) = handle(&mut engine, request, &mut out) {
                    return ExitCode::FAILURE;
                }
            }
        }
    }
}

/// Ends the pending search, if any, emits the fatal error (no `re`) and
/// returns the failure exit code (SEI §5, §7 rule 7).
fn fatal(
    pending: Option<search::Pending>,
    out: &mut impl Write,
    code: &str,
    message: &str,
) -> ExitCode {
    if let Some(pending) = pending {
        let _ = pending.done().write(out);
    }
    let _ = Event::fatal(code, message).write(out);
    ExitCode::FAILURE
}

/// Processes one request whose envelope is valid. `Err(())` means the output
/// could not be written: the session is over.
fn handle(engine: &mut Engine, request: Request, out: &mut impl Write) -> Result<(), ()> {
    let id = request.id;
    // `ping` stands apart: answered at once, changes nothing (§7 rule 3).
    if request.op == "ping" {
        let event = match protocol::check_ping(&request) {
            Ok(()) => Event::done(id),
            Err(error) => error.attached(id),
        };
        return event.write(out);
    }
    // Every other request first ends the active search (§7 rule 2).
    if let Some(pending) = engine.pending.take() {
        pending.done().write(out)?;
    }
    let event = match request.op.as_str() {
        "hello" => match protocol::check_hello(&request) {
            Ok(common) => {
                // The opening succeeds only when `version` is returned (§7 rule 1).
                engine.opened |= common;
                protocol::hello_done(id, common)
            }
            Err(error) => error.attached(id),
        },
        "configure" => match protocol::check_configure(&request) {
            Ok(seed) => {
                if seed != engine.seed {
                    engine.seed = seed;
                    engine.rng = rng::SplitMix64::seeded(seed);
                }
                Event::done(id)
            }
            Err(error) => error.attached(id),
        },
        "search" => match search::validate(&request) {
            Err(error) => error.attached(id),
            Ok(searched) => {
                if searched.fresh {
                    engine.rng = rng::SplitMix64::seeded(engine.seed);
                }
                let Ok((info, outcome)) = search::run(id, &searched, &mut engine.rng) else {
                    // No legal move could be produced: fatal (§8.4 Guaranteed result).
                    let _ = Event::fatal("internal", "no legal move could be produced").write(out);
                    return Err(());
                };
                if let Some(info) = info {
                    info.write(out)?;
                }
                match outcome {
                    search::Outcome::Done(done) => done,
                    search::Outcome::Infinite(pending) => {
                        engine.pending = Some(pending);
                        return Ok(());
                    }
                }
            }
        },
        "cancel" => match protocol::check_cancel(&request) {
            Ok(()) => Event::done(id),
            Err(error) => error.attached(id),
        },
        _ => protocol::Error::invalid("unknown operation", Some("/op")).attached(id),
    };
    event.write(out)
}
