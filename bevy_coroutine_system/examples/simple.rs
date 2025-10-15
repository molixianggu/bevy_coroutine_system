//! Simple coroutine system example - Box sequence animation
//!
//! This example demonstrates how to use the coroutine system to create a continuous animation sequence.
//! The box will automatically perform a series of actions in a continuous loop.

#![feature(coroutines, coroutine_trait)]

use bevy::prelude::*;
use bevy_coroutine_system::prelude::*;
use std::time::Duration;

fn main() {
    let mut app = App::new();

    app.add_plugins(DefaultPlugins)
        .add_systems(Startup, setup)
        .add_systems(Update, async_system(box_animation));

    app.run();
}

/// Set up the scene
fn setup(mut commands: Commands) {
    // Camera
    commands.spawn(Camera2d);

    // Create a box
    commands.spawn((
        Sprite {
            color: Color::WHITE,
            custom_size: Some(Vec2::new(100.0, 100.0)),
            ..default()
        },
        Transform::from_xyz(0.0, 0.0, 0.0),
        AnimatedBox,
    ));

    // Status text
    commands.spawn((
        Text2d::new("Animation running..."),
        TextFont {
            font_size: 24.0,
            ..default()
        },
        Transform::from_xyz(0.0, 250.0, 0.0),
        StatusText,
    ));
}

/// Marker for the animated box
#[derive(Component)]
struct AnimatedBox;

/// Marker for the status text
#[derive(Component)]
struct StatusText;

/// Coroutine animation sequence that runs in a loop

async fn box_animation() -> Result<()> {
    // Start animation
    info!("Animation started!");

    // Update text prompt
    noop().await.with(
        |_: In<()>, mut text: Query<&mut Text2d, With<StatusText>>| -> Result<()> {
            text.single_mut()?.0 = "Scaling up...".to_string();
            Ok(())
        },
    ).unwrap();

    // Phase 1: Scale up
    for _ in 0..30 {
        next_frame().await.with(
            |_: In<()>, mut box_query: Query<&mut Transform, With<AnimatedBox>>| -> Result<()> {
                for mut transform in box_query.iter_mut() {
                    transform.scale *= 1.02;
                }
                Ok(())
            },
        ).unwrap();
    }

    // Wait a moment
    sleep(Duration::from_millis(300)).await;

    // Update text
    noop().await.with(
        |_: In<()>, mut text: Query<&mut Text2d, With<StatusText>>| -> Result<()> {
            text.single_mut()?.0 = "Moving and rotating...".to_string();
            Ok(())
        },
    ).unwrap();

    // Phase 2: Move and rotate
    for _ in 0..60 {
        next_frame().await.with(
            |_: In<()>, mut box_query: Query<&mut Transform, With<AnimatedBox>>| -> Result<()> {
                for mut transform in box_query.iter_mut() {
                    transform.translation.x += 2.0;
                    transform.rotate_z(0.02);
                }
                Ok(())
            },
        ).unwrap();
    }

    // Wait
    sleep(Duration::from_millis(500)).await;

    // Update text
    noop().await.with(
        |_: In<()>, mut text: Query<&mut Text2d, With<StatusText>>| -> Result<()> {
            text.single_mut()?.0 = "Returning...".to_string();
            Ok(())
        },
    ).unwrap();

    // Phase 3: Return and scale down
    for _ in 0..60 {
        next_frame().await.with(
            |_: In<()>, mut box_query: Query<&mut Transform, With<AnimatedBox>>| -> Result<()> {
                for mut transform in box_query.iter_mut() {
                    transform.translation.x -= 2.0;
                    transform.rotate_z(-0.02);
                }
                Ok(())
            },
        ).unwrap();
    }

    // Finally restore size
    for _ in 0..30 {
        next_frame().await.with(
            |_: In<()>, mut box_query: Query<&mut Transform, With<AnimatedBox>>| -> Result<()> {
                for mut transform in box_query.iter_mut() {
                    transform.scale /= 1.02;
                }
                Ok(())
            },
        ).unwrap();
    }

    // Complete
    noop().await.with(
        |_: In<()>, mut text: Query<&mut Text2d, With<StatusText>>| -> Result<()> {
            text.single_mut()?.0 = "Animation complete! Restarting...".to_string();
            Ok(())
        },
    ).unwrap();

    info!("Animation cycle completed, restarting...");

    Ok(())
}
