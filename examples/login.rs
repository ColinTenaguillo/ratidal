#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = sonora_tui::config::Config::load()?;
    let http = reqwest::Client::new();

    let code = sonora_tui::auth::start_login(&http, &cfg.auth).await?;
    println!(
        "open {} and enter {}",
        code.verification_uri, code.user_code
    );

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(code.interval_secs)).await;
        match sonora_tui::auth::poll_once(&http, &cfg.auth, &code).await? {
            sonora_tui::auth::PollOutcome::Granted(t) => {
                println!("granted; country={} user={}", t.country_code, t.user_id);
                sonora_tui::auth::store::save(&t)?;
                return Ok(());
            }
            sonora_tui::auth::PollOutcome::Pending => print!("."),
            sonora_tui::auth::PollOutcome::SlowDown => println!("slow down"),
            sonora_tui::auth::PollOutcome::Expired => {
                println!("expired");
                return Ok(());
            }
        }
    }
}
