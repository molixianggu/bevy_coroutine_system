use std::{
    any::Any,
    future::Future,
    pin::Pin,
    task::{Context, Poll},
};

use crate::states::States;

/// noop
/// immediately return
pub fn noop() -> impl Future<Output = States<()>> {
    struct NoopFuture;

    impl Future for NoopFuture {
        type Output = States<()>;

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            Poll::Ready(States::new((), cx.waker().data()))
        }
    }

    NoopFuture
}

/// wait for a duration
pub fn sleep(duration: std::time::Duration) -> impl Future<Output = States<std::time::Instant>> {
    struct Sleep {
        when: std::time::Instant,
    }

    impl Future for Sleep {
        type Output = States<std::time::Instant>;

        fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            let now = std::time::Instant::now();
            if self.when <= now {
                Poll::Ready(States::new(now, cx.waker().data()))
            } else {
                cx.waker().wake_by_ref();
                Poll::Pending
            }
        }
    }

    Sleep {
        when: std::time::Instant::now() + duration,
    }
}

/// wait for the next frame
/// the first poll returns Pending, the second poll returns Ready
pub fn next_frame() -> impl Future<Output = States<()>> {
    struct NextFrameFuture {
        first_poll: bool,
    }

    impl Future for NextFrameFuture {
        type Output = States<()>;

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
            if self.first_poll {
                self.first_poll = false;
                Poll::Pending
            } else {
                Poll::Ready(States::new((), cx.waker().data()))
            }
        }
    }

    NextFrameFuture { first_poll: true }
}

/// a generic Future for executing blocking tasks in a background thread
struct ThreadFuture<T> {
    handle: Option<std::thread::JoinHandle<T>>,
}

impl<T: Send + 'static> Future for ThreadFuture<T> {
    type Output = States<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> std::task::Poll<Self::Output> {
        let this = self.get_mut();
        if let Some(handle) = &this.handle {
            if handle.is_finished() {
                // thread finished, get the result
                let handle = this.handle.take().unwrap();
                match handle.join() {
                    Ok(result) => Poll::Ready(States::new(result, cx.waker().data())),
                    Err(_) => panic!("Thread panicked"),
                }
            } else {
                // thread is still running
                Poll::Pending
            }
        } else {
            // handle has been taken, this should not happen
            panic!("ThreadFuture polled after completion");
        }
    }
}

/// a generic function for executing blocking tasks in a background thread and returning a Future
pub fn spawn_blocking_task<F, T>(task: F) -> impl Future<Output = States<T>>
where
    F: FnOnce() -> T + Send + 'static,
    T: Send + Any + 'static,
{
    /// a wrapper Future for converting the output type to Box<dyn Any + Send>
    struct AnyFuture<T> {
        inner: ThreadFuture<T>,
    }

    impl<T: Send + Any + 'static> Future for AnyFuture<T> {
        type Output = States<T>;

        fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> std::task::Poll<Self::Output> {
            match Pin::new(&mut self.inner).poll(cx) {
                Poll::Ready(value) => Poll::Ready(value),
                Poll::Pending => Poll::Pending,
            }
        }
    }
    let handle = std::thread::spawn(task);

    AnyFuture {
        inner: ThreadFuture {
            handle: Some(handle),
        },
    }
}
