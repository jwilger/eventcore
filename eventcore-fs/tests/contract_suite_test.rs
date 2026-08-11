//! Runs the shared EventCore backend contract suite against `eventcore-fs`.
//!
//! Each factory roots its store in a fresh temporary directory that the
//! returned wrapper owns, so the directory outlives the store for the whole
//! test (and is cleaned up on drop) without leaking.

mod fs_contract_suite {
    use eventcore_fs::{FileCheckpointStore, FileEventStore, FileProjectorCoordinator};
    use eventcore_testing::contract::backend_contract_tests;
    use eventcore_types::{
        CheckpointStore, Event, EventFilter, EventPage, EventReader, EventStore, EventStoreError,
        EventStream, EventStreamSlice, ProjectorCoordinator, StreamId, StreamPosition,
        StreamWrites,
    };
    use tempfile::TempDir;

    struct TempStore {
        _dir: TempDir,
        inner: FileEventStore,
    }

    impl EventStore for TempStore {
        async fn read_stream<E: Event>(
            &self,
            stream_id: StreamId,
        ) -> Result<EventStream<E>, EventStoreError> {
            self.inner.read_stream(stream_id).await
        }

        async fn read_stream_after<E: Event>(
            &self,
            stream_id: StreamId,
            exclusive_version: eventcore_types::StreamVersion,
        ) -> Result<EventStream<E>, EventStoreError> {
            self.inner
                .read_stream_after(stream_id, exclusive_version)
                .await
        }

        async fn append_events(
            &self,
            writes: StreamWrites,
        ) -> Result<EventStreamSlice, EventStoreError> {
            self.inner.append_events(writes).await
        }

        async fn load_command_state_snapshot(
            &self,
            snapshot_id: eventcore_types::CommandStateSnapshotId,
        ) -> Result<Option<eventcore_types::CommandStateSnapshot>, EventStoreError> {
            self.inner.load_command_state_snapshot(snapshot_id).await
        }

        async fn save_command_state_snapshot(
            &self,
            snapshot_id: eventcore_types::CommandStateSnapshotId,
            snapshot: eventcore_types::CommandStateSnapshot,
        ) -> Result<(), EventStoreError> {
            self.inner
                .save_command_state_snapshot(snapshot_id, snapshot)
                .await
        }
    }

    impl EventReader for TempStore {
        type Error = EventStoreError;

        async fn read_events<E: Event>(
            &self,
            filter: EventFilter,
            page: EventPage,
        ) -> Result<Vec<(E, StreamPosition)>, Self::Error> {
            self.inner.read_events(filter, page).await
        }
    }

    struct TempCheckpoint {
        _dir: TempDir,
        inner: FileCheckpointStore,
    }

    impl CheckpointStore for TempCheckpoint {
        type Error = <FileCheckpointStore as CheckpointStore>::Error;

        async fn load(&self, name: &str) -> Result<Option<StreamPosition>, Self::Error> {
            self.inner.load(name).await
        }

        async fn save(&self, name: &str, position: StreamPosition) -> Result<(), Self::Error> {
            self.inner.save(name, position).await
        }
    }

    struct TempCoordinator {
        _dir: TempDir,
        inner: FileProjectorCoordinator,
    }

    impl ProjectorCoordinator for TempCoordinator {
        type Error = <FileProjectorCoordinator as ProjectorCoordinator>::Error;
        type Guard = <FileProjectorCoordinator as ProjectorCoordinator>::Guard;

        async fn try_acquire(&self, subscription_name: &str) -> Result<Self::Guard, Self::Error> {
            self.inner.try_acquire(subscription_name).await
        }
    }

    fn make_store() -> TempStore {
        let dir = TempDir::new().expect("create temp dir");
        let inner = FileEventStore::open(dir.path()).expect("open file event store");
        TempStore { _dir: dir, inner }
    }

    fn make_checkpoint_store() -> TempCheckpoint {
        let dir = TempDir::new().expect("create temp dir");
        let inner = FileCheckpointStore::open(dir.path()).expect("open file checkpoint store");
        TempCheckpoint { _dir: dir, inner }
    }

    fn make_coordinator() -> TempCoordinator {
        let dir = TempDir::new().expect("create temp dir");
        let inner = FileProjectorCoordinator::open(dir.path()).expect("open file coordinator");
        TempCoordinator { _dir: dir, inner }
    }

    backend_contract_tests! {
        suite = fs,
        make_store = || { crate::fs_contract_suite::make_store() },
        make_checkpoint_store = || { crate::fs_contract_suite::make_checkpoint_store() },
        make_coordinator = || { crate::fs_contract_suite::make_coordinator() },
    }
}
