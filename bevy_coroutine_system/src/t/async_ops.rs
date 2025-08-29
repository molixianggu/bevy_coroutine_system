use std::{any::Any, pin::Pin, future::Future, time::{Duration, Instant}};

/// 异步sleep函数，用于在协程中暂停执行指定的时间
pub fn sleep(duration: Duration) -> Pin<Box<dyn Future<Output = Instant> + Send>> {
    struct SleepFuture {
        target_time: Instant,
    }
    
    impl Future for SleepFuture {
        type Output = Instant;
        
        fn poll(self: Pin<&mut Self>, _cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
            if Instant::now() >= self.target_time {
                std::task::Poll::Ready(Instant::now())
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
pub fn next_frame() -> Pin<Box<dyn Future<Output = ()> + Send>> {
    struct NextFrameFuture {
        first_poll: bool,
    }
    
    impl Future for NextFrameFuture {
        type Output = ();
        
        fn poll(mut self: Pin<&mut Self>, _cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
            if self.first_poll {
                self.first_poll = false;
                std::task::Poll::Pending
            } else {
                std::task::Poll::Ready(())
            }
        }
    }
    
    Box::pin(NextFrameFuture {
        first_poll: true,
    })
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
    type Output = T;
    
    fn poll(mut self: Pin<&mut Self>, cx: &mut std::task::Context<'_>) -> std::task::Poll<Self::Output> {
        match Pin::new(&mut self.inner).poll(cx) {
            std::task::Poll::Ready(value) => std::task::Poll::Ready(value),
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
pub fn spawn_blocking_task<F, T>(task: F) -> Pin<Box<dyn Future<Output = T> + Send>>
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
