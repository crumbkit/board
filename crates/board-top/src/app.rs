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
    /// A load finished, or the data was still current.
    Loaded(Box<View>),
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

/// Loads at once, then checks every `CHECK` whether the board changed and
/// reloads when it did. A load whose status holds an error is not kept, so
/// the next check retries it. Ends when the screen stops listening.
fn work(source: &Source, sender: &mpsc::Sender<Message>, reload: &mpsc::Receiver<()>) {
    let mut held: Option<(Inputs, String)> = None;
    let mut started: Option<Instant> = None;
    let mut last_load = Duration::ZERO;
    loop {
        let forced = match reload.recv_timeout(if started.is_none() {
            Duration::ZERO
        } else {
            CHECK
        }) {
            Ok(()) => true,
            Err(mpsc::RecvTimeoutError::Timeout) => false,
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        };
        let key = source.key();
        let moved = held.as_ref().is_none_or(|(_, seen)| *seen != key);
        let since = started.map_or(Duration::MAX, |at| at.elapsed());
        if !reload_due(
            forced,
            held.is_some(),
            moved,
            since,
            reload_floor(last_load),
        ) {
            if let Some((inputs, _)) = &mut held {
                inputs.now = now();
                if sender
                    .send(Message::Loaded(Box::new(build(inputs))))
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
        let message = match load(source) {
            Ok((inputs, view)) => {
                if inputs.status.is_empty() {
                    held = Some((inputs, key));
                }
                Message::Loaded(Box::new(view))
            }
            Err(error) => Message::Failed(error),
        };
        last_load = begin.elapsed();
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

/// Runs the full-screen view until `q` or Ctrl-C.
pub fn live(source: &Source) -> Result<(), String> {
    let mut terminal =
        ratatui::try_init().map_err(|error| format!("could not use the terminal: {error}"))?;
    let _restore = Restore;
    let (sender, receiver) = mpsc::channel::<Message>();
    let worker_source = source.clone();
    let (reload, reload_requests) = mpsc::channel::<()>();
    // The worker is not joined: quitting must not wait for a load.
    std::thread::spawn(move || work(&worker_source, &sender, &reload_requests));
    let mut view: Option<View> = None;
    let mut loaded: Option<Instant> = None;
    let mut loading = true;
    let mut stopped = false;
    loop {
        loop {
            match receiver.try_recv() {
                Ok(Message::Loading) => loading = true,
                Ok(Message::Loaded(fresh)) => {
                    let mut fresh = *fresh;
                    if let Some(old) = &view {
                        fresh.keep_selection(old);
                    }
                    view = Some(fresh);
                    loaded = Some(Instant::now());
                    loading = false;
                }
                Ok(Message::Failed(error)) => {
                    loading = false;
                    match &mut view {
                        Some(shown) => shown.status = vec![error],
                        None => return Err(error),
                    }
                }
                Err(mpsc::TryRecvError::Empty) => break,
                Err(mpsc::TryRecvError::Disconnected) => {
                    stopped = true;
                    loading = false;
                    break;
                }
            }
        }
        if let Some(shown) = &mut view {
            shown.freshness = freshness(loaded, loading);
            if stopped {
                shown.status =
                    vec!["refresh has stopped; the data is no longer updated".to_owned()];
            }
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
                    let quit = key.code == KeyCode::Char('q')
                        || (key.code == KeyCode::Char('c')
                            && key.modifiers.contains(KeyModifiers::CONTROL));
                    if quit {
                        return Ok(());
                    }
                    if let Some(shown) = &mut view {
                        match key.code {
                            KeyCode::Up => shown.move_by(-1),
                            KeyCode::Down => shown.move_by(1),
                            KeyCode::PageUp => shown.page(capacity, false),
                            KeyCode::PageDown => shown.page(capacity, true),
                            KeyCode::Home => shown.home(),
                            KeyCode::End => shown.end(),
                            _ => {}
                        }
                    }
                    if key.code == KeyCode::Char('r') {
                        // The worker may have ended; the screen says so already.
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
}
