use std::collections::VecDeque;
use std::sync::atomic::{AtomicU16, AtomicU32, AtomicU64, Ordering::Relaxed};
use std::sync::Mutex;

const HISTORY_LEN: usize = 60;
const LOG_CAPACITY: usize = 20;

pub struct PipelineMetrics {
    pub input_rms: AtomicU32,
    pub output_rms: AtomicU32,
    pub input_peak: AtomicU16,
    pub output_peak: AtomicU16,
    pub frames_sent: AtomicU64,
    pub frames_received: AtomicU64,
    pub bytes_sent: AtomicU64,
    pub bytes_received: AtomicU64,
    pub buffer_depth: AtomicU64,
    pub input_history: Mutex<VecDeque<f64>>,
    pub output_history: Mutex<VecDeque<f64>>,
    pub status: Mutex<String>,
    pub log: Mutex<VecDeque<String>>,
}

pub struct Snapshot {
    pub input_rms: f32,
    pub output_rms: f32,
    pub frames_sent: u64,
    pub frames_received: u64,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub buffer_depth: u64,
    pub input_history: Vec<u64>,
    pub output_history: Vec<u64>,
    pub status: String,
    pub log: Vec<String>,
}

impl PipelineMetrics {
    pub fn new() -> Self {
        Self {
            input_rms: AtomicU32::new(0),
            output_rms: AtomicU32::new(0),
            input_peak: AtomicU16::new(0),
            output_peak: AtomicU16::new(0),
            frames_sent: AtomicU64::new(0),
            frames_received: AtomicU64::new(0),
            bytes_sent: AtomicU64::new(0),
            bytes_received: AtomicU64::new(0),
            buffer_depth: AtomicU64::new(0),
            input_history: Mutex::new(VecDeque::with_capacity(HISTORY_LEN)),
            output_history: Mutex::new(VecDeque::with_capacity(HISTORY_LEN)),
            status: Mutex::new("Initializing...".into()),
            log: Mutex::new(VecDeque::with_capacity(LOG_CAPACITY)),
        }
    }

    pub fn snapshot(&self) -> Snapshot {
        let scale = |hist: &Mutex<VecDeque<f64>>| -> Vec<u64> {
            hist.lock()
                .unwrap()
                .iter()
                .map(|&v| (v * 100.0).min(100.0) as u64)
                .collect()
        };

        Snapshot {
            input_rms: f32::from_bits(self.input_rms.load(Relaxed)),
            output_rms: f32::from_bits(self.output_rms.load(Relaxed)),
            frames_sent: self.frames_sent.load(Relaxed),
            frames_received: self.frames_received.load(Relaxed),
            bytes_sent: self.bytes_sent.load(Relaxed),
            bytes_received: self.bytes_received.load(Relaxed),
            buffer_depth: self.buffer_depth.load(Relaxed),
            input_history: scale(&self.input_history),
            output_history: scale(&self.output_history),
            status: self.status.lock().unwrap().clone(),
            log: self.log.lock().unwrap().iter().cloned().collect(),
        }
    }

    // --- Lock-free writes (safe for cpal real-time callbacks) ---

    pub fn set_input_level(&self, rms: f32, peak: u16) {
        self.input_rms.store(rms.to_bits(), Relaxed);
        self.input_peak.fetch_max(peak, Relaxed);
    }

    pub fn set_output_level(&self, rms: f32, peak: u16) {
        self.output_rms.store(rms.to_bits(), Relaxed);
        self.output_peak.fetch_max(peak, Relaxed);
    }

    pub fn set_buffer_depth(&self, depth: usize) {
        self.buffer_depth.store(depth as u64, Relaxed);
    }

    // --- Mutex writes (async tasks only, never from cpal callbacks) ---

    pub fn push_input_history(&self, rms: f64) {
        push_history(&self.input_history, rms);
    }

    pub fn push_output_history(&self, rms: f64) {
        push_history(&self.output_history, rms);
    }

    pub fn add_sent(&self, frames: u64, bytes: u64) {
        self.frames_sent.fetch_add(frames, Relaxed);
        self.bytes_sent.fetch_add(bytes, Relaxed);
    }

    pub fn add_received(&self, frames: u64, bytes: u64) {
        self.frames_received.fetch_add(frames, Relaxed);
        self.bytes_received.fetch_add(bytes, Relaxed);
    }

    pub fn set_status(&self, s: String) {
        *self.status.lock().unwrap() = s;
    }

    pub fn push_log(&self, msg: String) {
        let mut log = self.log.lock().unwrap();
        if log.len() >= LOG_CAPACITY {
            log.pop_front();
        }
        log.push_back(msg);
    }
}

fn push_history(hist: &Mutex<VecDeque<f64>>, val: f64) {
    let mut h = hist.lock().unwrap();
    if h.len() >= HISTORY_LEN {
        h.pop_front();
    }
    h.push_back(val);
}

// --- Pure functions (no state, no allocation beyond result) ---

pub fn compute_rms(samples: &[i16]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let sum: f64 = samples.iter().map(|&s| (s as f64) * (s as f64)).sum();
    ((sum / samples.len() as f64).sqrt() / i16::MAX as f64) as f32
}

pub fn compute_peak(samples: &[i16]) -> u16 {
    samples.iter().map(|s| s.unsigned_abs()).max().unwrap_or(0)
}
