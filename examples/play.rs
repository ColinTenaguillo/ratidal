use std::time::Duration;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let path = std::env::args().nth(1).ok_or("usage: play <dash-fixture-dir>")?;

    let read = |n: &str| std::fs::read(format!("{path}/{n}")).map_err(|e| e.to_string());
    let reader = ratidal::playback::SegmentReader::from_slices(vec![
        read("init.mp4")?,
        read("1.m4s")?,
        read("2.m4s")?,
        read("3.m4s")?,
    ]);

    let mut sink = rodio::DeviceSinkBuilder::open_default_sink()?;
    sink.log_on_drop(false);
    let player = rodio::Player::connect_new(sink.mixer());
    player.append(rodio::Decoder::new(reader)?);
    player.play();

    for _ in 0..14 {
        tokio::time::sleep(Duration::from_secs(1)).await;
        println!("pos = {:?}  empty = {}", player.get_pos(), player.empty());
        if player.empty() {
            break;
        }
    }
    Ok(())
}
