//! Scratch tool: draws a game's level with no window and writes the picture
//! as raw RGBA bytes. `shot <folder> <out> x y z yaw pitch [width height frames upscaling]`.
use lore::render::{Camera, Headless, Upscaling};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args: Vec<String> = std::env::args().collect();
    let number = |at: usize, usual: f32| {
        args.get(at)
            .and_then(|text| text.parse().ok())
            .unwrap_or(usual)
    };
    let scene = lore::level::load(args[1].as_ref())?.ok_or("no level.glb there")?;
    #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
    let (width, height, frames) = (
        number(8, 1280.0) as u32,
        number(9, 720.0) as u32,
        number(10, 1.0) as u32,
    );
    let upscaling = match args.get(11).map(String::as_str) {
        Some("quality") => Upscaling::Quality,
        Some("native") => Upscaling::Native,
        _ => Upscaling::Off,
    };
    let mut headless = Headless::new(width, height, upscaling)?;
    headless.set_scene(&scene)?;
    let camera = Camera {
        position: [number(3, 0.0), number(4, 1.7), number(5, 0.0)],
        yaw: number(6, 0.0),
        pitch: number(7, 0.0),
        ..Camera::default()
    };
    for _ in 1..frames {
        headless.render(&camera)?;
    }
    std::fs::write(&args[2], headless.screenshot(&camera)?)?;
    Ok(())
}
