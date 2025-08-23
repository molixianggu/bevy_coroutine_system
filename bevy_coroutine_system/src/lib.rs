//! Bevy 协程系统的主要库
//! 
//! 该库提供了在Bevy系统中使用协程的能力，允许系统在多帧执行并暂停/恢复。
//!
//! # 特性
//! 
//! - 🎮 **多帧执行**: 系统可以跨多个游戏帧执行
//! - ⏸️ **暂停/恢复**: 支持在任意点暂停执行并在后续帧恢复
//! - 🔄 **异步操作**: 内置对异步操作的支持（如延时等待）
//! - 🛠️ **简单易用**: 通过宏自动处理复杂的生命周期和状态管理
//!
//! # 快速开始
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
//!     // 第一帧执行
//!     for mut transform in query.iter_mut() {
//!         transform.translation.x += 10.0;
//!     }
//!     
//!     // 暂停1秒
//!     yield sleep(Duration::from_secs(1));
//!     
//!     // 恢复后继续执行
//!     for mut transform in query.iter_mut() {
//!         transform.translation.y += 10.0;
//!     }
//! }
//! ```

#![feature(coroutines, coroutine_trait)]

use bevy::prelude::*;
use bevy::ecs::system::SystemId;
use std::any::Any;
use std::collections::HashMap;
use std::ops::Coroutine;
use std::pin::Pin;
use std::ptr::NonNull;
use std::future::Future;

// 重新导出过程宏
pub use bevy_coroutine_system_macro::*;


/// Bevy 协程系统插件
/// 
/// 添加此插件以启用协程系统功能
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
    /// 注册一个协程系统
    /// 
    /// # 参数
    /// - `system`: 协程系统函数
    /// - `system_id`: 系统的唯一标识符（通过 `system_name::id()` 获取）
    /// 
    /// # 返回值
    /// 返回注册后的 SystemId
    fn register_coroutine<M>(&mut self, system: impl IntoSystem<(), (), M> + 'static, system_id: &'static str) -> SystemId;
}

impl CoroutineSystem for App {
    fn register_coroutine<M>(&mut self, system: impl IntoSystem<(), (), M> + 'static, system_id: &'static str) -> SystemId {
        let id = self.world_mut().register_system_cached(system);
        self.world_mut().resource_mut::<RunningCoroutines>().register_systems.insert(system_id, id);
        id
    }
}


/// 协程任务的容器
pub struct CoroutineTask<R> {
    /// 协程实例
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
    /// 当前挂起的Future
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

/// 协程的输入参数
pub struct CoroutineTaskInput<T> {
    /// 使用裸指针传递任意类型的数据，避免生命周期限制
    pub data_ptr: Option<NonNull<T>>,
    /// 异步操作的结果
    pub async_result: Option<Box<dyn Any + Send>>,
}

// 手动实现 Debug，避免 NonNull 的限制
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
    /// 获取数据的可变引用
    /// 
    /// # Safety
    /// 调用者必须确保裸指针仍然有效
    pub fn data_mut(&mut self) -> &mut T {
        self.data_ptr
            .map(|mut ptr| unsafe { ptr.as_mut() })
            .expect("TaskInput data_ptr is None")
    }
    
    /// 获取异步结果并进行类型转换
    /// 
    /// # Panics
    /// 如果类型转换失败会panic
    pub fn result<R: 'static>(&mut self) -> R {
        self.async_result
            .take()
            .and_then(|v| v.downcast::<R>().ok().map(|b| *b))
            .expect("Failed to downcast async result")
    }
}

