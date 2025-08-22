# 🚀 Bevy Coroutine System

[![Rust](https://img.shields.io/badge/rust-nightly-orange.svg)](https://www.rust-lang.org/)
[![Bevy](https://img.shields.io/badge/bevy-0.16-blue.svg)](https://bevyengine.org/)

[English](./README-en.md) | 中文

一个为 Bevy 游戏引擎设计的协程系统，允许系统在多帧执行并支持暂停/恢复。

> <s>实现丑陋，但是有用的东西</s>

## ✨ 特性

- 🎮 **多帧执行**: 系统可以跨多个游戏帧执行
- ⏸️ **暂停/恢复**: 支持在任意点暂停执行并在后续帧恢复
- 🔄 **异步操作**: 内置对异步操作的支持（如延时等待）
- 🛠️ **简单易用**: 通过宏自动处理复杂的生命周期和状态管理
- 🔓 **非独占访问**: 不需要独占 World，只借用需要的系统参数
- 🔃 **实时数据更新**: 每次 yield 恢复后，自动获取最新的组件数据
- 🎯 **非拷贝**: 直接遍历原始组件数据，无需额外的数据拷贝

## 📦 安装

⚠️ **注意**: 该库需要 Rust nightly 版本，因为使用了不稳定的协程特性。

### 1️⃣ 添加依赖

```toml
[dependencies]
bevy = "0.16"
bevy_coroutine_system = { path = "path/to/bevy_coroutine_system" }
```

### 2️⃣ 设置 nightly 工具链

```bash
rustup override set nightly
```

### 3️⃣ 启用必需的 feature flags

在你的 crate 根文件（`main.rs` 或 `lib.rs`）的顶部添加：

```rust
#![feature(coroutines, coroutine_trait)]
```

⚠️ **重要**：这些 feature flags 是必需的，因为宏生成的代码会使用 `yield` 语法和协程相关类型。如果不添加，编译会失败并提示缺少这些特性。

## 🎯 基础用法

```rust
#![feature(coroutines, coroutine_trait)]

use bevy::prelude::*;
use bevy_coroutine_system::{coroutine_system, sleep, plugin, CoroutineSystem};
use std::time::Duration;

#[coroutine_system]
fn my_coroutine_system(
    mut commands: Commands,
    mut query: Query<&mut Transform>,
) {
    // 第一帧执行
    for mut transform in query.iter_mut() {
        transform.translation.x += 10.0;
    }
    
    // 暂停1秒（支持原生 yield 语法）
    yield sleep(Duration::from_secs(1));
    
    // 恢复后继续执行
    for mut transform in query.iter_mut() {
        transform.translation.y += 10.0;
    }
}

fn main() {
    let mut app = App::new();
    
    app.add_plugins((DefaultPlugins, plugin));
    
    // 注册协程系统
    app.register_coroutine(my_coroutine_system, my_coroutine_system::id());
    
    // 添加触发系统
    app.add_systems(Update, trigger_coroutine);
    
    app.run();
}

fn trigger_coroutine(
    mut commands: Commands,
    keyboard: Res<ButtonInput<KeyCode>>,
) {
    if keyboard.just_pressed(KeyCode::Space) {
        // 按空格键触发协程
        commands.run_system_cached(my_coroutine_system);
    }
}
```

## 🔍 工作原理

### 📋 概述

1. **🔮 过程宏转换**: `#[coroutine_system]` 宏将普通函数转换为协程系统
2. **💾 状态管理**: 每个协程的状态由 `Task` 结构管理
3. **🔗 参数传递**: 使用裸指针机制绕过 Bevy 的生命周期限制
4. **⚡ 异步集成**: Future 在每帧被轮询直到完成

### 🔬 宏展开示例

当你编写这样的协程系统：

```rust
#[coroutine_system]
fn my_coroutine_system(
    mut query: Query<&mut Transform>,
) {
    // 修改位置
    for mut transform in query.iter_mut() {
        transform.translation.x += 10.0;
    }
    
    // 暂停1秒
    yield sleep(Duration::from_secs(1));
    
    // 恢复后继续
    for mut transform in query.iter_mut() {
        transform.translation.y += 10.0;
    }
}
```

宏会将其展开为类似这样的伪代码：

<details>
<summary>🔽 点击查看展开后的代码</summary>

```rust
// 自动生成的参数结构体
#[derive(SystemParam)]
struct MyCoroutineSystemParams<'w, 's> {
    query: Query<'w, 's, &mut Transform>,
}

// 实际的系统函数
fn my_coroutine_system<'w, 's>(
    params: MyCoroutineSystemParams<'w, 's>,
    mut task: Local<Task<TaskInput<MyCoroutineSystemParams<'static, 'static>>>>,
    mut running_task: ResMut<RunningTask>,
) {
    // 首次运行时创建协程
    if task.coroutine.is_none() {
        task.coroutine = Some(Box::pin(
            #[coroutine]
            move |mut input: TaskInput<MyCoroutineSystemParams<'static, 'static>>| {
                // 获取参数的裸指针
                let params = input.data_mut();
                let query = &mut params.query;
                
                // 原始函数体的第一部分
                for mut transform in query.iter_mut() {
                    transform.translation.x += 10.0;
                }
                
                // yield 表达式被转换为协程的 yield
                input = yield sleep(Duration::from_secs(1));
                
                // yield 后重新获取参数（重要！）
                let params = input.data_mut();
                let query = &mut params.query;
                
                // 原始函数体的剩余部分
                for mut transform in query.iter_mut() {
                    transform.translation.y += 10.0;
                }
            }
        ));
        
        // 标记系统为运行中
        running_task.systems.insert(my_coroutine_system::id(), ());
    }
    
    // 处理异步操作（如sleep）
    let mut async_result = None;
    if let Some(fut) = &mut task.fut {
        // 轮询Future
        match fut.as_mut().poll(&mut Context::from_waker(&Waker::noop())) {
            Poll::Ready(v) => {
                async_result = Some(v);
                task.fut = None;
            }
            Poll::Pending => return, // Future未完成，下帧继续
        }
    }
    
    // 创建协程输入，包含参数指针和异步结果
    let input = TaskInput {
        data_ptr: Some(unsafe { NonNull::new_unchecked(&params as *const _ as *mut _) }),
        async_result,
    };
    
    // 恢复协程执行
    if let Some(coroutine) = &mut task.coroutine {
        match coroutine.as_mut().resume(input) {
            CoroutineState::Yielded(future) => {
                // 协程yield了一个Future，保存起来下帧继续
                task.fut = Some(future);
            }
            CoroutineState::Complete(()) => {
                // 协程执行完毕，清理状态
                task.coroutine = None;
                running_task.systems.remove(my_coroutine_system::id());
            }
        }
    }
}

// 生成的模块，提供唯一ID
pub mod my_coroutine_system {
    pub fn id() -> &'static str {
        concat!(module_path!(), "::my_coroutine_system")
    }
}
```

</details>

### 🔑 关键机制

1. **🔐 生命周期处理**: 使用裸指针(`NonNull`)传递参数，绕过 Rust 的生命周期检查
2. **📦 协程状态**: 通过 `Local<Task>` 保存协程状态，实现跨帧持久化
3. **⚡ 异步支持**: yield 的 Future 在每帧被轮询，直到完成
4. **🔄 自动注册**: `RunningTask` 资源跟踪所有活跃的协程，确保它们每帧执行

## 📚 示例

查看 `examples` 目录获取更多示例：

- 📝 `simple.rs` - 简单的协程系统示例
- 🌱 `minimal.rs` - 最小化的协程系统

运行示例：
```bash
cargo run --example simple
cargo run --example minimal
```

## ⚠️ 限制

- 🔧 需要 Rust nightly 版本
- 🚧 协程特性仍处于实验阶段
- 💡 使用不安全的裸指针传递参数
- 📊 宏覆盖范围有限，有些参数可能没有及时支持

## 🤝 贡献

欢迎贡献！请随时提交 Issue 或 Pull Request。

## 📄 License

MIT OR Apache-2.0