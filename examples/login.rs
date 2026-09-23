#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let cfg = ratidal::config::Config::load()?;
    let http = reqwest::Client::new();

    let code = ratidal::auth::start_login(&http, &cfg.auth).await?;
    println!(
        "open {} and enter {}",
        code.verification_uri, code.user_code
    );

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(code.interval_secs)).await;
        match ratidal::auth::poll_once(&http, &cfg.auth, &code).await? {
            ratidal::auth::PollOutcome::Granted(t) => {
                println!("granted; country={} user={}", t.country_code, t.user_id);
                ratidal::auth::store::save(&t)?;
                return Ok(());
            }
            ratidal::auth::PollOutcome::Pending => print!("."),
            ratidal::auth::PollOutcome::SlowDown => println!("slow down"),
            ratidal::auth::PollOutcome::Expired => {
                println!("expired");
                return Ok(());
            }
        }
    }
}
