//! The terminal loop: draws the view, refreshes it every ten seconds on a
//! worker thread, and quits on `q`.

use std::io::Write;
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::widgets::Paragraph;

use crate::model::{Inputs, View, build};
use crate::render::{body_capacity, draw, render_text};
use crate::source::Source;

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|elapsed| i64::try_from(elapsed.as_secs()).ok())
        .unwrap_or(0)
}

fn load(source: &Source) -> Result<(Inputs, View), String> {
    let inputs = source.load(now())?;
    let view = build(&inputs);
    Ok((inputs, view))
}

/// Loads once and renders one frame as plain text.
pub fn once(source: &Source, width: u16, height: u16) -> Result<String, String> {
    Ok(render_text(&load(source)?.1, width, height))
}

/// What the worker tells the screen.
enum Message {
    /// A load started.
    Loading,
    /// A view of the newest load, and when that load finished.
    Loaded(Box<View>, Instant),
    /// A load failed.
    Failed(String),
}

/// How often the worker looks at the board for changes.
const CHECK: Duration = Duration::from_secs(10);

/// The shortest time between two loads that read the board.
const FLOOR: Duration = Duration::from_secs(60);

/// How soon after the last load a reload may start: no sooner than 60 s and
/// no sooner than three times the last load took, so loading stays a small
/// share of the machine.
fn reload_floor(last_load: Duration) -> Duration {
    FLOOR.max(last_load * 3)
}

/// Whether a check should start a load. A forced reload (the `r` key) always
/// does. Otherwise the floor must have passed, and then a load runs when none
/// is held or the board's key moved.
fn reload_due(forced: bool, held: bool, moved: bool, since: Duration, floor: Duration) -> bool {
    forced || (since >= floor && (!held || moved))
}

/// The newest load that returned data.
struct Held {
    inputs: Inputs,
    /// The board's key when the load ran.
    key: String,
    /// When the load finished.
    at: Instant,
}

/// Drops the reload requests that piled up before a load starts, so a held key
/// does not queue a load per press. A press made during a load stays queued and
/// causes exactly one more load, which reads any change made after that load began.
fn drain(reload: &mpsc::Receiver<()>) {
    while reload.try_recv().is_ok() {}
}

/// Loads at once, then checks every `check` whether the board's key moved and
/// reloads when it did. Every load that returns data is held, degraded or not,
/// and a degraded one is retried only when the key moves or `r` is pressed.
/// A failed load keeps the held data on screen with the failure on the status
/// line. Ends when the screen stops listening.
fn work<L, K>(
    sender: &mpsc::Sender<Message>,
    reload: &mpsc::Receiver<()>,
    check: Duration,
    mut load: L,
    key: K,
) where
    L: FnMut() -> Result<(Inputs, View), String>,
    K: Fn() -> String,
{
    let mut held: Option<Held> = None;
    let mut failure: Option<String> = None;
    let mut started: Option<Instant> = None;
    let mut last_load = Duration::ZERO;
    loop {
        let forced = match reload.recv_timeout(if started.is_none() {
            Duration::ZERO
        } else {
            check
        }) {
            Ok(()) => true,
            Err(mpsc::RecvTimeoutError::Timeout) => false,
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        };
        let current = key();
        let moved = held.as_ref().is_none_or(|kept| kept.key != current);
        let since = started.map_or(Duration::MAX, |at| at.elapsed());
        let usable = held.is_some() && failure.is_none();
        if !reload_due(forced, usable, moved, since, reload_floor(last_load)) {
            if let Some(kept) = &mut held {
                kept.inputs.now = now();
                let mut view = build(&kept.inputs);
                if let Some(error) = &failure {
                    view.status = vec![error.clone()];
                }
                if sender
                    .send(Message::Loaded(Box::new(view), kept.at))
                    .is_err()
                {
                    return;
                }
            }
            continue;
        }
        if sender.send(Message::Loading).is_err() {
            return;
        }
        let begin = Instant::now();
        started = Some(begin);
        drain(reload);
        let result = load();
        last_load = begin.elapsed();
        let message = match result {
            Ok((inputs, view)) => {
                let at = Instant::now();
                held = Some(Held {
                    inputs,
                    key: current,
                    at,
                });
                failure = None;
                Message::Loaded(Box::new(view), at)
            }
            Err(error) => match &mut held {
                Some(kept) => {
                    kept.inputs.now = now();
                    let mut view = build(&kept.inputs);
                    view.status = vec![error.clone()];
                    failure = Some(error);
                    Message::Loaded(Box::new(view), kept.at)
                }
                None => Message::Failed(error),
            },
        };
        if sender.send(message).is_err() {
            return;
        }
    }
}