/// 管理所有运行中的协程任务
#[derive(Resource, Default)]
pub struct RunningCoroutines {
    /// 活跃的协程任务
    pub systems: HashMap<&'static str, ()>,
    /// 注册的系统ID
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

/// 创建一个睡眠Future
/// 
/// # Example
/// ```rust,ignore
/// yield sleep(Duration::from_secs(1));
/// ```
pub fn sleep(duration: std::time::Duration) -> Pin<Box<dyn Future<Output = Box<dyn Any + Send>> + Send>> {
    use std::time::Instant;
    
    struct SleepFuture {
        target_time: Instant,
    }
    
    impl Future for SleepFuture {
        type Output = Box<dyn Any + Send>;
        
        fn poll(self: Pin<&mut Self>, _cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
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

/// 创建一个等待下一帧的Future
/// 
/// 第一次poll时返回Pending，第二次poll时返回Ready
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
        
        fn poll(mut self: Pin<&mut Self>, _cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
            if self.first_poll {
                self.first_poll = false;
                std::task::Poll::Pending
            } else {
                std::task::Poll::Ready(Box::new(()) as Box<dyn Any + Send>)
            }
        }
    }
    
    Box::pin(NextFrameFuture {
        first_poll: true,
    })
}

/// 创建一个空操作（no-op）的 Future
/// 
/// 这个函数立即返回，不执行任何操作。主要用于在协程中创建一个 yield 点，
/// 帮助解决借用检查问题
/// 
/// # Example
/// ```rust,ignore
/// // 在两个可能有借用冲突的代码块之间使用
/// yield noop();
/// ```
pub fn noop() -> Pin<Box<dyn Future<Output = Box<dyn Any + Send>> + Send>> {
    struct NoopFuture;
    
    impl Future for NoopFuture {
        type Output = Box<dyn Any + Send>;
        
        fn poll(self: Pin<&mut Self>, _cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
            std::task::Poll::Ready(Box::new(()) as Box<dyn Any + Send>)
        }
    }
    
    Box::pin(NoopFuture)
}


/// 一个通用的Future，用于在后台线程中执行阻塞任务
struct ThreadFuture<T> {
    handle: Option<std::thread::JoinHandle<T>>,
}

impl<T: Send + 'static> Future for ThreadFuture<T> {
    type Output = T;
    
    fn poll(self: Pin<&mut Self>, _cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        let this = self.get_mut();
        if let Some(handle) = &this.handle {
            if handle.is_finished() {
                // 线程完成，获取结果
                let handle = this.handle.take().unwrap();
                match handle.join() {
                    Ok(result) => std::task::Poll::Ready(result),
                    Err(_) => panic!("Thread panicked"),
                }
            } else {
                // 线程还在运行
                std::task::Poll::Pending
            }
        } else {
            // handle已经被取走，这不应该发生
            panic!("ThreadFuture polled after completion");
        }
    }
}

/// 一个包装Future，用于将输出类型转换为Box<dyn Any + Send>
struct AnyFuture<T> {
    inner: ThreadFuture<T>,
}

impl<T: Send + Any + 'static> Future for AnyFuture<T> {
    type Output = Box<dyn Any + Send>;
    
    fn poll(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        match Pin::new(&mut self.inner).poll(cx) {
            std::task::Poll::Ready(value) => std::task::Poll::Ready(Box::new(value) as Box<dyn Any + Send>),
            std::task::Poll::Pending => std::task::Poll::Pending,
        }
    }
}

/// 一个通用的函数，用于在后台线程中执行阻塞任务并返回一个Future
/// 
/// # Example
/// ```rust,ignore
/// let result: String = yield spawn_blocking_task(move || {
///     // 阻塞任务
///     // ...
///     return "result";
/// });
pub fn spawn_blocking_task<F, T>(task: F) -> Pin<Box<dyn Future<Output = Box<dyn Any + Send>> + Send>>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + Any + 'static,
{
    let handle = std::thread::spawn(task);
    
    Box::pin(AnyFuture {
        inner: ThreadFuture {
            handle: Some(handle),
        }
    })
}

/// yield_async!宏（已废弃，推荐使用原生 yield 语法）
/// 
/// 现在可以直接使用原生的 yield 语法：
/// ```rust,ignore
/// // 推荐的新语法
/// let result: Instant = yield sleep(Duration::from_secs(1));
/// 
/// // 旧语法（仍然支持）
/// let result: Instant = yield_async!(sleep(Duration::from_secs(1)));
/// ```
#[macro_export]
#[deprecated(since = "0.2.0", note = "使用原生 yield 语法代替")]
macro_rules! yield_async {
    ($fut:expr) => {{
    }};
}

/// 预导入模块，包含常用的类型和功能
/// 
/// # Example
/// ```rust,ignore
/// use bevy_coroutine_system::prelude::*;
/// ```
pub mod prelude {
    pub use crate::{
        // Trait
        CoroutineSystem,
        
        // 宏（从 bevy_coroutine_system_macro 重新导出）
        coroutine_system,
        
        // 插件
        CoroutinePlugin,
        
        // 函数
        sleep,
        next_frame,
        noop,
        spawn_blocking_task,
        
        // 类型
        CoroutineTask,
        CoroutineTaskInput,
        RunningCoroutines,
    };
}
