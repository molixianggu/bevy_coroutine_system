use bevy::prelude::*;
use stable_unpin_test::*;
use std::time::{Duration, Instant};

mod async_ops;
use async_ops::sleep;

use crate::async_ops::{next_frame, spawn_blocking_task};

// mod xxx;

// 使用我们的协程宏
#[test_coroutine_system]
fn test_coroutine_system(
    mut commands: Commands,
    mut query: Query<&mut Transform>,
    mut i: Local<i32>,
) {

    println!("测试协程系统: i = {:?}", i);

    *i += 1;

    for transform in query.iter() {
        println!("transform: {}", transform.translation);
    }

    // {
    //     yield_op! {
    //         sleep(Duration::from_secs(1))
    //     };
    // }
    // if *i < 3 {
    //     yield_op! {
    //         sleep(Duration::from_secs(1))
    //     };
    // }


    // match *i {
    //     0 => {
    //         yield_op! {
    //             sleep(Duration::from_secs(1))
    //         };
    //     }
    //     _ => {}
    // }

    // for _ in 0..30 {
    //     yield_op! {
    //         next_frame()
    //     };
    //     for mut transform in query.iter_mut() {
    //         transform.translation.x += 1.0;
    //     }
    // }

    // if let Some(c) = Some(2) {
    //     println!("c = {}", c);
    //     yield_op! {
    //         next_frame()
    //     };
    //     println!("c = {}", c);
    // }

    // if *i < 10 {
    //     for _ in 0..30 {
    //         yield_op! {
    //             next_frame()
    //         };
    //         for mut transform in query.iter_mut() {
    //             transform.translation.x += 1.0;
    //         }
    //     }
    // }

    let entity: Entity = commands.spawn(Transform::default()).id();

    // 独立的yield_op!语句，支持多个语句
    yield_op! {
        println!("第一个yield之前");
        println!("准备开始sleep");
        let start_time = std::time::Instant::now();
        println!("开始时间: {:?}", start_time);
        sleep(Duration::from_secs(1))
    };

    for mut transform in query.iter_mut() {
        transform.translation.x += 1.0;
        if transform.translation.x > 10.0 {
            transform.translation.x = 0.0;
        }
    }

    commands.entity(entity).despawn();

    yield_op! {next_frame()};

    println!("第一个yield之后");

    // 带返回值的yield
    let t: Instant = yield_op! {
        sleep(Duration::from_secs(2))
    };

    println!("第二个yield之后, 结果: {:?}", t);

    let result: u32 = yield_op! {
        println!("计算最终结果...");
        spawn_blocking_task(|| {
            10
        })
    };
    println!("最终结果: result = {}", result);
    println!("协程系统结束");
}

fn main() {
    App::new()
        .add_plugins(MinimalPlugins)
        .add_systems(Update, test_coroutine_system)
        .run();
}
