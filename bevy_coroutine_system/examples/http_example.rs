//! HTTP request example - Demonstrates async HTTP requests in coroutines
//! 
//! This example shows how to make async HTTP requests within the coroutine system.
//! Press SPACE to trigger an HTTP request that fetches data from a test API.

#![feature(coroutines, coroutine_trait)]

use bevy::prelude::*;
use bevy_coroutine_system::prelude::*;

fn main() {
    let mut app = App::new();
    
    app.add_plugins((DefaultPlugins, CoroutinePlugin))
        .add_systems(Startup, setup)
        .add_systems(Update, trigger_request);
    
    // Register the coroutine system
    app.register_coroutine(http_request_coroutine, http_request_coroutine::id());
    
    app.run();
}

/// Set up the scene
fn setup(mut commands: Commands) {
    // Camera
    commands.spawn(Camera2d);
    
    // Status text
    commands.spawn((
        Text2d::new("Press SPACE to send HTTP request"),
        TextFont {
            font_size: 24.0,
            ..default()
        },
        Transform::from_xyz(0.0, 300.0, 0.0),
        StatusText,
    ));
    
    // Response text
    commands.spawn((
        Text2d::new(""),
        TextFont {
            font_size: 20.0,
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

/// Listen for spacebar to trigger HTTP request
fn trigger_request(
    mut commands: Commands,
    keyboard: Res<ButtonInput<KeyCode>>,
) {
    if keyboard.just_pressed(KeyCode::Space) {
        commands.run_system_cached(http_request_coroutine);
    }
}


/// Coroutine that performs an async HTTP request
#[coroutine_system]
fn http_request_coroutine(
    mut status_query: Query<&mut Text2d, (With<StatusText>, Without<ResponseText>)>,
    mut response_query: Query<&mut Text2d, (With<ResponseText>, Without<StatusText>)>,
) {
    // Update status
    for mut text in status_query.iter_mut() {
        **text = "Sending HTTP request...".to_string();
    }
    
    // Clear previous response
    for mut text in response_query.iter_mut() {
        **text = "".to_string();
    }
    
    // Make the async HTTP request
    info!("Starting HTTP request...");
    
    // Use spawn_blocking_task to perform HTTP request in background thread
    let response_result: Option<String> = yield spawn_blocking_task(move || {
        let mut response = ureq::get("https://httpbin.org/json").call().unwrap();
        response.body_mut().read_to_string().ok()
    });
    
    // Process the response
    match response_result {
        Some(body) => {
            info!("HTTP request successful!");
            
            // Update status
            for mut text in status_query.iter_mut() {
                **text = "Request successful! Press SPACE to try again".to_string();
            }
            
            // Show response (truncate if too long)
            for mut text in response_query.iter_mut() {
                let display_text = if body.len() > 500 {
                    format!("{}...", &body[..500])
                } else {
                    body.clone()
                };
                **text = format!("Response:\n{}", display_text);
            }
        }
        None => {
            error!("HTTP request failed!");
            
            // Update status
            for mut text in status_query.iter_mut() {
                **text = "Request failed! Press SPACE to try again".to_string();
            }
            
            for mut text in response_query.iter_mut() {
                **text = "Error: Failed to fetch data".to_string();
            }
        }
    }
    
    info!("Coroutine completed!");
}
