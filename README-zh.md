# 🚀 Bevy Coroutine System

[![Rust](https://img.shields.io/badge/rust-nightly-orange.svg)](https://www.rust-lang.org/)
[![Bevy](https://img.shields.io/badge/bevy-0.16-blue.svg)](https://bevyengine.org/)

[English](./README.md) | 中文

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
bevy_coroutine_system = "0.1.1"
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
use bevy_coroutine_system::prelude::*;
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

    app.add_plugins((DefaultPlugins, CoroutinePlugin));

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

### 协程系统的执行方式

协程系统可以通过两种方式执行，它们的行为有区别：

#### 方式1：注册并手动触发（一次性执行）

注册协程系统后，通过手动触发来执行。协程会自动连续运行直到完成：

```rust
// 注册协程系统
app.register_coroutine(my_coroutine_system, my_coroutine_system::id());

// 手动触发（例如响应按键）
fn trigger_system(mut commands: Commands, keyboard: Res<ButtonInput<KeyCode>>) {
    if keyboard.just_pressed(KeyCode::Space) {
        commands.run_system_cached(my_coroutine_system);
    }
}
```

这种方式下，协程会执行一次完整的流程直到结束。

#### 方式2：作为常规系统（循环执行）

将协程系统添加为常规的 Bevy 系统，**无需**使用 `register_coroutine`：

```rust
// 直接添加为 Update 系统
app.add_systems(Update, my_coroutine_system);
```

这种方式下，协程会不断重复执行。例如：

```rust
#[coroutine_system]
fn repeating_coroutine() {
    info!("1");
    yield sleep(Duration::from_secs(1));
    info!("2");
}
```

输出将会是：`1, 2, 1, 2, 1, 2...`（每个循环间隔1秒）

### 内置异步函数

本库提供了四个内置的异步函数，用于控制协程的执行流程：

#### 1. `sleep(duration)` - 延时等待

等待指定的时间后继续执行：

```rust
use std::time::{Duration, Instant};

// 等待1秒
let wake_time: Instant = yield sleep(Duration::from_secs(1));
// wake_time 是唤醒时的时间戳
```

#### 2. `next_frame()` - 等待下一帧

暂停执行直到下一帧：

```rust
// 等待一帧
yield next_frame();
// 返回值是 ()，通常不需要接收
```

#### 3. `noop()` - 空操作

立即返回，不执行任何操作。主要用于解决条件分支中的借用检查问题。

当在条件分支中使用 `yield` 时，如果只有部分分支有 yield，可能会遇到 "borrow may still be in use when coroutine yields" 错误：

```rust
// ❌ 错误示例
if condition {
    yield sleep(Duration::from_secs(1));  // 只有一个分支有 yield
}
// 使用参数时报错

// ✅ 正确示例
if condition {
    yield sleep(Duration::from_secs(1));
}
yield noop(); // 确保所有控制流路径都有 yield 点
```

#### 4. `spawn_blocking_task(closure)` - 执行阻塞任务

在后台线程中执行阻塞代码，避免阻塞游戏主线程。可以执行文件I/O、网络请求、长时间计算等操作：

```rust
let response: String = yield spawn_blocking_task(move || {
    // 这里可以安全地执行阻塞操作
});
```

- 任务在单独的线程中执行，不会阻塞游戏主线程
- 协程会在每帧检查线程是否完成
- 任务完成后自动恢复执行后续操作

⚠️ 这里的返回值类型需要匹配手动确认匹配，编译不会报错，但运行时会panic!

### 获取异步操作的返回值

你可以通过明确指定类型来获取 yield 表达式的返回值：

```rust
// 明确指定返回类型
let result: std::time::Instant = yield sleep(Duration::from_secs(1));
```

⚠️ **警告**：如果指定的类型与实际返回类型不匹配，程序会 panic！请确保使用正确的类型（见上述各函数说明）。

## 🔍 工作原理

### 📋 概述

1. **🔮 过程宏转换**: `#[coroutine_system]` 宏将协程函数转换为常规的、可多次重复执行的 Bevy 系统函数
2. **💾 状态管理**: 每个协程的状态由 `CoroutineTask` 结构管理
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
    mut task: Local<CoroutineTask<CoroutineTaskInput<MyCoroutineSystemParams<'static, 'static>>>>,
    mut running_task: ResMut<RunningCoroutines>,
) {
    // 首次运行时创建协程
    if task.coroutine.is_none() {
        task.coroutine = Some(Box::pin(
            #[coroutine]
            move |mut input: CoroutineTaskInput<MyCoroutineSystemParams<'static, 'static>>| {
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
    let input = CoroutineTaskInput {
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
                return;
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
2. **📦 协程状态**: 通过 `Local<CoroutineTask>` 保存协程状态，实现跨帧持久化
3. **⚡ 异步支持**: yield 的 Future 在每帧被轮询，直到完成
4. **🔄 自动注册**: `RunningCoroutines` 资源跟踪所有活跃的协程，确保它们每帧执行

## 📚 示例

查看 `examples` 目录获取更多示例：

- 📝 `simple.rs` - 简单的协程系统示例
- 🌱 `minimal.rs` - 最小化的协程系统
- 🌐 `http_example.rs` - HTTP请求示例，演示如何使用 `spawn_blocking_task` 执行异步HTTP请求

运行示例：
```bash
cargo run --example simple
cargo run --example minimal
cargo run --example http_example
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
