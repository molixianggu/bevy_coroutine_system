//! Main library for the Bevy coroutine system
//!
//! This crate provides coroutine capabilities for Bevy systems, allowing systems to run across multiple frames with pause/resume.
//!
//! # Features
//!
//! - 🎮 Multi-frame Execution: Systems can execute across multiple game frames
//! - ⏸️ Pause/Resume: Support pausing at any point and resuming in subsequent frames
//! - 🔄 Async Operations: Built-in support for asynchronous operations (e.g., timed delays)
//! - 🛠️ Easy to Use: Automatically handles complex lifecycle and state management via macros
//!
//! # Quick Start
//!
//! ```rust,ignore
//! #![feature(coroutines, coroutine_trait)]
//!
//! use bevy::prelude::*;
//! use bevy_coroutine_system::prelude::*;
//! use std::time::Duration;
//!
//! #[coroutine_system]
//! fn my_coroutine_system(
//!     mut commands: Commands,
//!     mut query: Query<&mut Transform>,
//! ) {
//!     // Execute on the first frame
//!     for mut transform in query.iter_mut() {
//!         transform.translation.x += 10.0;
//!     }
//!
//!     // Pause for 1 second
//!     yield sleep(Duration::from_secs(1));
//!
//!     // Continue after resume
//!     for mut transform in query.iter_mut() {
//!         transform.translation.y += 10.0;
//!     }
//! }
//! ```

#![feature(coroutines, coroutine_trait)]

use bevy::ecs::system::SystemId;
use bevy::prelude::*;
use std::any::Any;
use std::collections::HashMap;
use std::future::Future;
use std::ops::Coroutine;
use std::pin::Pin;
use std::ptr::NonNull;

// Re-export procedural macros
pub use bevy_coroutine_system_macro::*;

/// Bevy Coroutine System plugin
///
/// Add this plugin to enable coroutine system functionality
///
/// # Example
/// ```rust,ignore
/// app.add_plugins(CoroutinePlugin);
/// ```
pub struct CoroutinePlugin;

impl Plugin for CoroutinePlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<RunningCoroutines>()
            .add_systems(Update, update_running_tasks);
    }
}

pub trait CoroutineSystem {
    /// Register a coroutine system
    ///
    /// # Parameters
    /// - `system`: The coroutine system function
    /// - `system_id`: The unique identifier for the system (obtained via `system_name::id()`)
    ///
    /// # Returns
    /// The registered SystemId
    fn register_coroutine<M>(
        &mut self,
        system: impl IntoSystem<(), (), M> + 'static,
        system_id: &'static str,
    ) -> SystemId;
}

impl CoroutineSystem for App {
    fn register_coroutine<M>(
        &mut self,
        system: impl IntoSystem<(), (), M> + 'static,
        system_id: &'static str,
    ) -> SystemId {
        let id = self.world_mut().register_system_cached(system);
        self.world_mut()
            .resource_mut::<RunningCoroutines>()
            .register_systems
            .insert(system_id, id);
        id
    }
}

/// Container for a coroutine task
pub struct CoroutineTask<R> {
    /// Coroutine instance
    pub coroutine: Option<
        Pin<
            Box<
                dyn Coroutine<
                        R,
                        Yield = Pin<Box<dyn Future<Output = Box<dyn Any + Send>> + Send>>,
                        Return = (),
                    > + Send,
            >,
        >,
    >,
    /// The currently pending Future
    pub fut: Option<Pin<Box<dyn Future<Output = Box<dyn Any + Send>> + Send>>>,
}

impl<R> Default for CoroutineTask<R> {
    fn default() -> Self {
        Self {
            coroutine: None,
            fut: None,
        }
    }
}

/// Input parameters for the coroutine
pub struct CoroutineTaskInput<T> {
    /// Use a raw pointer to pass arbitrary data, avoiding lifetime restrictions
    pub data_ptr: Option<NonNull<T>>,
    /// Result of the async operation
    pub async_result: Option<Box<dyn Any + Send>>,
}

// Manually implement Debug to avoid NonNull limitations
impl<T> std::fmt::Debug for CoroutineTaskInput<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CoroutineTaskInput")
            .field("data_ptr", &self.data_ptr.is_some())
            .field("async_result", &self.async_result.is_some())
            .finish()
    }
}

unsafe impl<T: Send> Send for CoroutineTaskInput<T> {}

impl<T> CoroutineTaskInput<T> {
    /// Get a mutable reference to the data
    ///
    /// # Safety
    /// The caller must ensure the raw pointer remains valid
    pub fn data_mut(&mut self) -> &mut T {
        self.data_ptr
            .map(|mut ptr| unsafe { ptr.as_mut() })
            .expect("TaskInput data_ptr is None")
    }

    /// Retrieve the async result and downcast it
    ///
    /// # Panics
    /// Panics if the downcast fails
    pub fn result<R: 'static>(&mut self) -> R {
        self.async_result
            .take()
            .and_then(|v| v.downcast::<R>().ok().map(|b| *b))
            .expect("Failed to downcast async result")
    }
}

/// Manage all running coroutine tasks
#[derive(Resource, Default)]
pub struct RunningCoroutines {
    /// Active coroutine tasks
    pub systems: HashMap<&'static str, ()>,
    /// Registered system IDs
    pub register_systems: HashMap<&'static str, SystemId>,
}

fn update_running_tasks(mut commands: Commands, running_task: Res<RunningCoroutines>) {
    if running_task.systems.is_empty() {
        return;
    }
    for (system_name, system_id) in running_task.register_systems.iter() {
        if running_task.systems.contains_key(system_name) {
            commands.run_system(*system_id);
        }
    }
}