/// Restores the terminal when dropped, including on a panic unwinding past it.
struct Restore;

impl Drop for Restore {
    fn drop(&mut self) {
        ratatui::restore();
    }
}

fn freshness(loaded: Option<Instant>, loading: bool) -> String {
    let mut text = loaded.map_or_else(
        || "not loaded yet".to_owned(),
        |at| {
            let seconds = at.elapsed().as_secs();
            if seconds < 60 {
                format!("updated {seconds}s ago")
            } else {
                format!(
                    "updated {} ago",
                    crate::time::age(i64::try_from(seconds).unwrap_or(0))
                )
            }
        },
    );
    if loading {
        text.push_str(" · loading");
    }
    text
}

/// The first frame, before any data has arrived.
fn draw_loading(frame: &mut ratatui::Frame) {
    frame.render_widget(
        Paragraph::new(" board-top: reading the board; a cold board can take a minute · loading"),
        frame.area(),
    );
}

/// Applies one key to the view: text while the search prompt is open,
/// scrolling and `/` otherwise. No key changes a record.
fn handle_key(view: &mut View, code: KeyCode, modifiers: KeyModifiers, capacity: usize) {
    if view.typing {
        match code {
            KeyCode::Char(c) if !modifiers.contains(KeyModifiers::CONTROL) => view.type_char(c),
            KeyCode::Backspace => view.backspace(),
            KeyCode::Enter => view.keep_search(),
            KeyCode::Esc => view.clear_search(),
            _ => {}
        }
        return;
    }
    match code {
        KeyCode::Up => view.move_by(-1),
        KeyCode::Down => view.move_by(1),
        KeyCode::PageUp => view.page(capacity, false),
        KeyCode::PageDown => view.page(capacity, true),
        KeyCode::Home => view.home(),
        KeyCode::End => view.end(),
        KeyCode::Char('/') => view.start_search(),
        KeyCode::Esc => view.clear_search(),
        _ => {}
    }
}

/// Runs the full-screen view until `q` or Ctrl-C.
pub fn live(source: &Source) -> Result<(), String> {
    let mut terminal =
        ratatui::try_init().map_err(|error| format!("could not use the terminal: {error}"))?;
    let _restore = Restore;
    let (sender, receiver) = mpsc::channel::<Message>();
    let worker_source = source.clone();
    let (reload, reload_requests) = mpsc::channel::<()>();
    // The worker is not joined: quitting must not wait for a load.
    std::thread::spawn(move || {
        work(
            &sender,
            &reload_requests,
            CHECK,
            || load(&worker_source),
            || worker_source.key(),
        );
    });
    let mut view: Option<View> = None;
    let mut loaded: Option<Instant> = None;
    let mut loading = true;
    loop {
        loop {
            match receiver.try_recv() {
                Ok(Message::Loading) => loading = true,
                Ok(Message::Loaded(fresh, at)) => {
                    let mut fresh = *fresh;
                    if let Some(old) = &view {
                        fresh.keep_selection(old);
                    }
                    view = Some(fresh);
                    loaded = Some(at);
                    loading = false;
                }
                Ok(Message::Failed(error)) => {
                    return Err(error);
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    // The panic hook has already restored the terminal.
                    return Err("the refresh thread stopped".to_owned());
                }
            }
        }
        if let Some(shown) = &mut view {
            shown.freshness = freshness(loaded, loading);
        }
        let capacity = terminal.size().map_or(0, |size| body_capacity(size.height));
        if let Some(shown) = &mut view {
            shown.reveal(capacity);
        }
        let drawn = terminal.draw(|frame| match &view {
            Some(shown) => draw(frame, shown),
            None => draw_loading(frame),
        });
        if let Err(error) = drawn {
            return Err(format!("could not draw: {error}"));
        }
        match event::poll(Duration::from_millis(200)) {
            Ok(true) => match event::read() {
                Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                    let control_c = key.code == KeyCode::Char('c')
                        && key.modifiers.contains(KeyModifiers::CONTROL);
                    let typing = view.as_ref().is_some_and(|shown| shown.typing);
                    if control_c || (!typing && key.code == KeyCode::Char('q')) {
                        return Ok(());
                    }
                    if let Some(shown) = &mut view {
                        handle_key(shown, key.code, key.modifiers, capacity);
                    }
                    if !typing && key.code == KeyCode::Char('r') {
                        // The worker may have ended; the next frame returns the error.
                        let _ = reload.send(());
                    }
                }
                Ok(_) => {}
                Err(error) => return Err(format!("could not read the keyboard: {error}")),
            },
            Ok(false) => {}
            Err(error) => return Err(format!("could not poll the keyboard: {error}")),
        }
    }
}

