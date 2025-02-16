//! bevy_ggrs is a bevy plugin for the P2P rollback networking library GGRS.
#![forbid(unsafe_code)] // let us try

use bevy::{
    ecs::system::{Command, Resource},
    prelude::*,
    reflect::{FromType, GetTypeRegistration},
};
use ggrs::{P2PSession, P2PSpectatorSession, PlayerHandle, SessionState, SyncTestSession};
use ggrs_stage::{GGRSStage, GGRSStageResetSession};
use reflect_resource::ReflectResource;

pub(crate) mod ggrs_stage;
pub(crate) mod reflect_resource;
pub(crate) mod world_snapshot;

/// Stage label for the Custom GGRS Stage.
pub const GGRS_UPDATE: &str = "ggrs_update";

/// Defines the Session that the GGRS Plugin should expect as a resource.
/// Use `with_session_type(type)` to set accordingly.
pub enum SessionType {
    SyncTestSession,
    P2PSession,
    P2PSpectatorSession,
}

impl Default for SessionType {
    fn default() -> Self {
        SessionType::SyncTestSession
    }
}

/// Add this component to all entities you want to be loaded/saved on rollback.
/// The `id` has to be unique. Consider using the `RollbackIdProvider` resource.
#[derive(Component)]
pub struct Rollback {
    id: u32,
}

impl Rollback {
    /// Creates a new rollback tag with the given id.
    pub fn new(id: u32) -> Self {
        Self { id }
    }

    /// Returns the rollback id.
    pub const fn id(&self) -> u32 {
        self.id
    }
}

/// Provides unique ids for your Rollback components.
/// When you add the GGRS Plugin, this should be available as a resource.
#[derive(Default)]
pub struct RollbackIdProvider {
    next_id: u32,
}

impl RollbackIdProvider {
    /// Returns an unused, unique id.
    pub fn next_id(&mut self) -> u32 {
        if self.next_id == u32::MAX {
            // TODO: do something smart?
            panic!("RollbackIdProvider: u32::MAX has been reached.");
        }
        let ret = self.next_id;
        self.next_id += 1;
        ret
    }
}

/// Provides all functionality for the GGRS p2p rollback networking library.
pub struct GGRSPlugin;

impl Plugin for GGRSPlugin {
    fn build(&self, app: &mut App) {
        // ggrs stage
        app.add_stage_before(CoreStage::Update, GGRS_UPDATE, GGRSStage::new());
        // insert a rollback id provider
        app.insert_resource(RollbackIdProvider::default());
    }
}

/// Extension trait for the `App`.
pub trait GGRSApp {
    /// Adds the given `ggrs::SyncTestSession` to your app.
    fn with_synctest_session(&mut self, sess: SyncTestSession) -> &mut Self;

    /// Adds the given `ggrs::P2PSession` to your app.
    fn with_p2p_session(&mut self, sess: P2PSession) -> &mut Self;

    /// Adds the given `ggrs::P2PSpectatorSession` to your app.
    fn with_p2p_spectator_session(&mut self, sess: P2PSpectatorSession) -> &mut Self;

    /// Adds a schedule into the GGRSStage that holds the game logic systems. This schedule should contain all
    /// systems you want to be executed during frame advances.
    fn with_rollback_schedule(&mut self, schedule: Schedule) -> &mut Self;

    /// Registers a given system as the input system. This system should provide encoded inputs for a given player.
    fn with_input_system<Params>(
        &mut self,
        input_system: impl IntoSystem<PlayerHandle, Vec<u8>, Params>,
    ) -> &mut Self;

    /// Sets the fixed update frequency
    fn with_update_frequency(&mut self, update_frequency: u32) -> &mut Self;

    /// Registers a type of component for saving and loading during rollbacks.
    fn register_rollback_type<T>(&mut self) -> &mut Self
    where
        T: GetTypeRegistration + Reflect + Default + Component;

    // Inserts a resource in bevy with saving and loading during rollbacks.
    fn insert_rollback_resource<T>(&mut self, resource: T) -> &mut Self
    where
        T: GetTypeRegistration + Reflect + Default + Component + Resource;
}

impl GGRSApp for App {
    fn with_synctest_session(&mut self, session: SyncTestSession) -> &mut Self {
        self.insert_resource(SessionType::SyncTestSession);
        self.insert_resource(session);
        self
    }

    fn with_p2p_session(&mut self, session: P2PSession) -> &mut Self {
        self.insert_resource(SessionType::P2PSession);
        self.insert_resource(session);
        self
    }

    fn with_p2p_spectator_session(&mut self, session: P2PSpectatorSession) -> &mut Self {
        self.insert_resource(SessionType::P2PSpectatorSession);
        self.insert_resource(session);
        self
    }