/// Create a sleep Future
///
/// # Example
/// ```rust,ignore
/// yield sleep(Duration::from_secs(1));
/// ```
pub fn sleep(
    duration: std::time::Duration,
) -> Pin<Box<dyn Future<Output = Box<dyn Any + Send>> + Send>> {
    use std::time::Instant;

    struct SleepFuture {
        target_time: Instant,
    }

    impl Future for SleepFuture {
        type Output = Box<dyn Any + Send>;

        fn poll(
            self: Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Self::Output> {
            if Instant::now() >= self.target_time {
                std::task::Poll::Ready(Box::new(Instant::now()) as Box<dyn Any + Send>)
            } else {
                std::task::Poll::Pending
            }
        }
    }

    Box::pin(SleepFuture {
        target_time: Instant::now() + duration,
    })
}

/// Create a Future that waits for the next frame
///
/// Returns Pending on the first poll and Ready on the second
///
/// # Example
/// ```rust,ignore
/// yield next_frame();
/// ```
pub fn next_frame() -> Pin<Box<dyn Future<Output = Box<dyn Any + Send>> + Send>> {
    struct NextFrameFuture {
        first_poll: bool,
    }

    impl Future for NextFrameFuture {
        type Output = Box<dyn Any + Send>;

        fn poll(
            mut self: Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Self::Output> {
            if self.first_poll {
                self.first_poll = false;
                std::task::Poll::Pending
            } else {
                std::task::Poll::Ready(Box::new(()) as Box<dyn Any + Send>)
            }
        }
    }

    Box::pin(NextFrameFuture { first_poll: true })
}

/// Create a no-op Future
///
/// This returns immediately and performs no work. It's mainly used to create a yield point in a coroutine,
/// helping to resolve borrow checker issues
///
/// # Example
/// ```rust,ignore
/// // Use between two code blocks that may conflict with borrows
/// yield noop();
/// ```
pub fn noop() -> Pin<Box<dyn Future<Output = Box<dyn Any + Send>> + Send>> {
    struct NoopFuture;

    impl Future for NoopFuture {
        type Output = Box<dyn Any + Send>;

        fn poll(
            self: Pin<&mut Self>,
            _cx: &mut std::task::Context<'_>,
        ) -> std::task::Poll<Self::Output> {
            std::task::Poll::Ready(Box::new(()) as Box<dyn Any + Send>)
        }
    }

    Box::pin(NoopFuture)
}

/// A generic Future used to run a blocking task on a background thread
struct ThreadFuture<T> {
    handle: Option<std::thread::JoinHandle<T>>,
}

impl<T: Send + 'static> Future for ThreadFuture<T> {
    type Output = T;

    fn poll(
        self: Pin<&mut Self>,
        _cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        let this = self.get_mut();
        if let Some(handle) = &this.handle {
            if handle.is_finished() {
                // Thread completed; get the result
                let handle = this.handle.take().unwrap();
                match handle.join() {
                    Ok(result) => std::task::Poll::Ready(result),
                    Err(_) => panic!("Thread panicked"),
                }
            } else {
                // Thread is still running
                std::task::Poll::Pending
            }
        } else {
            // Handle has already been taken; this should not happen
            panic!("ThreadFuture polled after completion");
        }
    }
}

/// A wrapper Future that converts the output to Box<dyn Any + Send>
struct AnyFuture<T> {
    inner: ThreadFuture<T>,
}

impl<T: Send + Any + 'static> Future for AnyFuture<T> {
    type Output = Box<dyn Any + Send>;

    fn poll(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Self::Output> {
        match Pin::new(&mut self.inner).poll(cx) {
            std::task::Poll::Ready(value) => {
                std::task::Poll::Ready(Box::new(value) as Box<dyn Any + Send>)
            }
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}

/// A generic function that runs a blocking task in a background thread and returns a Future
///
/// # Example
/// ```rust,ignore
/// let result: String = yield spawn_blocking_task(move || {
///     // Blocking task
///     // ...
///     return "result";
/// });
pub fn spawn_blocking_task<F, T>(
    task: F,
) -> Pin<Box<dyn Future<Output = Box<dyn Any + Send>> + Send>>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + Any + 'static,
{
    let handle = std::thread::spawn(task);

    Box::pin(AnyFuture {
        inner: ThreadFuture {
            handle: Some(handle),
        },
    })
}

/// yield_async! macro (deprecated; prefer native yield syntax)
///
/// You can now use the native yield syntax directly:
/// ```rust,ignore
/// // Preferred new syntax
/// let result: Instant = yield sleep(Duration::from_secs(1));
///
/// // Legacy syntax (still supported)
/// let result: Instant = yield_async!(sleep(Duration::from_secs(1)));
/// ```
#[macro_export]
#[deprecated(since = "0.2.0", note = "use native yield syntax instead")]
macro_rules! yield_async {
    ($fut:expr) => {{}};
}

/// Prelude module with commonly used types and functions
///
/// # Example
/// ```rust,ignore
/// use bevy_coroutine_system::prelude::*;
/// ```
pub mod prelude {
    pub use crate::{
        // Plugin
        CoroutinePlugin,

        // Trait
        CoroutineSystem,

        // Types
        CoroutineTask,
        CoroutineTaskInput,
        RunningCoroutines,
        // Macro (re-exported from bevy_coroutine_system_macro)
        coroutine_system,
        next_frame,
        noop,
        // Functions
        sleep,
        spawn_blocking_task,
    };
}
