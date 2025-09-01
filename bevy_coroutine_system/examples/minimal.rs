//! Minimal coroutine system example

#![feature(coroutines, coroutine_trait)]

use bevy::prelude::*;
use bevy_coroutine_system::prelude::*;
use std::time::Duration;

fn main() {
    let mut app = App::new();

    app.add_plugins((MinimalPlugins, CoroutinePlugin));

    let id = app.register_coroutine(minimal_system, minimal_system::id());

    println!("entities: {}", app.world().entities().len());

    // Manually run a few updates to see the effect
    for i in 0..12 {
        println!("--- Frame {} ---", i);

        app.world_mut().run_system(id).ok();

        std::thread::sleep(Duration::from_millis(200));
    }

    println!("entities: {}", app.world().entities().len());
}

#[coroutine_system]
fn minimal_system(mut commands: Commands) {
    println!("System running - phase 1");
    commands.spawn_empty();

    // Directly use the yield statement
    yield sleep(Duration::from_secs(1));

    println!("System running - phase 2");
    commands.spawn_empty();
}
