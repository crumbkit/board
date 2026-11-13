//! The terminal loop: draws the view, refreshes it every ten seconds on a
//! worker thread, and quits on `q`.

use std::io::Write;
use std::sync::mpsc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use ratatui::crossterm::event::{self, Event, KeyCode, KeyEventKind, KeyModifiers};

use crate::model::{View, build};
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

fn load(source: &Source) -> Result<View, String> {
    source.load(now()).map(|inputs| build(&inputs))
}

/// Loads once and renders one frame as plain text.
pub fn once(source: &Source, width: u16, height: u16) -> Result<String, String> {
    Ok(render_text(&load(source)?, width, height))
}

/// Restores the terminal when dropped, including on a panic unwinding past it.
struct Restore;

impl Drop for Restore {
    fn drop(&mut self) {
        ratatui::restore();
    }
}

/// Runs the full-screen view until `q` or Ctrl-C.
pub fn live(source: &Source) -> Result<(), String> {
    emit(
        Stream::Err,
        "board-top: reading the board; a cold board can take a minute",
    );
    let mut view = load(source)?;
    let (sender, receiver) = mpsc::channel::<Result<View, String>>();
    let (stop, stopped) = mpsc::channel::<()>();
    let worker_source = source.clone();
    let worker = std::thread::spawn(move || {
        while let Err(mpsc::RecvTimeoutError::Timeout) = stopped.recv_timeout(REFRESH) {
            if sender.send(load(&worker_source)).is_err() {
                break;
            }
        }
    });
    let result = {
        let mut terminal = ratatui::init();
        let _restore = Restore;
        loop {
            while let Ok(next) = receiver.try_recv() {
                match next {
                    Ok(mut fresh) => {
                        fresh.selected = view.selected.min(fresh.rows().count().saturating_sub(1));
                        view = fresh;
                    }
                    Err(error) => view.status = vec![error],
                }
            }
            if let Err(error) = terminal.draw(|frame| draw(frame, &view)) {
                break Err(format!("could not draw: {error}"));
            }
            match event::poll(Duration::from_millis(200)) {
                Ok(true) => match event::read() {
                    Ok(Event::Key(key)) if key.kind == KeyEventKind::Press => {
                        let quit = key.code == KeyCode::Char('q')
                            || (key.code == KeyCode::Char('c')
                                && key.modifiers.contains(KeyModifiers::CONTROL));
                        if quit {
                            break Ok(());
                        }
                    }
                    Ok(_) => {}
                    Err(error) => break Err(format!("could not read the keyboard: {error}")),
                },
                Ok(false) => {}
                Err(error) => break Err(format!("could not poll the keyboard: {error}")),
            }
        }
    };
    drop(stop);
    let _ = worker.join();
    result
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
