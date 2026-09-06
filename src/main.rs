fn main() -> anyhow::Result<()> {
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

    let runtime = tokio::runtime::Runtime::new()?;

    // init() enables raw mode, the alternate screen, and a panic hook that
    // restores both. restore() undoes it on the normal path.
    let mut terminal = ratatui::init();

    // A panic on a detached task does not stop the process: tokio swallows it
    // at the task boundary. But ratatui's hook is process-global, so it would
    // restore the terminal while the render loop kept drawing — dropping the
    // user out of the TUI and garbling their shell for the rest of the
    // session. Exiting after the hook has restored is the honest outcome.
    let restore_then_exit = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        restore_then_exit(info);
        std::process::exit(1);
    }));

    let result = runtime.block_on(ratidal::shell::run(&mut terminal));
    ratatui::restore();

    result
}
