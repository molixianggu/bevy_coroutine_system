use bevy_ecs::{
    prelude::*,
    system::{In, IntoSystem, System},
};

pub struct States<T> {
    result: Option<T>,
    world: *const (),
}

impl<T: 'static> States<T> {
    pub(crate) fn new(result: T, data: *const ()) -> Self {
        Self {
            result: Some(result),
            world: data,
        }
    }
    pub fn with<Marker, F, R>(mut self, fun: F) -> Result<R>
    where
        F: IntoSystem<In<T>, Result<R>, Marker>,
    {
        let world = unsafe { &mut *(self.world as *mut World) };

        let result = self.result.take().expect("result is not set");

        // Convert the closure into a system using Bevy's built-in trait.
        let mut system = <F as IntoSystem<In<T>, Result<R>, Marker>>::into_system(fun);

        // Initialize the system.
        system.initialize(world);

        // Run the system with the input.
        let r = match system.run(result, world) {
            Ok(r) => r,
            Err(e) => return Err(e),
        };

        // Apply any deferred commands.
        system.apply_deferred(world);

        match r {
            Ok(r) => Ok(r),
            Err(e) => Err(e),
        }
    }
}
