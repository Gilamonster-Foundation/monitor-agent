//! An append-only, hash-linked journal of metric snapshots.
//!
//! Monty's metric history used to live in a 60-sample `VecDeque` in RAM, which
//! meant two things: nothing survived a restart, and there was no series long
//! enough to forecast from. This crate is the durable half.
//!
//! # Shape
//!
//! Every snapshot is stored as a [`MerkleNode<MetricSet>`] — the payload plus
//! the id of the snapshot before it — minted by the `content-addressable`
//! crate and written as its canonical dag-cbor. Identity is *derived from the
//! bytes*, never assigned: change any earlier snapshot and every later id
//! changes, so "this series is what was actually collected" is a proof over
//! bytes rather than a claim.
//!
//! On disk, in one directory:
//!
//! ```text
//! journal.log   u32be frame length || canonical dag-cbor of the node, repeated
//! HEAD          "<cid> <acknowledged log length in bytes>"
//! ```
//!
//! One log, one chain, one writer. The daemon collects sequentially in a single
//! task, so there is no contention to design around.
//!
//! # Reading verifies
//!
//! [`Journal::frames`] re-derives every id from its own bytes and checks that
//! each frame links the one before it. A mismatch is an error, not a log line —
//! evidence nobody reads is decoration.
//!
//! # Crash safety
//!
//! A frame is written and flushed *before* `HEAD` advances, and `HEAD` is
//! replaced atomically. A crash between the two leaves an unacknowledged tail,
//! which [`Journal::open`] truncates: the last sample is lost, the chain is
//! not. If `HEAD` is lost entirely, `open` rebuilds it by verifying the log.

use std::fs::{self, File, OpenOptions};
use std::io::{BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use content_addressable::{canonical, ContentAddressable, ContentId, MerkleNode};
use monitor_core::metrics::MetricSet;

const LOG_FILE: &str = "journal.log";
const HEAD_FILE: &str = "HEAD";

/// Largest frame we will read. A metric snapshot is a few KB; anything past
/// this is a corrupt length prefix, and we refuse rather than allocate it.
const MAX_FRAME: u32 = 8 * 1024 * 1024;

/// Something went wrong reading or writing the journal.
#[derive(Debug, thiserror::Error)]
pub enum JournalError {
    #[error("journal i/o: {0}")]
    Io(#[from] std::io::Error),

    #[error("journal encoding: {0}")]
    Content(#[from] content_addressable::ContentError),

    /// A frame's parent link does not name the frame before it. The chain has
    /// been edited, reordered, or spliced.
    ///
    /// Boxed because two `ContentId`s inline make every `Result` in the crate
    /// pay for the rarest error.
    #[error(
        "chain broken at byte {}: frame links {:?}, expected {:?}",
        .0.position, .0.found, .0.expected
    )]
    ChainBroken(Box<ChainBreak>),

    /// The log ends mid-frame: a truncated write we cannot attribute.
    #[error("truncated frame at byte {position}: wanted {wanted} bytes, found {found}")]
    Truncated {
        position: u64,
        wanted: usize,
        found: usize,
    },

    /// A length prefix larger than [`MAX_FRAME`].
    #[error("implausible frame length {length} at byte {position}")]
    FrameTooLarge { position: u64, length: u32 },

    /// `HEAD` is not parseable as "<cid> <length>".
    #[error("malformed HEAD: {0}")]
    MalformedHead(String),

    /// `HEAD` acknowledges more bytes than the log actually holds — the log was
    /// truncated underneath us, which we will not silently paper over.
    #[error("log is shorter than HEAD claims: log {log_len}, HEAD {head_len}")]
    LogShorterThanHead { log_len: u64, head_len: u64 },
}

/// Where a chain link failed to match, and what was expected instead.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChainBreak {
    /// Byte offset of the offending frame in the log.
    pub position: u64,
    /// The id the frame should have named as its parent.
    pub expected: Option<ContentId>,
    /// The id it actually named.
    pub found: Option<ContentId>,
}

