//! Simple coroutine system example - Box sequence animation
//! 
//! This example demonstrates how to use the coroutine system to create a continuous animation sequence.
//! Press the spacebar to trigger the animation, and the box will perform a series of actions.

#![feature(coroutines, coroutine_trait)]

use bevy::prelude::*;
use bevy_coroutine_system::prelude::*;
use std::time::Duration;

fn main() {
    let mut app = App::new();
    
    app.add_plugins((DefaultPlugins, CoroutinePlugin))
        .add_systems(Startup, setup)
        .add_systems(Update, trigger_animation);
    
    // Register the coroutine system
    app.register_coroutine(box_animation, box_animation::id());
    
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
        Text2d::new("Press SPACE to start animation"),
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

/// Listen for spacebar to trigger animation
fn trigger_animation(
    mut commands: Commands,
    keyboard: Res<ButtonInput<KeyCode>>,
) {
    if keyboard.just_pressed(KeyCode::Space) {
        commands.run_system_cached(box_animation);
    }
}

/// Coroutine animation sequence
#[coroutine_system]
fn box_animation(
    mut box_query: Query<&mut Transform, With<AnimatedBox>>,
    mut text_query: Query<&mut Text2d, With<StatusText>>,
) {
    // Start animation
    info!("Animation started!");
    
    // Update text prompt
    for mut text in text_query.iter_mut() {
        **text = "Scaling up...".to_string();
    }
    
    // Phase 1: Scale up
    for _ in 0..30 {
        yield next_frame();
        for mut transform in box_query.iter_mut() {
            transform.scale *= 1.02;
        }
    }
    
    // Wait a moment
    yield sleep(Duration::from_millis(300));
    
    // Update text
    for mut text in text_query.iter_mut() {
        **text = "Moving and rotating...".to_string();
    }
    
    // Phase 2: Move and rotate
    for _ in 0..60 {
        yield next_frame();
        for mut transform in box_query.iter_mut() {
            transform.translation.x += 2.0;
            transform.rotate_z(0.02);
        }
    }
    
    // Wait
    yield sleep(Duration::from_millis(500));
    
    // Update text
    for mut text in text_query.iter_mut() {
        **text = "Returning...".to_string();
    }
    
    // Phase 3: Return and scale down
    for _ in 0..60 {
        yield next_frame();
        for mut transform in box_query.iter_mut() {
            transform.translation.x -= 2.0;
            transform.rotate_z(-0.02);
        }
    }
    
    // Finally restore size
    for _ in 0..30 {
        yield next_frame();
        for mut transform in box_query.iter_mut() {
            transform.scale /= 1.02;
        }
    }
    yield noop();
    
    // Complete
    for mut text in text_query.iter_mut() {
        **text = "Animation complete! Press SPACE to restart".to_string();
    }
    
    info!("Animation completed!");
}