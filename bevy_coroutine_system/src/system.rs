use std::{
    future::Future,
    pin::Pin,
    task::{Context, Poll, RawWaker, RawWakerVTable, Waker},
};

use bevy_ecs::prelude::*;

pub struct AsyncTask<Fut> {
    future: Option<Pin<Box<Fut>>>,
}

impl<Fut> Default for AsyncTask<Fut> {
    fn default() -> Self {
        Self { future: None }
    }
}

pub fn async_system<F, Fut>(async_fn: F) -> impl FnMut(&mut World, Local<AsyncTask<Fut>>) -> Result<()>
where
    F: Fn() -> Fut + Clone + 'static,
    Fut: Future<Output = Result<()>> + Send + 'static,
{
    move |world: &mut World, mut task: Local<AsyncTask<Fut>>| -> Result<()> {
        if task.future.is_none() {
            task.future = Some(Box::pin(async_fn()));
        }

        if let Some(future) = &mut task.future {
            let waker = unsafe {
                let waker_data = world as *mut _ as *const ();
                Waker::from_raw(RawWaker::new(waker_data, &VTABLE))
            };
            let mut context = Context::from_waker(&waker);

            match future.as_mut().poll(&mut context) {
                Poll::Pending => {}
                Poll::Ready(r) => {
                    task.future = None;
                    r?;
                }
            }
        }

        Ok(())
    }
}

// copy from task\wake.rs RawWaker::NOOP
const VTABLE: RawWakerVTable = RawWakerVTable::new(
    |_| NOOP,
    |_| {},
    |_| {},
    |_| {},
);
const NOOP: RawWaker = RawWaker::new(std::ptr::null(), &VTABLE);
