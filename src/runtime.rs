use crate::{
    host,
    model::Record,
    store::{Cache, Store},
    ui::{Effect, Line, Screen, View},
};
use anyhow::Result;
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{self, DisableMouseCapture, EnableMouseCapture, Event},
    execute, queue,
    style::Print,
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use notify::{EventKind, RecursiveMode, Watcher};
use std::{
    io::{self, Write},
    path::PathBuf,
    sync::{
        Arc,
        mpsc::{self, Sender},
    },
    thread,
    time::{Duration, Instant},
};

enum Message {
    Input(Event),
    Data(Vec<Arc<Record>>),
    Status(String),
    Job(std::result::Result<String, String>),
    Quit,
}

struct Terminal;
impl Terminal {
    fn start() -> Result<Self> {
        terminal::enable_raw_mode()?;
        let guard = Self;
        execute!(io::stdout(), EnterAlternateScreen, Hide, EnableMouseCapture)?;
        Ok(guard)
    }
}
impl Drop for Terminal {
    fn drop(&mut self) {
        restore();
    }
}
fn restore() {
    let _ = execute!(
        io::stdout(),
        DisableMouseCapture,
        Show,
        LeaveAlternateScreen
    );
    let _ = terminal::disable_raw_mode();
}

fn watch(store: Store, tx: Sender<Message>) {
    thread::spawn(move || {
        let (changes, events) = mpsc::sync_channel(1);
        let watcher = notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
            if !matches!(event.as_ref().map(|e| &e.kind), Ok(EventKind::Access(_))) {
                let _ = changes.try_send(());
            }
        });
        let mut watcher = watcher.ok();
        let active = watcher.as_mut().is_some_and(|w| {
            w.watch(&store.directory, RecursiveMode::NonRecursive)
                .is_ok()
        });
        if !active {
            let _ = tx.send(Message::Status(
                "File watcher unavailable. Checking file metadata every 5 seconds.".into(),
            ));
        }
        let mut cache = Cache::default();
        let mut sources = crate::audit::SourceCache::default();
        let mut source_dirs = std::collections::HashSet::new();
        let mut last_drift = vec![];
        let mut first = true;
        let mut old_error = String::new();
        loop {
            match cache.refresh(&store) {
                Ok(changed) => {
                    let records = sources.refresh(&cache.records());
                    let drift: Vec<_> = records
                        .iter()
                        .map(|r| (r.flow.name.clone(), r.drifted_paths.clone()))
                        .collect();
                    let dirs = crate::audit::watch_directories(&records);
                    if let Some(watcher) = watcher.as_mut() {
                        for dir in source_dirs.difference(&dirs) {
                            if dir != &store.directory {
                                let _ = watcher.unwatch(dir);
                            }
                        }
                        for dir in dirs.difference(&source_dirs) {
                            let _ = watcher.watch(dir, RecursiveMode::NonRecursive);
                        }
                    }
                    source_dirs = dirs;
                    if (changed || first || drift != last_drift)
                        && tx.send(Message::Data(records)).is_err()
                    {
                        return;
                    }
                    last_drift = drift;
                    if !old_error.is_empty() {
                        let _ = tx.send(Message::Status("Flow data loaded.".into()));
                        old_error.clear();
                    }
                    first = false;
                }
                Err(error) => {
                    let message = format!("Cannot reload flows: {error:#}");
                    if message != old_error {
                        if tx.send(Message::Status(message.clone())).is_err() {
                            return;
                        }
                        old_error = message;
                    }
                }
            }
            // Notifications wake this worker immediately. The slow metadata check
            // recovers missed events. It does not reparse unchanged documents.
            match events.recv_timeout(Duration::from_secs(5)) {
                Ok(()) => while events.try_recv().is_ok() {},
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => thread::sleep(Duration::from_secs(5)),
            }
        }
    });
}

