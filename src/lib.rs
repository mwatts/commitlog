#![feature(test)]
// This is silly...
#![allow(unused_features)]

#[macro_use]
extern crate log;
extern crate crc;
extern crate memmap;
extern crate byteorder;
extern crate env_logger;

#[cfg(test)]
extern crate test;
#[cfg(test)]
extern crate rand;

use std::path::{Path, PathBuf};
use std::fs;
use std::io;

mod segment;
mod index;
#[cfg(test)]
mod testutil;

/// Offset of an appended log segment.
pub struct Offset(u64);

#[derive(Debug)]
pub enum AppendError {
    IoError(io::Error),
}

impl From<io::Error> for AppendError {
    fn from(e: io::Error) -> AppendError {
        AppendError::IoError(e)
    }
}

/// Commit log options allow customization of the commit
/// log behavior.
pub struct LogOptions {
    log_max_bytes: usize,
    index_max_bytes: usize,
}

impl Default for LogOptions {
    fn default() -> LogOptions {
        LogOptions {
            log_max_bytes: 100 * 1024 * 1024,
            index_max_bytes: 800_000,
        }
    }
}

impl LogOptions {
    /// Bounds the size of a log segment to a number of bytes.
    #[inline]
    pub fn max_bytes_log(&mut self, bytes: usize) -> &mut LogOptions {
        self.log_max_bytes = bytes;
        self
    }

    /// Bounds the size of an individual memory-mapped index file.
    #[inline]
    pub fn max_log_items(&mut self, items: usize) -> &mut LogOptions {
        self.index_max_bytes = items * index::INDEX_ENTRY_BYTES;
        self
    }
}

/// The commit log is an append-only data structure that can be used in a variety
/// of use-cases, such as tracking sequences of events, logging transactions in a
/// local database, or replicated state machines.
///
/// This implementation of the commit log data structure uses log segments
/// that roll over at pre-defined maximum size boundaries. The messages appended
/// to the log have a unique, monotonically incrasing offset that can be used as
/// a pointer to a log entry.
///
/// The index of the commit log logically stores the offset to a position in the
/// log segment corresponding. The index and segments are separated, in that a
/// segment file does not necessarily correspond to one particular segment file,
/// it could contain file pointers to many index files. In addition, index files
/// are memory-mapped for efficient read and write access.
pub struct CommitLog {
    active_segment: segment::Segment,
    active_index: index::Index,
    log_dir: PathBuf,
    options: LogOptions,
}

impl CommitLog {
    pub fn new<P>(log_dir: P, opts: LogOptions) -> io::Result<CommitLog>
        where P: AsRef<Path>
    {
        // TODO: figure out what's already been written to
        fs::create_dir_all(&log_dir).unwrap_or(());

        let owned_path = log_dir.as_ref().to_owned();
        info!("Opening log at path {:?}", owned_path.to_str());
        let seg = try!(segment::Segment::new(&owned_path, 0u64, opts.log_max_bytes));
        let ind = try!(index::Index::new(&owned_path, 0u64, opts.index_max_bytes));

        Ok(CommitLog {
            active_segment: seg,
            active_index: ind,
            log_dir: owned_path,
            options: opts,
        })
    }

    fn index_append(&mut self, offset: u64, pos: u32) -> Result<(), AppendError> {
        match self.active_index.append(offset, pos) {
            Ok(()) => Ok(()),
            Err(index::IndexWriteError::IndexFull) => {
                try!(self.active_index.set_readonly());
                self.active_index =
                    try!(index::Index::new(&self.log_dir, offset, self.options.index_max_bytes));
                self.index_append(offset, pos)
            }
            Err(index::IndexWriteError::OffsetLessThanBase) => unreachable!(),
        }
    }

    /// Appends a log entry to the commit log. The offset of the appended entry
    /// is the result of the comutation.
    pub fn append(&mut self, payload: &[u8]) -> Result<Offset, AppendError> {
        let meta = match self.active_segment.append(payload) {
            Ok(meta) => {
                trace!("Successfully appended message {:?}", meta);
                meta
            }
            Err(segment::SegmentAppendError::LogFull) => {
                // close segment
                try!(self.active_segment.flush_sync());
                let next_offset = self.active_segment.next_offset();
                info!("Closing segment at offset {}", next_offset);
                self.active_segment = try!(segment::Segment::new(&self.log_dir,
                                                                 next_offset,
                                                                 self.options.log_max_bytes));

                // try again
                return self.append(payload);
            }
            Err(segment::SegmentAppendError::IoError(e)) => {
                return Err(AppendError::IoError(e));
            }
        };
        try!(self.index_append(meta.offset(), meta.file_pos()));
        Ok(Offset(meta.offset()))
    }

    pub fn flush(&mut self) -> io::Result<()> {
        try!(self.active_segment.flush_sync());
        self.active_index.flush_sync()
    }
}