    fn with_rollback_schedule(&mut self, schedule: Schedule) -> &mut Self {
        let ggrs_stage = self
            .schedule
            .get_stage_mut::<GGRSStage>(&GGRS_UPDATE)
            .expect("No GGRSStage found! Did you install the GGRSPlugin?");
        ggrs_stage.set_schedule(schedule);
        self
    }

    fn with_input_system<Params>(
        &mut self,
        input_system: impl IntoSystem<PlayerHandle, Vec<u8>, Params>,
    ) -> &mut Self {
        let mut input_system = input_system.system();
        input_system.initialize(&mut self.world);
        let ggrs_stage = self
            .schedule
            .get_stage_mut::<GGRSStage>(&GGRS_UPDATE)
            .expect("No GGRSStage found! Did you install the GGRSPlugin?");
        ggrs_stage.input_system = Some(Box::new(input_system));
        self
    }

    fn with_update_frequency(&mut self, update_frequency: u32) -> &mut Self {
        let ggrs_stage = self
            .schedule
            .get_stage_mut::<GGRSStage>(&GGRS_UPDATE)
            .expect("No GGRSStage found! Did you install the GGRSPlugin?");
        ggrs_stage.set_update_frequency(update_frequency);
        self
    }

    fn register_rollback_type<T>(&mut self) -> &mut Self
    where
        T: GetTypeRegistration + Reflect + Default + Component,
    {
        let ggrs_stage = self
            .schedule
            .get_stage_mut::<GGRSStage>(&GGRS_UPDATE)
            .expect("No GGRSStage found! Did you install the GGRSPlugin?");

        let mut registry = ggrs_stage.type_registry.write();

        registry.register::<T>();

        let registration = registry.get_mut(std::any::TypeId::of::<T>()).unwrap();
        registration.insert(<ReflectComponent as FromType<T>>::from_type());
        registration.insert(<ReflectResource as FromType<T>>::from_type());
        drop(registry);

        self
    }

    fn insert_rollback_resource<T>(&mut self, resource: T) -> &mut Self
    where
        T: GetTypeRegistration + Reflect + Default + Component + Resource,
    {
        self.insert_resource(resource).register_rollback_type::<T>()
    }
}

pub trait CommandsExt {
    fn start_p2p_session(&mut self, session: P2PSession);
    fn start_p2p_spectator_session(&mut self, session: P2PSpectatorSession);
    fn start_synctest_session(&mut self, session: SyncTestSession);
    fn stop_session(&mut self);
}

impl CommandsExt for Commands<'_, '_> {
    fn start_p2p_session(&mut self, session: P2PSession) {
        self.add(StartP2PSessionCommand(session));
    }

    fn start_p2p_spectator_session(&mut self, session: P2PSpectatorSession) {
        self.add(StartP2PSpectatorSessionCommand(session));
    }

    fn start_synctest_session(&mut self, session: SyncTestSession) {
        self.add(StartSyncTestSessionCommand(session));
    }

    fn stop_session(&mut self) {
        self.add(StopSessionCommand);
    }
}

struct StartP2PSpectatorSessionCommand(P2PSpectatorSession);

impl Command for StartP2PSessionCommand {
    fn write(mut self, world: &mut World) {
        // caller is responsible that the session is either already running...
        if self.0.current_state() == SessionState::Initializing {
            // ...or ready to be started
            self.0.start_session().unwrap();
        }
        world.insert_resource(self.0);
        world.insert_resource(SessionType::P2PSession);
    }
}

struct StartP2PSessionCommand(P2PSession);

impl Command for StartP2PSpectatorSessionCommand {
    fn write(mut self, world: &mut World) {
        // caller is responsible that the session is either already running...
        if self.0.current_state() == SessionState::Initializing {
            // ...or ready to be started
            self.0.start_session().unwrap();
        }
        world.insert_resource(self.0);
        world.insert_resource(SessionType::P2PSpectatorSession);
    }
}

struct StartSyncTestSessionCommand(SyncTestSession);

impl Command for StartSyncTestSessionCommand {
    fn write(self, world: &mut World) {
        world.insert_resource(self.0);
        world.insert_resource(SessionType::SyncTestSession);
    }
}

struct StopSessionCommand;

impl Command for StopSessionCommand {
    fn write(self, world: &mut World) {
        world.remove_resource::<SessionType>();
        world.remove_resource::<P2PSession>();
        world.remove_resource::<SyncTestSession>();
        world.remove_resource::<P2PSpectatorSession>();
        world.insert_resource(GGRSStageResetSession);
    }
}
