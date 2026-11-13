//! The terminal loop: draws the view, refreshes it every ten seconds on a
//! worker thread, and quits on `q`.

use std::io::Write;
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};
use ratatui::widgets::Paragraph;

use crate::model::{Inputs, View, build};
use crate::render::{draw, render_text};
use crate::source::Source;

/// How often the view reloads.
pub const REFRESH: Duration = Duration::from_secs(10);

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

/// Loads at once, then every `REFRESH`, skipping the reload when no checkout
/// has moved since the last one. Ends when the screen stops listening.
fn work(source: &Source, sender: &mpsc::Sender<Message>) {
    let mut last: Option<(Inputs, Vec<String>)> = None;
    loop {
        let heads = source.heads();
        let message = match &mut last {
            Some((inputs, seen)) if *seen == heads => {
                inputs.now = now();
                Message::Loaded(Box::new(build(inputs)))
            }
            _ => {
                if sender.send(Message::Loading).is_err() {
                    return;
                }
                match load(source) {
                    Ok((inputs, view)) => {
                        last = Some((inputs, heads));
                        Message::Loaded(Box::new(view))
                    }
                    Err(error) => Message::Failed(error),
                }
            }
        };
        if sender.send(message).is_err() {
            return;
        }
        std::thread::sleep(REFRESH);
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
    // The worker is not joined: quitting must not wait for a load.
    std::thread::spawn(move || work(&worker_source, &sender));
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
                        fresh.selected = old
                            .selected_path()
                            .and_then(|path| fresh.rows().position(|row| row.path == path))
                            .unwrap_or_else(|| {
                                old.selected.min(fresh.rows().count().saturating_sub(1))
                            });
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