/// Where `emit` writes.
#[derive(Debug, Clone, Copy)]
pub enum Stream {
    /// Standard output, for the `--once` frame.
    Out,
    /// Standard error, for failures and the loading note.
    Err,
}

/// Writes text and a newline to a standard stream.
#[expect(
    clippy::disallowed_methods,
    reason = "the app module owns standard output and standard error"
)]
pub fn emit(stream: Stream, text: &str) {
    let _ = match stream {
        Stream::Out => writeln!(std::io::stdout().lock(), "{text}"),
        Stream::Err => writeln!(std::io::stderr().lock(), "{text}"),
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    const MINUTE: Duration = Duration::from_secs(60);

    #[test]
    fn the_floor_is_a_minute_or_three_load_times() {
        assert_eq!(reload_floor(Duration::from_secs(5)), MINUTE);
        assert_eq!(
            reload_floor(Duration::from_secs(40)),
            Duration::from_secs(120)
        );
    }

    #[test]
    fn a_reload_waits_for_the_floor_and_for_a_moved_board() {
        let floor = MINUTE;
        let early = Duration::from_secs(20);
        let late = Duration::from_secs(70);
        assert!(!reload_due(false, true, true, early, floor));
        assert!(reload_due(false, true, true, late, floor));
        assert!(!reload_due(false, true, false, late, floor));
        assert!(reload_due(false, false, false, late, floor));
        assert!(!reload_due(false, false, false, early, floor));
    }

    #[test]
    fn the_reload_key_ignores_the_floor_and_the_board() {
        assert!(reload_due(true, true, false, Duration::ZERO, MINUTE));
    }

    fn loaded() -> (Inputs, View) {
        let inputs = Inputs::default();
        let view = build(&inputs);
        (inputs, view)
    }

    #[test]
    fn any_number_of_presses_during_a_load_cause_one_more_load() {
        let (sender, messages) = mpsc::channel::<Message>();
        let (reload, requests) = mpsc::channel::<()>();
        let calls = std::sync::atomic::AtomicUsize::new(0);
        let count = || calls.load(std::sync::atomic::Ordering::SeqCst);
        std::thread::scope(|scope| {
            let presses = reload.clone();
            let calls = &calls;
            scope.spawn(move || {
                work(
                    &sender,
                    &requests,
                    Duration::from_millis(10),
                    || {
                        if calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst) == 0 {
                            for _ in 0..30 {
                                presses.send(()).unwrap();
                            }
                        }
                        Ok(loaded())
                    },
                    String::new,
                );
            });
            while count() < 1 {
                std::thread::sleep(Duration::from_millis(5));
            }
            while count() < 2 {
                std::thread::sleep(Duration::from_millis(5));
            }
            std::thread::sleep(Duration::from_millis(200));
            assert_eq!(count(), 2, "held presses queued more than one load");
            reload.send(()).unwrap();
            while count() < 3 {
                std::thread::sleep(Duration::from_millis(5));
            }
            drop(messages);
        });
        assert_eq!(count(), 3);
    }

    #[test]
    fn a_degraded_load_is_held_and_not_retried_until_the_board_moves() {
        let (sender, messages) = mpsc::channel::<Message>();
        let (_reload, requests) = mpsc::channel::<()>();
        let calls = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|scope| {
            let calls = &calls;
            scope.spawn(move || {
                work(
                    &sender,
                    &requests,
                    Duration::from_millis(10),
                    || {
                        calls.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                        let inputs = Inputs {
                            status: vec!["questions unavailable".to_owned()],
                            ..Inputs::default()
                        };
                        let view = build(&inputs);
                        Ok((inputs, view))
                    },
                    String::new,
                );
            });
            let mut seen = 0;
            while seen < 4 {
                if let Ok(Message::Loaded(view, _)) = messages.recv() {
                    assert_eq!(view.status, ["questions unavailable"]);
                    seen += 1;
                }
            }
            drop(messages);
        });
        assert_eq!(calls.load(std::sync::atomic::Ordering::SeqCst), 1);
    }

    #[test]
    fn while_typing_q_and_r_are_text_and_enter_returns_the_keys() {
        let mut view = build(&Inputs::default());
        let none = KeyModifiers::NONE;
        handle_key(&mut view, KeyCode::Char('/'), none, 10);
        handle_key(&mut view, KeyCode::Char('q'), none, 10);
        handle_key(&mut view, KeyCode::Char('r'), none, 10);
        assert_eq!(view.filter, "qr");
        handle_key(&mut view, KeyCode::Enter, none, 10);
        assert!(!view.typing);
        handle_key(&mut view, KeyCode::Esc, none, 10);
        assert_eq!(view.filter, "");
    }
}
