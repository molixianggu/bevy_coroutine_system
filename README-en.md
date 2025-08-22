# 🚀 Bevy Coroutine System

[![Rust](https://img.shields.io/badge/rust-nightly-orange.svg)](https://www.rust-lang.org/)
[![Bevy](https://img.shields.io/badge/bevy-0.16-blue.svg)](https://bevyengine.org/)

English | [中文](./README.md)

A coroutine system designed for the Bevy game engine, allowing systems to execute across multiple frames with pause/resume support.

> <s>Ugly implementation, but useful stuff</s>

## ✨ Features

- 🎮 **Multi-frame Execution**: Systems can execute across multiple game frames
- ⏸️ **Pause/Resume**: Support for pausing execution at any point and resuming in subsequent frames
- 🔄 **Async Operations**: Built-in support for asynchronous operations (e.g., timed delays)
- 🛠️ **Easy to Use**: Automatically handles complex lifecycle and state management through macros
- 🔓 **Non-exclusive Access**: No need for exclusive World access, only borrows required system parameters
- 🔃 **Real-time Data Updates**: Automatically fetches the latest component data after each yield resume
- 🎯 **No-copy**: Directly iterates over raw component data without additional copying

## 📦 Installation

⚠️ **Note**: This library requires Rust nightly version due to the use of unstable coroutine features.

### 1️⃣ Add Dependencies

```toml
[dependencies]
bevy = "0.16"
bevy_coroutine_system = { path = "path/to/bevy_coroutine_system" }
```

### 2️⃣ Set up Nightly Toolchain

```bash
rustup override set nightly
```

### 3️⃣ Enable Required Feature Flags

Add the following at the top of your crate root file (`main.rs` or `lib.rs`):

```rust
#![feature(coroutines, coroutine_trait)]
```

⚠️ **Important**: These feature flags are required because the macro-generated code uses `yield` syntax and coroutine-related types. Without them, compilation will fail with missing feature errors.

## 🎯 Basic Usage

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
    // Execute on first frame
    for mut transform in query.iter_mut() {
        transform.translation.x += 10.0;
    }
    
    // Pause for 1 second (supports native yield syntax)
    yield sleep(Duration::from_secs(1));
    
    // Continue execution after resume
    for mut transform in query.iter_mut() {
        transform.translation.y += 10.0;
    }
}

fn main() {
    let mut app = App::new();
    
    app.add_plugins((DefaultPlugins, plugin));
    
    // Register the coroutine system
    app.register_coroutine(my_coroutine_system, my_coroutine_system::id());
    
    // Add trigger system
    app.add_systems(Update, trigger_coroutine);
    
    app.run();
}

fn trigger_coroutine(
    mut commands: Commands,
    keyboard: Res<ButtonInput<KeyCode>>,
) {
    if keyboard.just_pressed(KeyCode::Space) {
        // Trigger coroutine on spacebar press
        commands.run_system_cached(my_coroutine_system);
    }
}
```

## 🔍 How It Works

### 📋 Overview

1. **🔮 Procedural Macro Transformation**: The `#[coroutine_system]` macro transforms regular functions into coroutine systems
2. **💾 State Management**: Each coroutine's state is managed by the `Task` structure
3. **🔗 Parameter Passing**: Uses raw pointer mechanism to bypass Bevy's lifetime restrictions
4. **⚡ Async Integration**: Futures are polled each frame until completion

### 🔬 Macro Expansion Example

When you write a coroutine system like this:

```rust
#[coroutine_system]
fn my_coroutine_system(
    mut query: Query<&mut Transform>,
) {
    // Modify position
    for mut transform in query.iter_mut() {
        transform.translation.x += 10.0;
    }
    
    // Pause for 1 second
    yield sleep(Duration::from_secs(1));
    
    // Continue after resume
    for mut transform in query.iter_mut() {
        transform.translation.y += 10.0;
    }
}
```

The macro expands it to something like this pseudocode:

<details>
<summary>🔽 Click to view expanded code</summary>

```rust
// Auto-generated parameter struct
#[derive(SystemParam)]
struct MyCoroutineSystemParams<'w, 's> {
    query: Query<'w, 's, &mut Transform>,
}

// Actual system function
fn my_coroutine_system<'w, 's>(
    params: MyCoroutineSystemParams<'w, 's>,
    mut task: Local<Task<TaskInput<MyCoroutineSystemParams<'static, 'static>>>>,
    mut running_task: ResMut<RunningTask>,
) {
    // Create coroutine on first run
    if task.coroutine.is_none() {
        task.coroutine = Some(Box::pin(
            #[coroutine]
            move |mut input: TaskInput<MyCoroutineSystemParams<'static, 'static>>| {
                // Get raw pointer to parameters
                let params = input.data_mut();
                let query = &mut params.query;
                
                // First part of original function body
                for mut transform in query.iter_mut() {
                    transform.translation.x += 10.0;
                }
                
                // yield expression is converted to coroutine yield
                input = yield sleep(Duration::from_secs(1));
                
                // Re-fetch parameters after yield (important!)
                let params = input.data_mut();
                let query = &mut params.query;
                
                // Remaining part of original function body
                for mut transform in query.iter_mut() {
                    transform.translation.y += 10.0;
                }
            }
        ));
        
        // Mark system as running
        running_task.systems.insert(my_coroutine_system::id(), ());
    }
    
    // Handle async operations (like sleep)
    let mut async_result = None;
    if let Some(fut) = &mut task.fut {
        // Poll the Future
        match fut.as_mut().poll(&mut Context::from_waker(&Waker::noop())) {
            Poll::Ready(v) => {
                async_result = Some(v);
                task.fut = None;
            }
            Poll::Pending => return, // Future not ready, continue next frame
        }
    }
    
    // Create coroutine input with parameter pointer and async result
    let input = TaskInput {
        data_ptr: Some(unsafe { NonNull::new_unchecked(&params as *const _ as *mut _) }),
        async_result,
    };
    
    // Resume coroutine execution
    if let Some(coroutine) = &mut task.coroutine {
        match coroutine.as_mut().resume(input) {
            CoroutineState::Yielded(future) => {
                // Coroutine yielded a Future, save it for next frame
                task.fut = Some(future);
            }
            CoroutineState::Complete(()) => {
                // Coroutine completed, clean up state
                task.coroutine = None;
                running_task.systems.remove(my_coroutine_system::id());
            }
        }
    }
}

// Generated module providing unique ID
pub mod my_coroutine_system {
    pub fn id() -> &'static str {
        concat!(module_path!(), "::my_coroutine_system")
    }
}
```

</details>

### 🔑 Key Mechanisms

1. **🔐 Lifetime Handling**: Uses raw pointers (`NonNull`) to pass parameters, bypassing Rust's lifetime checks
2. **📦 Coroutine State**: Saves coroutine state via `Local<Task>` for cross-frame persistence
3. **⚡ Async Support**: Yielded Futures are polled each frame until completion
4. **🔄 Auto Registration**: `RunningTask` resource tracks all active coroutines, ensuring they execute each frame

## 📚 Examples

Check the `examples` directory for more examples:

- 📝 `simple.rs` - Simple coroutine system example
- 🌱 `minimal.rs` - Minimal coroutine system

Run examples:
```bash
cargo run --example simple
cargo run --example minimal
```

## ⚠️ Limitations

- 🔧 Requires Rust nightly version
- 🚧 Coroutine features are still experimental
- 💡 Uses unsafe raw pointers for parameter passing
- 📊 Limited macro coverage, some parameters might not be supported yet

## 🤝 Contributing

Contributions are welcome! Feel free to submit Issues or Pull Requests.

## 📄 License

MIT OR Apache-2.0
