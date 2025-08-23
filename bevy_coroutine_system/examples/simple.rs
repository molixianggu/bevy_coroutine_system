//! 简化的协程系统示例

#![feature(coroutines, coroutine_trait)]

use bevy::prelude::*;
use bevy_coroutine_system::prelude::*;
use std::time::Duration;

fn main() {
    // 创建一个简单的Bevy应用
    let mut app = App::new();

    app.add_plugins((DefaultPlugins, CoroutinePlugin));

    app.world_mut()
        .spawn((Transform::from_xyz(0.0, 0.0, 0.0), Hp(100), Player));

    // 注册协程
    app.register_coroutine(simple_coroutine, simple_coroutine::id());

    app.add_systems(Update, key_input);

    app.run();
}

fn key_input(mut commands: Commands, keyboard_input: Res<ButtonInput<KeyCode>>) {
    if keyboard_input.just_pressed(KeyCode::Space) {
        commands.run_system_cached(simple_coroutine);
    }
}

#[derive(Component)]
struct Hp(i32);

#[derive(Component)]
struct Player;

#[coroutine_system]
fn simple_coroutine(
    mut commands: Commands,
    mut query: Query<(&Transform, &mut Hp), With<Player>>,
    time: Res<Time>,
) {
    info!("Coroutine started at {:?}!", time.elapsed());

    // 查询并修改玩家血量
    for (transform, mut hp) in query.iter_mut() {
        info!("Player at {:?} has {} HP", transform.translation, hp.0);
        hp.0 += 10;
        info!("Healed player to {} HP", hp.0);
    }

    // 创建一个实体
    let entity = commands
        .spawn((Transform::from_xyz(100.0, 0.0, 0.0), Hp(50)))
        .id();
    info!("Created entity: {:?}", entity);

    // 暂停1秒
    info!("Yielding for 1 second...");
    let t: std::time::Instant = yield sleep(Duration::from_secs(1));

    // 恢复后继续
    info!("Coroutine resumed at {:?} ! {:?}", time.elapsed(), t);

    // 再次查询并修改玩家血量
    for (transform, mut hp) in query.iter_mut() {
        info!("Player at {:?} now has {} HP", transform.translation, hp.0);
        hp.0 -= 5;
        info!("Damaged player to {} HP", hp.0);
    }

    yield next_frame();

    // 再创建一个实体
    let entity2 = commands
        .spawn((Transform::from_xyz(-100.0, 0.0, 0.0), Hp(30)))
        .id();
    info!("Created another entity: {:?}", entity2);

    info!("Coroutine completed!");
}

#[derive(Event)]
struct AEvent;

#[coroutine_system]
fn simple_coroutine_2(time: Res<Time>, mut event_writer: EventWriter<AEvent>, mut event_reader: EventReader<AEvent>, mut query: Query<&Transform, With<Player>>, mut commands: Commands, a: Local<i32>) {
}

