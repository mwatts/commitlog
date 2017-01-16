extern crate commitlog;
extern crate env_logger;

use commitlog::*;
use std::time::{self, SystemTime};

const BATCH_SIZE: u32 = 200;
const BATCHES: u32 = 10000;

fn main() {
    env_logger::init().unwrap();

    // open a directory called 'log' for segment and index storage
    let opts = LogOptions::new(format!(".log{}",
                                       SystemTime::now()
                                           .duration_since(time::UNIX_EPOCH)
                                           .unwrap()
                                           .as_secs()));
    let mut log = CommitLog::new(opts).unwrap();

    let start = SystemTime::now();
    for i in 0..BATCHES {
        let mut buf = (0..BATCH_SIZE).map(|j| format!("{}-{}", i, j)).collect::<MessageBuf>();
        log.append(&mut buf).expect("Unable to append batch");

        if i == 99 || i == 50 {
            log.flush().expect("Unable to flush");
        }
    }

    let end = SystemTime::now();
    println!("Appended {} messages in {:?}",
             BATCH_SIZE * BATCHES,
             end.duration_since(start));

    // read the log
    let start = SystemTime::now();
    let mut total = 0;
    let mut iterations = 0;
    let mut pos = ReadPosition::Beginning;
    loop {
        let entries = log.read(pos, ReadLimit::Bytes(10240))
            .expect("Unable to read messages from the log");
        match entries.last_offset() {
            Some(off) => {
                iterations += 1;
                total += entries.len();
                if let ReadPosition::Offset(Offset(prev)) = pos {
                    assert!(prev < off.0);
                }
                // TODO: re-enable read position based on log file positions
                // rather than offset
                //pos = ReadPosition::Position(entries.next_read_position().unwrap());

                pos = ReadPosition::Offset(Offset(off.0 + 1));
            }
            None => {
                let end = SystemTime::now();
                println!("Read {} messages in {:?}, {} iterations",
                         total,
                         end.duration_since(start),
                         iterations);
                break;
            }

        }
    }
}