/// An append-only journal of metric snapshots in one directory.
pub struct Journal {
    dir: PathBuf,
    log: File,
    head: Option<ContentId>,
    /// Bytes of `journal.log` that `HEAD` has acknowledged.
    acked: u64,
}

impl Journal {
    /// Open (creating if needed) the journal in `dir`, recovering from an
    /// interrupted write.
    pub fn open(dir: impl AsRef<Path>) -> Result<Self, JournalError> {
        let dir = dir.as_ref().to_path_buf();
        fs::create_dir_all(&dir)?;

        let log_path = dir.join(LOG_FILE);
        let mut log = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&log_path)?;
        let log_len = log.metadata()?.len();

        let (head, acked) = match read_head(&dir)? {
            Some((cid, acked)) => {
                if log_len < acked {
                    return Err(JournalError::LogShorterThanHead {
                        log_len,
                        head_len: acked,
                    });
                }
                if log_len > acked {
                    // A frame was written but never acknowledged. Drop it.
                    tracing::warn!(
                        "journal: discarding {} unacknowledged byte(s) after crash",
                        log_len - acked
                    );
                    log.set_len(acked)?;
                }
                (Some(cid), acked)
            }
            // No HEAD. If the log has content, the only honest way back is to
            // read it — which verifies it on the way.
            None if log_len > 0 => {
                tracing::warn!("journal: HEAD missing, rebuilding by verifying the log");
                let mut last = None;
                let mut acked = 0u64;
                for frame in Frames::open(&log_path)? {
                    let (id, _, end) = frame?;
                    last = Some(id);
                    acked = end;
                }
                if log_len > acked {
                    log.set_len(acked)?;
                }
                (last, acked)
            }
            None => (None, 0),
        };

        log.seek(SeekFrom::End(0))?;
        Ok(Self {
            dir,
            log,
            head,
            acked,
        })
    }

    /// The id of the newest snapshot, if any.
    pub fn head(&self) -> Option<ContentId> {
        self.head
    }

    /// Append one snapshot, linking it to the current head. Returns its id.
    pub fn append(&mut self, set: &MetricSet) -> Result<ContentId, JournalError> {
        let node = MerkleNode::new(set.clone(), self.head);
        let bytes = node.canonical_form()?;
        let id = ContentId::from_canonical_bytes(&bytes);

        let len = u32::try_from(bytes.len()).map_err(|_| JournalError::FrameTooLarge {
            position: self.acked,
            length: u32::MAX,
        })?;
        if len > MAX_FRAME {
            return Err(JournalError::FrameTooLarge {
                position: self.acked,
                length: len,
            });
        }

        // Frame first, then HEAD: a crash in between costs this sample, not the
        // chain.
        self.log.write_all(&len.to_be_bytes())?;
        self.log.write_all(&bytes)?;
        self.log.flush()?;

        let acked = self.acked + 4 + u64::from(len);
        write_head(&self.dir, &id, acked)?;

        self.head = Some(id);
        self.acked = acked;
        Ok(id)
    }

    /// Iterate the journal oldest-first, verifying every frame as it is read.
    pub fn frames(&self) -> Result<Frames, JournalError> {
        Frames::open(self.dir.join(LOG_FILE))
    }

    /// Walk the whole chain and return how many snapshots verified.
    ///
    /// This is the production read path's guarantee made explicit: it fails on
    /// the first frame whose bytes do not match the id the chain gives it.
    pub fn verify(&self) -> Result<u64, JournalError> {
        let mut n = 0;
        for frame in self.frames()? {
            frame?;
            n += 1;
        }
        Ok(n)
    }
}

/// A verifying, oldest-first iterator over the frames of a journal log.
///
/// Yields `(id, snapshot, end_offset)`. Every item has had its id re-derived
/// from its own bytes and its parent link checked against the previous frame.
pub struct Frames {
    rdr: BufReader<File>,
    pos: u64,
    prev: Option<ContentId>,
    failed: bool,
}

impl Frames {
    fn open(path: impl AsRef<Path>) -> Result<Self, JournalError> {
        Ok(Self {
            rdr: BufReader::new(File::open(path)?),
            pos: 0,
            prev: None,
            failed: false,
        })
    }

