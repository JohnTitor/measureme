use crate::file_header::{write_file_header, FILE_MAGIC_EVENT_STREAM};
use crate::raw_event::{RawEvent, Timestamp, TimestampKind};
use crate::serialization::SerializationSink;
use crate::stringtable::{SerializableString, StringId, StringTableBuilder};
use std::error::Error;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;

pub struct ProfilerFiles {
    pub events_file: PathBuf,
    pub string_data_file: PathBuf,
    pub string_index_file: PathBuf,
}

impl ProfilerFiles {
    pub fn new(path_stem: &Path) -> ProfilerFiles {
        ProfilerFiles {
            events_file: path_stem.with_extension("events"),
            string_data_file: path_stem.with_extension("string_data"),
            string_index_file: path_stem.with_extension("string_index"),
        }
    }
}

pub struct Profiler<S: SerializationSink> {
    event_sink: Arc<S>,
    string_table: StringTableBuilder<S>,
    start_time: Instant,
}

impl<S: SerializationSink> Profiler<S> {
    pub fn new(path_stem: &Path) -> Result<Profiler<S>, Box<dyn Error>> {
        let paths = ProfilerFiles::new(path_stem);
        let event_sink = Arc::new(S::from_path(&paths.events_file)?);

        // The first thing in every file we generate must be the file header.
        write_file_header(&*event_sink, FILE_MAGIC_EVENT_STREAM);

        let string_table = StringTableBuilder::new(
            Arc::new(S::from_path(&paths.string_data_file)?),
            Arc::new(S::from_path(&paths.string_index_file)?),
        );

        let profiler = Profiler {
            event_sink,
            string_table,
            start_time: Instant::now(),
        };

        let mut args = String::new();
        for arg in std::env::args() {
            args.push_str(&arg.escape_default().to_string());
            args.push(' ');
        }

        profiler.string_table.alloc_metadata(&*format!(
            r#"{{ "start_time": {}, "process_id": {}, "cmd": "{}" }}"#,
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
            std::process::id(),
            args,
        ));

        Ok(profiler)
    }

    #[inline(always)]
    pub fn alloc_string_with_reserved_id<STR: SerializableString + ?Sized>(
        &self,
        id: StringId,
        s: &STR,
    ) -> StringId {
        self.string_table.alloc_with_reserved_id(id, s)
    }

    #[inline(always)]
    pub fn alloc_string<STR: SerializableString + ?Sized>(&self, s: &STR) -> StringId {
        self.string_table.alloc(s)
    }

    /// Records an event with the given parameters. The event time is computed
    /// automatically.
    pub fn record_event(
        &self,
        event_kind: StringId,
        event_id: StringId,
        thread_id: u64,
        timestamp_kind: TimestampKind,
    ) {
        let duration_since_start = self.start_time.elapsed();
        let nanos_since_start = duration_since_start.as_secs() * 1_000_000_000
            + duration_since_start.subsec_nanos() as u64;
        let timestamp = Timestamp::new(nanos_since_start, timestamp_kind);

        let raw_event = RawEvent {
            event_kind,
            id: event_id,
            thread_id,
            timestamp,
        };

        self.event_sink
            .write_atomic(std::mem::size_of::<RawEvent>(), |bytes| {
                debug_assert_eq!(bytes.len(), std::mem::size_of::<RawEvent>());

                let raw_event_bytes: &[u8] = unsafe {
                    std::slice::from_raw_parts(
                        &raw_event as *const _ as *const u8,
                        std::mem::size_of::<RawEvent>(),
                    )
                };

                bytes.copy_from_slice(raw_event_bytes);
            });
    }

    /// Creates a "start" event and returns a `TimingGuard` that will create
    /// the corresponding "end" event when it is dropped.
    pub fn start_recording_interval_event<'a>(
        &'a self,
        event_kind: StringId,
        event_id: StringId,
        thread_id: u64,
    ) -> TimingGuard<'a, S> {
        self.record_event(event_kind, event_id, thread_id, TimestampKind::Start);

        TimingGuard {
            profiler: self,
            event_id,
            event_kind,
            thread_id,
        }
    }
}

/// When dropped, this `TimingGuard` will record an "end" event in the
/// `Profiler` it was created by.
#[must_use]
pub struct TimingGuard<'a, S: SerializationSink> {
    profiler: &'a Profiler<S>,
    event_id: StringId,
    event_kind: StringId,
    thread_id: u64,
}

impl<'a, S: SerializationSink> Drop for TimingGuard<'a, S> {
    #[inline]
    fn drop(&mut self) {
        self.profiler.record_event(
            self.event_kind,
            self.event_id,
            self.thread_id,
            TimestampKind::End,
        );
    }
}
