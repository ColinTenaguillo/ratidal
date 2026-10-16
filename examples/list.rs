#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let token = sonora_tui::auth::store::load()?.ok_or("no token — run the login example")?;
    let client = sonora_tui::tidal::Client::new(token);

    println!("country = {}", client.country());

    for p in sonora_tui::library::playlists(&client).await? {
        println!("playlist  {:>4}  {}", p.track_count, p.title);
    }

    let tracks = sonora_tui::library::favourite_tracks(&client).await?;
    for t in tracks.iter().take(5) {
        println!(
            "track  {}  {}  {}",
            t.id,
            if t.is_hires() { "HI" } else { "  " },
            t.title
        );
    }

    if let Some(t) = tracks.first() {
        let info = client
            .playback_info(t.id.clone(), sonora_tui::domain::Quality::HiResLossless)
            .await?;
        // Read back what was DELIVERED, never assume the requested tier.
        println!(
            "delivered {:?}  {:?}bit  {:?}Hz",
            info.delivered, info.bit_depth, info.sample_rate
        );
    }
    Ok(())
}
