//! Minimal coroutine system example - Timer loop
//!
//! This is the simplest example demonstrating the coroutine system.
//! It continuously logs elapsed time every second, showing how to use
//! the `sleep` async operation with a system callback.

use bevy::prelude::*;
use bevy_coroutine_system::prelude::*;
use bevy::log::LogPlugin;
use std::time::{Duration, Instant};

async fn mini_system() -> Result<()> {
    let a = sleep(Duration::from_secs(1))
        .await
        .with(|_: In<Instant>, v: Res<Time>| -> Result<i32> {
            info!("{:.2} seconds", v.elapsed_secs());
            Ok(3)
        }).unwrap();
    info!("a: {}", a);
    Ok(())
}

fn main() {
    App::new()
        .add_plugins((MinimalPlugins, LogPlugin::default()))
        .add_systems(Update, async_system(mini_system))
        .run();
}
