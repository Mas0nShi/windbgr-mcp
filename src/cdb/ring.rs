//! Bounded ring buffer for cdb stdout/stderr. Offsets are monotonically
//! increasing and MCP clients paginate by `since_offset`.

use std::collections::VecDeque;

#[derive(Debug)]
pub struct RingBuffer {
    capacity: usize,
    data: VecDeque<u8>,
    /// Total number of bytes ever written.
    pub total_written: u64,
}

impl RingBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            capacity,
            data: VecDeque::with_capacity(capacity.min(64 * 1024)),
            total_written: 0,
        }
    }

    pub fn push(&mut self, chunk: &[u8]) {
        self.total_written = self.total_written.saturating_add(chunk.len() as u64);
        if chunk.len() >= self.capacity {
            self.data.clear();
            let start = chunk.len() - self.capacity;
            self.data.extend(chunk[start..].iter().copied());
            return;
        }
        while self.data.len() + chunk.len() > self.capacity {
            let overflow = self.data.len() + chunk.len() - self.capacity;
            for _ in 0..overflow.min(self.data.len()) {
                self.data.pop_front();
            }
        }
        self.data.extend(chunk.iter().copied());
    }

    pub fn total_written(&self) -> u64 {
        self.total_written
    }

    /// Earliest offset currently still buffered.
    pub fn earliest_offset(&self) -> u64 {
        self.total_written.saturating_sub(self.data.len() as u64)
    }

    /// Copy bytes from `since_offset` forward. Returns (bytes, next_offset,
    /// truncated_from_start). If `since_offset` is older than what we still
    /// have, the earliest retained slice is returned and `truncated_from_start`
    /// is set to true.
    pub fn read_since(&self, since_offset: u64) -> (Vec<u8>, u64, bool) {
        let earliest = self.earliest_offset();
        let (start_idx, truncated) = if since_offset < earliest {
            (0usize, true)
        } else {
            ((since_offset - earliest) as usize, false)
        };
        if start_idx >= self.data.len() {
            return (Vec::new(), self.total_written, truncated);
        }
        let mut out = Vec::with_capacity(self.data.len() - start_idx);
        let (a, b) = self.data.as_slices();
        if start_idx < a.len() {
            out.extend_from_slice(&a[start_idx..]);
            out.extend_from_slice(b);
        } else {
            let off = start_idx - a.len();
            out.extend_from_slice(&b[off..]);
        }
        (out, self.total_written, truncated)
    }

    pub fn snapshot_tail(&self, max_bytes: usize) -> String {
        if self.data.is_empty() {
            return String::new();
        }
        let take = max_bytes.min(self.data.len());
        let start = self.data.len() - take;
        let mut bytes = Vec::with_capacity(take);
        let (a, b) = self.data.as_slices();
        if start < a.len() {
            bytes.extend_from_slice(&a[start..]);
            bytes.extend_from_slice(b);
        } else {
            bytes.extend_from_slice(&b[start - a.len()..]);
        }
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wraparound_and_read_since() {
        let mut ring = RingBuffer::new(8);
        ring.push(b"01234");
        ring.push(b"56789");
        assert_eq!(ring.total_written(), 10);
        assert_eq!(ring.earliest_offset(), 2);
        let (bytes, next, truncated) = ring.read_since(0);
        assert_eq!(bytes, b"23456789");
        assert_eq!(next, 10);
        assert!(truncated);

        let (bytes, next, truncated) = ring.read_since(6);
        assert_eq!(bytes, b"6789");
        assert_eq!(next, 10);
        assert!(!truncated);
    }

    #[test]
    fn tail_snapshot() {
        let mut ring = RingBuffer::new(16);
        ring.push(b"hello world");
        assert_eq!(ring.snapshot_tail(5), "world");
        assert_eq!(ring.snapshot_tail(100), "hello world");
    }
}
