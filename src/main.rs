/// On macOS the media keys are only delivered to a process running a Core
/// Foundation run loop on its main thread, so the app runs on a second
/// thread and this one pumps that loop. Everywhere else the app owns the
/// main thread as usual.
#[cfg(target_os = "macos")]
fn main() {
    let code = std::sync::Arc::new(std::sync::atomic::AtomicI32::new(0));
    let exit = code.clone();
    ratidal::shell::mediakeys_macos::run_with_main_loop(move || async move {
        if let Err(e) = app() {
            eprintln!("{e}");
            exit.store(1, std::sync::atomic::Ordering::Release);
        }
    });
    std::process::exit(code.load(std::sync::atomic::Ordering::Acquire));
}

#[cfg(not(target_os = "macos"))]
fn main() -> anyhow::Result<()> {
    app()
}

fn app() -> anyhow::Result<()> {
    // File logging: a TUI cannot be debugged with println!.
    if let Some(dir) = ratidal::config::paths::cache_dir() {
        std::fs::create_dir_all(&dir)?;
        let appender = tracing_appender::rolling::never(&dir, "ratidal.log");
        tracing_subscriber::fmt()
            .with_writer(appender)
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_default_env()
                    .unwrap_or_else(|_| "ratidal=info".into()),
            )
            .init();
    }

    // On macOS a runtime is already running on this thread -- `main` handed
    // the app to one so the run loop could keep the main thread -- and
    // nesting a second would panic on `block_on`.
    #[cfg(not(target_os = "macos"))]
    let runtime = tokio::runtime::Runtime::new()?;

    // Probe for an image protocol BEFORE the terminal is put into raw mode
    // and switched to the alternate screen: the query writes an escape to
    // stdout and reads the reply from stdin, which the alternate screen would
    // swallow. Most terminals answer nothing, and covers fall back to blocks.
    let picker = ratidal::shell::artwork::Artwork::probe();

    // try_init() rather than init(): the latter panics when there is no
    // terminal, which is what happens if this is piped or run from a script.
    // A Rust backtrace is a poor way to say "I need a terminal".
    let mut terminal = match ratatui::try_init() {
        Ok(t) => t,
        Err(e) => {
            eprintln!("ratidal needs an interactive terminal, and could not open one.");
            eprintln!("Run it directly in a terminal rather than through a pipe or a script.");
            eprintln!();
            eprintln!("The underlying error was: {e}");
            std::process::exit(1);
        }
    };

    // try_init() has installed a panic hook that restores the terminal. Chain
    // an exit onto it: a panic on a detached task does not stop the process
    // (tokio swallows it at the task boundary), but the hook is process-global,
    // so it would restore the terminal while the render loop kept drawing —
    // dropping the user out of the TUI and garbling their shell for the rest
    // of the session. Exiting after the restore is the honest outcome.
    let restore_then_exit = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        // Log before restoring. The restore repaints the screen, so a panic
        // message printed to the terminal is wiped a moment later and the
        // user sees the app simply vanish — with nothing in the log either,
        // which makes a crash indistinguishable from a clean exit.
        tracing::error!("panic: {info}");
        restore_then_exit(info);
        std::process::exit(1);
    }));

    #[cfg(not(target_os = "macos"))]
    let result = runtime.block_on(ratidal::shell::run(&mut terminal, picker));
    // Already inside a runtime: the future is driven on this thread, which
    // is the app's thread rather than the one pumping the run loop.
    #[cfg(target_os = "macos")]
    let result = tokio::task::block_in_place(|| {
        tokio::runtime::Handle::current().block_on(ratidal::shell::run(&mut terminal, picker))
    });
    ratatui::restore();

    result
}
