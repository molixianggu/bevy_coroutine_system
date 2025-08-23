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

#[derive(Event, Clone)]
struct AnEvent;

/// 测试不同类型的输入
/// 都可以被宏正确解析
/// 保证修改过程中，功能没有被破坏
#[coroutine_system]
fn simple_coroutine(
    time: Res<Time>,
    mut event_writer: EventWriter<AnEvent>,
    mut event_reader: EventReader<AnEvent>,
    query: Query<&Transform, With<Player>>,
    query_2: Query<&H, With<Player>>,
    mut commands: Commands,
    a: Local<i32>,
) {}
