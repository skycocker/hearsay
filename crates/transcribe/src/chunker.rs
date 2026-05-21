//! Buffer audio frames into fixed-size chunks the transcriber can consume.
//!
//! For v1 we use plain fixed-size windows with no overlap. Whisper still
//! produces solid segments within a chunk, and boundary artifacts on speech
//! that spans two chunks are tolerable — we'll add overlap + segment
//! deduplication when the trade-off bites.

/// One contiguous audio chunk handed to the transcriber.
#[derive(Debug, Clone)]
pub struct TranscribeRequest {
    /// 16 kHz mono `f32` PCM samples.
    pub pcm: Vec<f32>,
    /// Where this chunk starts in milliseconds, relative to the session's
    /// start. Used to translate whisper's chunk-local timestamps back into
    /// session-relative ones.
    pub offset_ms: u32,
}

/// Stateful chunker. `push` may return zero or more chunks per call;
/// `flush` returns whatever is left at the end of a session.
pub struct Chunker {
    sample_rate: u32,
    chunk_samples: usize,
    buffer: Vec<f32>,
    /// Number of samples already emitted in previous chunks. Updated on
    /// every successful emit so the next chunk's `offset_ms` is correct.
    samples_emitted: u64,
}

impl Chunker {
    pub fn new(sample_rate: u32, chunk_seconds: u32) -> Self {
        let chunk_samples = (sample_rate as usize) * (chunk_seconds as usize);
        Self {
            sample_rate,
            chunk_samples,
            buffer: Vec::with_capacity(chunk_samples * 2),
            samples_emitted: 0,
        }
    }

    pub fn push(&mut self, frame: &[f32]) -> Vec<TranscribeRequest> {
        self.buffer.extend_from_slice(frame);
        let mut out = Vec::new();
        while self.buffer.len() >= self.chunk_samples {
            let pcm: Vec<f32> = self.buffer.drain(..self.chunk_samples).collect();
            let offset_ms = self.offset_ms_for_emission();
            self.samples_emitted += pcm.len() as u64;
            out.push(TranscribeRequest { pcm, offset_ms });
        }
        out
    }

    pub fn flush(&mut self) -> Option<TranscribeRequest> {
        if self.buffer.is_empty() {
            return None;
        }
        let pcm = std::mem::take(&mut self.buffer);
        let offset_ms = self.offset_ms_for_emission();
        self.samples_emitted += pcm.len() as u64;
        Some(TranscribeRequest { pcm, offset_ms })
    }

    fn offset_ms_for_emission(&self) -> u32 {
        let raw = self.samples_emitted * 1_000 / u64::from(self.sample_rate);
        // u32::MAX ms ≈ 49 days; if a session ever gets longer than that
        // the truncation is the least of anyone's worries.
        u32::try_from(raw).unwrap_or(u32::MAX)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const RATE: u32 = 16_000;
    const CHUNK_SECS: u32 = 20;

    fn frame(n: usize) -> Vec<f32> {
        vec![0.0; n]
    }

    #[test]
    fn emits_nothing_below_chunk_size() {
        let mut c = Chunker::new(RATE, CHUNK_SECS);
        // Half a chunk.
        let out = c.push(&frame((RATE * CHUNK_SECS / 2) as usize));
        assert!(out.is_empty());
    }

    #[test]
    fn emits_one_chunk_at_boundary() {
        let mut c = Chunker::new(RATE, CHUNK_SECS);
        let out = c.push(&frame((RATE * CHUNK_SECS) as usize));
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].pcm.len(), (RATE * CHUNK_SECS) as usize);
        assert_eq!(out[0].offset_ms, 0);
    }

    #[test]
    fn emits_consecutive_chunks_with_correct_offsets() {
        let mut c = Chunker::new(RATE, CHUNK_SECS);
        // 3 chunks' worth.
        let out = c.push(&frame((RATE * CHUNK_SECS * 3) as usize));
        assert_eq!(out.len(), 3);
        assert_eq!(out[0].offset_ms, 0);
        assert_eq!(out[1].offset_ms, 20_000);
        assert_eq!(out[2].offset_ms, 40_000);
    }

    #[test]
    fn flush_returns_partial_tail() {
        let mut c = Chunker::new(RATE, CHUNK_SECS);
        // Emit one full chunk, then leave half a chunk in the buffer.
        let _ = c.push(&frame((RATE * CHUNK_SECS) as usize));
        let _ = c.push(&frame(8_000));
        let tail = c.flush().unwrap();
        assert_eq!(tail.pcm.len(), 8_000);
        assert_eq!(tail.offset_ms, 20_000);
    }

    #[test]
    fn flush_on_empty_returns_none() {
        let mut c = Chunker::new(RATE, CHUNK_SECS);
        assert!(c.flush().is_none());
    }

    #[test]
    fn many_small_pushes_emit_correctly() {
        // Mimics what cpal actually does — lots of tiny frames trickling in.
        let mut c = Chunker::new(RATE, CHUNK_SECS);
        let total_samples_needed = (RATE * CHUNK_SECS * 2) as usize;
        let mut emissions = 0;
        for _ in 0..(total_samples_needed / 320) {
            emissions += c.push(&frame(320)).len();
        }
        assert_eq!(emissions, 2);
    }
}
