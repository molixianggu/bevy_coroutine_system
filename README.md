# 🚀 Bevy Coroutine System

[![Rust](https://img.shields.io/badge/rust-nightly-orange.svg)](https://www.rust-lang.org/)
[![Bevy](https://img.shields.io/badge/bevy-0.16-blue.svg)](https://bevyengine.org/)

English | [中文](./README-zh.md)

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
bevy_coroutine_system = "0.1.1"
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
use bevy_coroutine_system::prelude::*;
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

    app.add_plugins((DefaultPlugins, CoroutinePlugin));

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

### Execution Methods for Coroutine Systems

Coroutine systems can be executed in two ways, with behavioral differences:

#### Method 1: Register and Trigger Manually (One-time Execution)

After registering a coroutine system, execute it through manual triggering. The coroutine will run continuously until completion:

```rust
// Register the coroutine system
app.register_coroutine(my_coroutine_system, my_coroutine_system::id());

// Manual trigger (e.g., responding to keyboard input)
fn trigger_system(mut commands: Commands, keyboard: Res<ButtonInput<KeyCode>>) {
    if keyboard.just_pressed(KeyCode::Space) {
        commands.run_system_cached(my_coroutine_system);
    }
}
```

In this mode, the coroutine executes once through its complete flow until it finishes.

#### Method 2: As a Regular System (Loop Execution)

Add the coroutine system as a regular Bevy system, **without** using `register_coroutine`:

```rust
// Add directly as an Update system
app.add_systems(Update, my_coroutine_system);
```

In this mode, the coroutine will execute repeatedly. For example:

```rust
#[coroutine_system]
fn repeating_coroutine() {
    info!("1");
    yield sleep(Duration::from_secs(1));
    info!("2");
}
```

The output will be: `1, 2, 1, 2, 1, 2...` (with a 1-second interval between each loop)

### Built-in Async Functions

This library provides four built-in async functions to control coroutine execution flow:

#### 1. `sleep(duration)` - Timed Delay

Wait for a specified duration before continuing:

```rust
use std::time::{Duration, Instant};

// Wait for 1 second
let wake_time: Instant = yield sleep(Duration::from_secs(1));
// wake_time is the timestamp when awakened
```

#### 2. `next_frame()` - Wait for Next Frame

Pause execution until the next frame:

```rust
// Wait for one frame
yield next_frame();
// Returns (), usually no need to capture the result
```

#### 3. `noop()` - No Operation

Returns immediately without doing anything. Mainly used to solve borrow checker issues in conditional branches.

When using `yield` in conditional branches where only some branches have yield, you may encounter "borrow may still be in use when coroutine yields" error:

```rust
// ❌ Incorrect example
if condition {
    yield sleep(Duration::from_secs(1));  // Only one branch has yield
}
// Error when using parameters

// ✅ Correct example
if condition {
    yield sleep(Duration::from_secs(1));
}
yield noop(); // Ensures all control flow paths have a yield point
```

#### 4. `spawn_blocking_task(closure)` - Execute Blocking Task

Execute blocking code in a background thread to avoid blocking the main game thread. Can be used for file I/O, network requests, long computations, etc.:

```rust
let response: String = yield spawn_blocking_task(move || {
    // It's safe to execute blocking operations here
});
```

- The task runs in a separate thread, won't block the main game thread
- The coroutine checks each frame if the thread has completed
- Automatically resumes execution after the task completes

⚠️ The return type here needs to be manually confirmed to match. It won't cause a compilation error, but will panic at runtime if incorrect!

### Getting Return Values from Async Operations

You can get return values from yield expressions by explicitly specifying the type:

```rust
// Explicitly specify return type
let result: std::time::Instant = yield sleep(Duration::from_secs(1));
```

⚠️ **Warning**: If the specified type doesn't match the actual return type, the program will panic! Make sure to use the correct types (see the function descriptions above).

## 🔍 How It Works

### 📋 Overview

1. **🔮 Procedural Macro Transformation**: The `#[coroutine_system]` macro transforms coroutine functions into regular, repeatable Bevy system functions
2. **💾 State Management**: Each coroutine's state is managed by the `CoroutineTask` structure
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
    mut task: Local<CoroutineTask<CoroutineTaskInput<MyCoroutineSystemParams<'static, 'static>>>>,
    mut running_task: ResMut<RunningCoroutines>,
) {
    // Create coroutine on first run
    if task.coroutine.is_none() {
        task.coroutine = Some(Box::pin(
            #[coroutine]
            move |mut input: CoroutineTaskInput<MyCoroutineSystemParams<'static, 'static>>| {
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
    let input = CoroutineTaskInput {
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
                return;
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
2. **📦 Coroutine State**: Saves coroutine state via `Local<CoroutineTask>` for cross-frame persistence
3. **⚡ Async Support**: Yielded Futures are polled each frame until completion
4. **🔄 Auto Registration**: `RunningCoroutines` resource tracks all active coroutines, ensuring they execute each frame

## 📚 Examples

Check the `examples` directory for more examples:

- 📝 `simple.rs` - Simple coroutine system example
- 🌱 `minimal.rs` - Minimal coroutine system
- 🌐 `http_example.rs` - HTTP request example, demonstrates how to use `spawn_blocking_task` to execute async HTTP requests

Run examples:
```bash
cargo run --example simple
cargo run --example minimal
cargo run --example http_example
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
