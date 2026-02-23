use anyhow::Result;
use std::time::{Duration, Instant};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

const RECONNECT_COOLDOWN: Duration = Duration::from_secs(5);

pub struct RvcClient {
    stream: Option<UnixStream>,
    socket_path: String,
    accumulator: Vec<i16>,
    block_size: usize,
    reconnect_at: Instant,
}

impl RvcClient {
    /// `block_time` must match the Python RvcProcessor's `--block-time`
    /// (both come from `[rvc].block_time` in config.toml).  Mismatched
    /// values cause time-stretching artifacts (robotic / repeated audio).
    pub async fn connect(socket_path: &str, api_rate: u32, block_time: f64) -> Result<Self> {
        let stream = UnixStream::connect(socket_path).await?;
        let block_size = (api_rate as f64 * block_time) as usize;
        Ok(Self {
            stream: Some(stream),
            socket_path: socket_path.to_string(),
            accumulator: Vec::with_capacity(block_size * 2),
            block_size,
            reconnect_at: Instant::now(),
        })
    }

    pub fn is_connected(&self) -> bool {
        self.stream.is_some()
    }

    /// Mark as disconnected. Caller should passthrough audio.
    pub fn disconnect(&mut self) {
        self.stream = None;
        self.accumulator.clear();
        self.reconnect_at = Instant::now() + RECONNECT_COOLDOWN;
    }

    /// Non-blocking reconnect attempt. Only tries if cooldown has elapsed.
    /// Returns true if reconnected.
    pub async fn try_reconnect(&mut self) -> bool {
        if Instant::now() < self.reconnect_at {
            return false;
        }
        match UnixStream::connect(&self.socket_path).await {
            Ok(stream) => {
                self.stream = Some(stream);
                true
            }
            Err(_) => {
                self.reconnect_at = Instant::now() + RECONNECT_COOLDOWN;
                false
            }
        }
    }

    /// Reset the Python processor's internal buffers (sliding window, SOLA,
    /// pitch caches).  Sends a zero-length payload which the server interprets
    /// as a reset signal.  Also clears the local accumulator.
    pub async fn reset(&mut self) -> Result<()> {
        self.accumulator.clear();

        let stream = self
            .stream
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("RVC disconnected"))?;

        // Zero-length payload = reset signal
        stream.write_all(&0u32.to_le_bytes()).await?;
        stream.flush().await?;

        // Read the zero-length ack
        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf).await?;

        Ok(())
    }

    pub async fn process(&mut self, samples: &[i16]) -> Result<Option<Vec<i16>>> {
        self.accumulator.extend_from_slice(samples);

        if self.accumulator.len() < self.block_size {
            return Ok(None);
        }

        let block: Vec<i16> = self.accumulator.drain(..self.block_size).collect();
        self.send_block(&block).await.map(Some)
    }

    pub async fn flush(&mut self) -> Result<Option<Vec<i16>>> {
        if self.accumulator.is_empty() {
            return Ok(None);
        }
        // Pad to full block size so the Python processor receives the
        // expected number of samples.  Undersized blocks cause time-stretching.
        self.accumulator.resize(self.block_size, 0);
        let block: Vec<i16> = self.accumulator.drain(..).collect();
        self.send_block(&block).await.map(Some)
    }

    async fn send_block(&mut self, block: &[i16]) -> Result<Vec<i16>> {
        let stream = self
            .stream
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("RVC disconnected"))?;

        let payload: Vec<u8> = block.iter().flat_map(|s| s.to_le_bytes()).collect();

        stream
            .write_all(&(payload.len() as u32).to_le_bytes())
            .await?;
        stream.write_all(&payload).await?;
        stream.flush().await?;

        let mut len_buf = [0u8; 4];
        stream.read_exact(&mut len_buf).await?;
        let response_len = u32::from_le_bytes(len_buf) as usize;

        let mut response = vec![0u8; response_len];
        stream.read_exact(&mut response).await?;

        Ok(response
            .chunks_exact(2)
            .map(|c| i16::from_le_bytes([c[0], c[1]]))
            .collect())
    }
}