fn job(effect: Effect, store: Store, tx: Sender<Message>) {
    thread::spawn(move || {
        let result: Result<String> = match effect {
            Effect::Open(project, loc) => {
                host::open_source(&project, &loc).map(|_| "Opened the source in Neovim.".into())
            }
            Effect::Diff(project, loc) => {
                host::open_diff(&project, &loc).map(|_| "Opened the file diff through dn.".into())
            }
            Effect::Delete(name) => store
                .delete(&name)
                .map(|_| "Deleted the selected flow. No undo is available.".into()),
            _ => Ok(String::new()),
        };
        let _ = tx.send(Message::Job(result.map_err(|e| format!("{e:#}"))));
    });
}

fn paint(screen: &Screen, previous: &mut Vec<Line>, resized: bool) -> Result<()> {
    let mut stdout = io::stdout().lock();
    if resized {
        queue!(stdout, Clear(ClearType::All))?;
        previous.clear();
    }
    for (y, line) in screen.lines.iter().enumerate() {
        if previous.get(y) == Some(line) {
            continue;
        }
        queue!(stdout, MoveTo(0, y as u16), Clear(ClearType::CurrentLine))?;
        if line.color != 0 && std::env::var_os("NO_COLOR").is_none() {
            queue!(
                stdout,
                Print(format!("\x1b[{}m{}\x1b[0m", line.color, line.text))
            )?;
        } else {
            queue!(stdout, Print(&line.text))?;
        }
    }
    for y in screen.lines.len()..previous.len() {
        queue!(stdout, MoveTo(0, y as u16), Clear(ClearType::CurrentLine))?;
    }
    stdout.flush()?;
    *previous = screen.lines.clone();
    Ok(())
}

pub fn view(store: Store, project: Option<PathBuf>) -> Result<()> {
    store.ensure_dir()?;
    let mut model = View::new(store.scope.clone(), project);
    let (tx, rx) = mpsc::channel();
    let mut signals = signal_hook::iterator::Signals::new([
        signal_hook::consts::SIGTERM,
        signal_hook::consts::SIGINT,
        signal_hook::consts::SIGHUP,
    ])?;
    let signal_tx = tx.clone();
    thread::spawn(move || {
        if signals.forever().next().is_some() {
            let _ = signal_tx.send(Message::Quit);
        }
    });
    let prior_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore();
        prior_hook(info);
    }));
    let _terminal = Terminal::start()?;
    watch(store.clone(), tx.clone());
    let input_tx = tx.clone();
    thread::spawn(move || {
        loop {
            match event::read() {
                Ok(input) => {
                    if input_tx.send(Message::Input(input)).is_err() {
                        break;
                    }
                }
                Err(_) => {
                    let _ = input_tx.send(Message::Quit);
                    break;
                }
            }
        }
    });
    let (mut width, mut height) = terminal::size()?;
    let mut screen = model.render(width as usize, height as usize);
    let mut previous = vec![];
    paint(&screen, &mut previous, true)?;
    // No redraw timer. Sleep until input, a data update, or a worker result arrives.
    while let Ok(message) = rx.recv() {
        let mut resized = false;
        match message {
            Message::Quit => break,
            Message::Data(data) => model.update(data),
            Message::Status(status) => model.message = status,
            Message::Job(result) => {
                model.busy = false;
                model.message = result.unwrap_or_else(|e| format!("Error: {e}"));
            }
            Message::Input(Event::Resize(w, h)) => {
                width = w;
                height = h;
                resized = true;
            }
            Message::Input(input) => {
                // Release and motion events do not change this UI.
                if matches!(&input, Event::Mouse(m) if matches!(m.kind, event::MouseEventKind::Up(_) | event::MouseEventKind::Moved | event::MouseEventKind::Drag(_)))
                {
                    continue;
                }
                match model.event(input, &screen, Instant::now()) {
                    Effect::Quit => break,
                    Effect::None => {}
                    effect => job(effect, store.clone(), tx.clone()),
                }
            }
        }
        screen = model.render(width as usize, height as usize);
        paint(&screen, &mut previous, resized)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod tests;
