//! An example to showcase that the macro can handle varied inputs.

#![feature(coroutines, coroutine_trait)]

use bevy::prelude::*;
use bevy_coroutine_system::prelude::*;

fn main() {
    println!("OK PASS!");
}

#[derive(Component)]
struct H(());

#[derive(Component)]
struct Player;

#[derive(Message, Clone)]
struct AnEvent;

/// Test different types of inputs
//// All can be correctly parsed by the macro
/// Ensure functionality isn't broken during modifications
#[coroutine_system]
fn simple_coroutine(
    time: Res<Time>,
    mut event_writer: MessageWriter<AnEvent>,
    mut event_reader: MessageReader<AnEvent>,
    query: Query<&Transform, With<Player>>,
    query_2: Query<&H, With<Player>>,
    mut commands: Commands,
    a: Local<i32>,
) {
}
