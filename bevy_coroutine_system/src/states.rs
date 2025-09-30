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
    pub fn with<Marker, F>(mut self, fun: F)
    where
        F: IntoSystem<In<T>, (), Marker>,
    {
        let world = unsafe { &mut *(self.world as *mut World) };

        
        let result = self.result.take().expect("result is not set");

        // Convert the closure into a system using Bevy's built-in trait.
        let mut system = <F as IntoSystem<In<T>, (), Marker>>::into_system(fun);

        // Initialize the system.
        system.initialize(world);

        // Run the system with the input.
        system.run(result, world).expect("error: run system");

        // Apply any deferred commands.
        system.apply_deferred(world);
    }
}