    fn next_frame(&mut self) -> Option<Result<(ContentId, MetricSet, u64), JournalError>> {
        let mut len_buf = [0u8; 4];
        match read_exact_or_eof(&mut self.rdr, &mut len_buf) {
            Ok(0) => return None,
            Ok(n) if n < 4 => {
                return Some(Err(JournalError::Truncated {
                    position: self.pos,
                    wanted: 4,
                    found: n,
                }))
            }
            Ok(_) => {}
            Err(e) => return Some(Err(e.into())),
        }

        let len = u32::from_be_bytes(len_buf);
        if len > MAX_FRAME {
            return Some(Err(JournalError::FrameTooLarge {
                position: self.pos,
                length: len,
            }));
        }

        let mut bytes = vec![0u8; len as usize];
        match read_exact_or_eof(&mut self.rdr, &mut bytes) {
            Ok(n) if n < bytes.len() => {
                return Some(Err(JournalError::Truncated {
                    position: self.pos + 4,
                    wanted: bytes.len(),
                    found: n,
                }))
            }
            Ok(_) => {}
            Err(e) => return Some(Err(e.into())),
        }

        // The checked door: decoding must reproduce these exact bytes, so the
        // value we hand back really is the value this id names.
        let node: MerkleNode<MetricSet> = match canonical::from_canonical_dagcbor_checked(&bytes) {
            Ok(n) => n,
            Err(e) => return Some(Err(e.into())),
        };
        let id = ContentId::from_canonical_bytes(&bytes);

        let found = node.parents().iter().copied().next();
        if found != self.prev || node.parents().len() > 1 {
            return Some(Err(JournalError::ChainBroken(Box::new(ChainBreak {
                position: self.pos,
                expected: self.prev,
                found,
            }))));
        }

        self.pos += 4 + u64::from(len);
        self.prev = Some(id);
        Some(Ok((id, node.payload().clone(), self.pos)))
    }
}

impl Iterator for Frames {
    type Item = Result<(ContentId, MetricSet, u64), JournalError>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.failed {
            return None;
        }
        let item = self.next_frame();
        if matches!(item, Some(Err(_))) {
            self.failed = true;
        }
        item
    }
}

/// Read until `buf` is full or EOF, returning how many bytes were read.
fn read_exact_or_eof(rdr: &mut impl Read, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        match rdr.read(&mut buf[filled..])? {
            0 => break,
            n => filled += n,
        }
    }
    Ok(filled)
}

fn read_head(dir: &Path) -> Result<Option<(ContentId, u64)>, JournalError> {
    let path = dir.join(HEAD_FILE);
    if !path.exists() {
        return Ok(None);
    }
    let text = fs::read_to_string(&path)?;
    let text = text.trim();
    let (cid_s, len_s) = text
        .split_once(char::is_whitespace)
        .ok_or_else(|| JournalError::MalformedHead(text.to_owned()))?;
    let cid = cid_s
        .parse::<ContentId>()
        .map_err(|e| JournalError::MalformedHead(format!("{cid_s}: {e}")))?;
    let len = len_s
        .trim()
        .parse::<u64>()
        .map_err(|e| JournalError::MalformedHead(format!("{len_s}: {e}")))?;
    Ok(Some((cid, len)))
}

