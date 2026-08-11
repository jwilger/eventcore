use super::*;
use eventcore_types::collect_events;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tempfile::TempDir;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct TestEvent {
    stream_id: StreamId,
    data: String,
}

impl Event for TestEvent {
    fn stream_id(&self) -> &StreamId {
        &self.stream_id
    }

    fn event_type_name() -> &'static str {
        "TestEvent"
    }
}

fn stream(name: &str) -> StreamId {
    StreamId::try_new(name).expect("valid stream id")
}

async fn append_one(store: &FileEventStore, stream_id: &StreamId, expected: usize, data: &str) {
    let writes = StreamWrites::new()
        .register_stream(stream_id.clone(), StreamVersion::new(expected))
        .and_then(|writes| {
            writes.append(TestEvent {
                stream_id: stream_id.clone(),
                data: data.to_string(),
            })
        })
        .expect("build writes");
    let _ = store.append_events(writes).await.expect("append succeeds");
}

fn transaction_files(events_dir: &Path) -> Vec<PathBuf> {
    let mut files: Vec<PathBuf> = fs::read_dir(events_dir)
        .expect("read events dir")
        .map(|entry| entry.expect("entry").path())
        .filter(|path| path.extension().and_then(|ext| ext.to_str()) == Some("jsonl"))
        .collect();
    files.sort();
    files
}

#[tokio::test]
async fn transaction_file_records_reserved_header_fields() {
    let dir = TempDir::new().expect("temp dir");
    let store = FileEventStore::open(dir.path()).expect("open");
    let account = stream("account-1");
    append_one(&store, &account, 0, "first").await;

    let files = transaction_files(&dir.path().join("events"));
    assert_eq!(files.len(), 1, "exactly one transaction file");
    let (header, events) = parse_transaction(&files[0]).expect("parse");

    // Header reserved fields.
    assert_eq!(header.format_version, FORMAT_VERSION);
    let stem = files[0].file_stem().and_then(|s| s.to_str()).expect("stem");
    assert_eq!(
        header.transaction_id,
        Uuid::parse_str(stem).expect("uuid stem")
    );
    assert!(
        header.parent_transaction_ids.is_empty(),
        "first transaction has no parents"
    );
    let mut expected_bases = BTreeMap::new();
    let _ = expected_bases.insert("account-1".to_string(), 0usize);
    assert_eq!(header.stream_bases, expected_bases);
    assert!(
        chrono::DateTime::parse_from_rfc3339(&header.created_at).is_ok(),
        "created_at is rfc3339"
    );
    // replica_id is the persisted machine-local identity.
    let persisted =
        fs::read_to_string(dir.path().join(".eventcore/replica_id")).expect("replica id file");
    assert_eq!(
        header.replica_id,
        Uuid::parse_str(persisted.trim()).expect("replica uuid")
    );

    // Event envelope.
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].stream_id, "account-1");
    assert_eq!(events[0].stream_version, 1);
    assert_eq!(events[0].event_type, "TestEvent");
    assert_eq!(events[0].metadata, serde_json::json!({}));
}

#[tokio::test]
async fn second_transaction_links_to_first_and_advances_base() {
    let dir = TempDir::new().expect("temp dir");
    let store = FileEventStore::open(dir.path()).expect("open");
    let account = stream("account-7");
    append_one(&store, &account, 0, "first").await;
    append_one(&store, &account, 1, "second").await;

    let files = transaction_files(&dir.path().join("events"));
    assert_eq!(files.len(), 2);
    // Files sort by UUID7 filename = write order.
    let (first, _) = parse_transaction(&files[0]).expect("parse first");
    let (second, second_events) = parse_transaction(&files[1]).expect("parse second");

    assert_eq!(
        second.parent_transaction_ids,
        vec![first.transaction_id],
        "second transaction links to the first as its parent"
    );
    let mut expected_bases = BTreeMap::new();
    let _ = expected_bases.insert("account-7".to_string(), 1usize);
    assert_eq!(second.stream_bases, expected_bases);
    assert_eq!(second_events[0].stream_version, 2);
}

#[tokio::test]
async fn reopen_rebuilds_index_from_events() {
    let dir = TempDir::new().expect("temp dir");
    let account = stream("account-9");
    {
        let store = FileEventStore::open(dir.path()).expect("open");
        append_one(&store, &account, 0, "alpha").await;
        append_one(&store, &account, 1, "beta").await;
    }
    let reopened = FileEventStore::open(dir.path()).expect("reopen");
    let stream = reopened
        .read_stream::<TestEvent>(account.clone())
        .await
        .expect("read");
    let events = collect_events(stream).await.expect("collect");
    let data: Vec<String> = events.iter().map(|event| event.data.clone()).collect();
    assert_eq!(data, vec!["alpha".to_string(), "beta".to_string()]);
}

#[tokio::test]
async fn store_lock_blocks_second_open_on_same_root() {
    let dir = TempDir::new().expect("temp dir");
    let _first = FileEventStore::open(dir.path()).expect("first open");
    let second = FileEventStore::open(dir.path());
    assert!(
        matches!(second, Err(FsEventStoreError::StoreLocked { .. })),
        "second open of a locked root must fail with StoreLocked, got {second:?}"
    );
}

#[tokio::test]
async fn non_jsonl_files_in_events_dir_are_ignored() {
    let dir = TempDir::new().expect("temp dir");
    let account = stream("account-3");
    {
        let store = FileEventStore::open(dir.path()).expect("open");
        append_one(&store, &account, 0, "only").await;
    }
    // A stray non-transaction file must not break the scan.
    fs::write(dir.path().join("events/README.txt"), "not a transaction").expect("write stray");
    let reopened = FileEventStore::open(dir.path()).expect("reopen ignores stray");
    let stream = reopened
        .read_stream::<TestEvent>(account)
        .await
        .expect("read");
    let events = collect_events(stream).await.expect("collect");
    assert_eq!(events.len(), 1);
}

#[tokio::test]
async fn open_writes_git_metadata_keeping_replica_id_out_of_git() {
    let dir = TempDir::new().expect("temp dir");
    let _store = FileEventStore::open(dir.path()).expect("open");

    let gitignore = fs::read_to_string(dir.path().join(".gitignore")).expect("gitignore");
    assert!(
        gitignore.contains("/.eventcore/"),
        "replica id must be gitignored to avoid the copy trap"
    );
    assert!(gitignore.contains("/.lock"));
    let attributes = fs::read_to_string(dir.path().join(".gitattributes")).expect("gitattributes");
    assert!(attributes.contains("merge=union"));
}

#[tokio::test]
async fn open_preserves_existing_git_metadata() {
    let dir = TempDir::new().expect("temp dir");
    {
        let _store = FileEventStore::open(dir.path()).expect("open");
    }
    fs::write(dir.path().join(".gitignore"), "custom\n").expect("write custom");
    {
        let _store = FileEventStore::open(dir.path()).expect("reopen");
    }
    assert_eq!(
        fs::read_to_string(dir.path().join(".gitignore")).expect("read"),
        "custom\n",
        "existing git metadata is preserved, not overwritten"
    );
}
