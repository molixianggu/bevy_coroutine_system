//! HTTP request example - Demonstrates async HTTP requests in coroutines
//!
//! This example shows how to make async HTTP requests within the coroutine system.
//! The coroutine automatically runs in a loop, fetching data from a test API and displaying the results.

use std::time::{Duration, Instant};

use bevy::prelude::*;
use bevy_coroutine_system::prelude::*;

fn main() {
    let mut app = App::new();

    app.add_plugins(DefaultPlugins)
        .add_systems(Startup, setup)
        .add_systems(Update, async_system(http_request_coroutine));

    app.run();
}

/// Set up the scene
fn setup(mut commands: Commands) {
    // Camera
    commands.spawn(Camera2d);

    // Status text
    commands.spawn((
        Text2d::new("Waiting to start..."),
        TextFont {
            font_size: FontSize::Px(24.0),
            ..default()
        },
        Transform::from_xyz(0.0, 300.0, 0.0),
        StatusText,
    ));

    // Response text
    commands.spawn((
        Text2d::new(""),
        TextFont {
            font_size: FontSize::Px(20.0),
            ..default()
        },
        Transform::from_xyz(0.0, -50.0, 0.0),
        ResponseText,
    ));
}

/// Marker for the status text
#[derive(Component)]
struct StatusText;

/// Marker for the response text
#[derive(Component)]
struct ResponseText;

/// Coroutine that performs async HTTP requests in a loop

async fn http_request_coroutine() -> Result<()> {
    noop().await.with(
        |_: In<()>, mut text: Query<&mut Text2d, With<StatusText>>| -> Result<()> {
            text.single_mut()?.0 = "Sending HTTP request...".to_string();
            Ok(())
        },
    ).unwrap();

    // Clear previous response
    noop().await.with(
        |_: In<()>, mut text: Query<&mut Text2d, With<ResponseText>>| -> Result<()> {
            text.single_mut()?.0 = "".to_string();
            Ok(())
        },
    ).unwrap();

    // Make the async HTTP request
    info!("Starting HTTP request...");

    // Use spawn_blocking_task to perform HTTP request in background thread
    spawn_blocking_task(|| {
        let mut response = ureq::get("https://httpbin.org/json").call().unwrap();
        response.body_mut().read_to_string().ok()
    })
    .await
    .with(
        |response_result: In<Option<String>>,
         mut text: Query<&mut Text2d, With<ResponseText>>|
         -> Result<()> {
            match response_result.0 {
                Some(body) => {
                    text.single_mut()?.0 = format!("Response:\n{}", body);
                    Ok(())
                }
                None => {
                    text.single_mut()?.0 = "Error: Failed to fetch data".to_string();
                    Ok(())
                }
            }
        },
    ).unwrap();

    for i in 0..10 {
        sleep(Duration::from_secs(1)).await.with(
            move |_: In<Instant>, mut text: Query<&mut Text2d, With<StatusText>>| -> Result<()> {
                text.single_mut()?.0 = format!("Next request in {} seconds...", 10 - i).to_string();
                Ok(())
            },
        ).unwrap();
    }

    info!("Request cycle completed, restarting...");

    Ok(())
}