/// Replace `HEAD` atomically — a torn HEAD is worse than a stale one.
fn write_head(dir: &Path, id: &ContentId, acked: u64) -> Result<(), JournalError> {
    let tmp = dir.join("HEAD.tmp");
    {
        let mut f = File::create(&tmp)?;
        write!(f, "{id} {acked}")?;
        f.flush()?;
    }
    fs::rename(&tmp, dir.join(HEAD_FILE))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use monitor_core::metrics::MetricPath;

    fn sample(target: &str, cpu: f64) -> MetricSet {
        let mut m = MetricSet::new(target);
        m.insert("cpu.percent", cpu);
        m.insert_with_unit("disk./.used_pct", 42.0, "%");
        m
    }

    #[test]
    fn append_then_replay_roundtrips_the_values() {
        let dir = tempfile::tempdir().unwrap();
        let mut j = Journal::open(dir.path()).unwrap();
        j.append(&sample("gnuc", 10.0)).unwrap();
        j.append(&sample("gnuc", 20.0)).unwrap();

        let got: Vec<f64> = j
            .frames()
            .unwrap()
            .map(|f| f.unwrap().1.get(&MetricPath::new("cpu.percent")).unwrap())
            .collect();
        assert_eq!(got, vec![10.0, 20.0]);
    }

    #[test]
    fn history_survives_reopen() {
        let dir = tempfile::tempdir().unwrap();
        let first = {
            let mut j = Journal::open(dir.path()).unwrap();
            j.append(&sample("gnuc", 1.0)).unwrap()
        };
        let mut j = Journal::open(dir.path()).unwrap();
        assert_eq!(j.head(), Some(first));
        j.append(&sample("gnuc", 2.0)).unwrap();
        assert_eq!(j.verify().unwrap(), 2);
    }

    #[test]
    fn each_snapshot_links_the_one_before_it() {
        let dir = tempfile::tempdir().unwrap();
        let mut j = Journal::open(dir.path()).unwrap();
        let a = j.append(&sample("gnuc", 1.0)).unwrap();
        let b = j.append(&sample("gnuc", 2.0)).unwrap();
        assert_ne!(a, b);
        assert_eq!(j.head(), Some(b));
        assert_eq!(j.verify().unwrap(), 2);
    }

    /// Identity is derived from content: the same snapshot at the same instant
    /// in the same chain position mints the same id.
    #[test]
    fn identity_is_a_function_of_content() {
        let set = sample("gnuc", 1.0);
        let d1 = tempfile::tempdir().unwrap();
        let d2 = tempfile::tempdir().unwrap();
        let a = Journal::open(d1.path()).unwrap().append(&set).unwrap();
        let b = Journal::open(d2.path()).unwrap().append(&set).unwrap();
        assert_eq!(a, b);
    }

    /// The point of the whole crate: editing history is detected.
    #[test]
    fn tampering_with_a_stored_value_is_caught_on_read() {
        let dir = tempfile::tempdir().unwrap();
        let mut j = Journal::open(dir.path()).unwrap();
        j.append(&sample("gnuc", 10.0)).unwrap();
        j.append(&sample("gnuc", 20.0)).unwrap();
        drop(j);

        // Flip a byte in the first frame's payload region.
        let path = dir.path().join(LOG_FILE);
        let mut raw = fs::read(&path).unwrap();
        let mid = raw.len() / 4;
        raw[mid] ^= 0xff;
        fs::write(&path, &raw).unwrap();

        let err = Frames::open(&path)
            .unwrap()
            .find_map(Result::err)
            .expect("a tampered journal must not read clean");
        // Either the bytes stop decoding, or they decode to something whose id
        // no longer matches what the next frame links. Both are refusals.
        assert!(
            matches!(err, JournalError::ChainBroken(_) | JournalError::Content(_)),
            "unexpected error: {err:?}"
        );
    }

    /// Reordering two snapshots breaks the links even though every frame is
    /// individually well-formed.
    #[test]
    fn reordering_snapshots_is_caught() {
        let dir = tempfile::tempdir().unwrap();
        let mut j = Journal::open(dir.path()).unwrap();
        j.append(&sample("gnuc", 1.0)).unwrap();
        let split = fs::metadata(dir.path().join(LOG_FILE)).unwrap().len() as usize;
        j.append(&sample("gnuc", 2.0)).unwrap();
        drop(j);

        let path = dir.path().join(LOG_FILE);
        let raw = fs::read(&path).unwrap();
        let (first, second) = raw.split_at(split);
        let mut swapped = second.to_vec();
        swapped.extend_from_slice(first);
        fs::write(&path, &swapped).unwrap();

        let err = Frames::open(&path)
            .unwrap()
            .find_map(Result::err)
            .expect("reordered frames must not read clean");
        assert!(matches!(err, JournalError::ChainBroken(_)));
    }

    /// A crash after the frame is written but before HEAD advances must cost
    /// the last sample, not the chain.
    #[test]
    fn unacknowledged_tail_is_discarded_on_open() {
        let dir = tempfile::tempdir().unwrap();
        let mut j = Journal::open(dir.path()).unwrap();
        let first = j.append(&sample("gnuc", 1.0)).unwrap();
        let acked = fs::metadata(dir.path().join(LOG_FILE)).unwrap().len();
        j.append(&sample("gnuc", 2.0)).unwrap();
        drop(j);

        // Rewind HEAD to before the second frame, as a crash would leave it.
        write_head(dir.path(), &first, acked).unwrap();

        let j = Journal::open(dir.path()).unwrap();
        assert_eq!(j.head(), Some(first));
        assert_eq!(j.verify().unwrap(), 1);
        assert_eq!(
            fs::metadata(dir.path().join(LOG_FILE)).unwrap().len(),
            acked
        );
    }

    #[test]
    fn lost_head_is_rebuilt_by_verifying_the_log() {
        let dir = tempfile::tempdir().unwrap();
        let mut j = Journal::open(dir.path()).unwrap();
        j.append(&sample("gnuc", 1.0)).unwrap();
        let last = j.append(&sample("gnuc", 2.0)).unwrap();
        drop(j);

        fs::remove_file(dir.path().join(HEAD_FILE)).unwrap();

        let mut j = Journal::open(dir.path()).unwrap();
        assert_eq!(j.head(), Some(last));
        // And the rebuilt chain still extends correctly.
        j.append(&sample("gnuc", 3.0)).unwrap();
        assert_eq!(j.verify().unwrap(), 3);
    }

    #[test]
    fn a_truncated_final_frame_is_reported_not_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let mut j = Journal::open(dir.path()).unwrap();
        j.append(&sample("gnuc", 1.0)).unwrap();
        drop(j);

        let path = dir.path().join(LOG_FILE);
        let raw = fs::read(&path).unwrap();
        fs::write(&path, &raw[..raw.len() - 3]).unwrap();

        let err = Frames::open(&path)
            .unwrap()
            .find_map(Result::err)
            .expect("a truncated frame must be an error");
        assert!(matches!(err, JournalError::Truncated { .. }));
    }

    #[test]
    fn empty_journal_reads_clean_and_has_no_head() {
        let dir = tempfile::tempdir().unwrap();
        let j = Journal::open(dir.path()).unwrap();
        assert_eq!(j.head(), None);
        assert_eq!(j.verify().unwrap(), 0);
    }

    #[test]
    fn malformed_head_is_refused_rather_than_guessed() {
        let dir = tempfile::tempdir().unwrap();
        Journal::open(dir.path())
            .unwrap()
            .append(&sample("gnuc", 1.0))
            .unwrap();
        fs::write(dir.path().join(HEAD_FILE), "not-a-cid 12").unwrap();
        assert!(matches!(
            Journal::open(dir.path()),
            Err(JournalError::MalformedHead(_))
        ));
    }

    /// A non-finite metric value has no canonical dag-cbor form. The encoder
    /// refuses it, so a bad collector reading can never land a frame the
    /// journal cannot read back — the failure is loud and local to that sample.
    #[test]
    fn non_finite_values_are_refused_at_encode() {
        let dir = tempfile::tempdir().unwrap();
        let mut j = Journal::open(dir.path()).unwrap();
        let mut bad = MetricSet::new("gnuc");
        bad.insert("cpu.percent", f64::NAN);

        assert!(
            matches!(j.append(&bad), Err(JournalError::Content(_))),
            "a NaN sample must be refused, not written"
        );
        // The refusal costs that sample only: the chain is untouched and still
        // extends.
        assert_eq!(j.verify().unwrap(), 0);
        j.append(&sample("gnuc", 1.0)).unwrap();
        assert_eq!(j.verify().unwrap(), 1);
    }
}
