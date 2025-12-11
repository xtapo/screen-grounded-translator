use crate::config::AudioSource;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use std::sync::{Arc, Mutex};
use std::sync::atomic::{AtomicBool, Ordering};

pub struct AudioCapture {
    stream: Option<cpal::Stream>,
    is_running: Arc<AtomicBool>,
}

impl AudioCapture {
    pub fn new() -> Self {
        Self {
            stream: None,
            is_running: Arc::new(AtomicBool::new(false)),
        }
    }

    pub fn start<F>(&mut self, source: AudioSource, on_audio: F) -> Result<(), String>
    where
        F: Fn(Vec<u8>) + Send + Sync + 'static,
    {
        let host = cpal::default_host();
        
        // Loopback logic: Use default OUTPUT device
        let device = match source {
            AudioSource::Microphone => host.default_input_device().ok_or_else(|| "No input device available".to_string())?,
            AudioSource::SystemLoopback => host.default_output_device().ok_or_else(|| "No output device available for loopback".to_string())?,
        };

        log::info!("Audio capture device: {} (Source: {:?})", device.name().unwrap_or_default(), source);

        // Get config
        let config = match source {
            AudioSource::Microphone => device.default_input_config(),
            AudioSource::SystemLoopback => device.default_output_config(),
        }.map_err(|e| format!("Failed to get default config: {}", e))?;

        let sample_rate = config.sample_rate().0;
        let channels = config.channels() as usize;

        log::info!("Input format: {} Hz, {} channels", sample_rate, channels);

        // We target 16000 Hz, Mono, i16 for Gemini
        let target_sample_rate = 16000;
        
        let err_fn = |err| log::error!("an error occurred on stream: {}", err);
        
        let is_running = self.is_running.clone();
        is_running.store(true, Ordering::SeqCst);
        
        let on_audio = Arc::new(on_audio);

        let stream_config: cpal::StreamConfig = config.clone().into();

        let stream = match config.sample_format() {
            cpal::SampleFormat::F32 => device.build_input_stream(
                &stream_config,
                move |data: &[f32], _: &_| {
                    if !is_running.load(Ordering::SeqCst) { return; }
                    let pcm = process_f32_data(data, channels, sample_rate, target_sample_rate);
                    if !pcm.is_empty() {
                        on_audio(pcm);
                    }
                },
                err_fn,
                None, // None=blocking
            ),
            cpal::SampleFormat::I16 => device.build_input_stream(
                &stream_config,
                move |data: &[i16], _: &_| {
                    if !is_running.load(Ordering::SeqCst) { return; }
                     let f32_data: Vec<f32> = data.iter().map(|&x| x as f32 / 32768.0).collect();
                     let pcm = process_f32_data(&f32_data, channels, sample_rate, target_sample_rate);
                     if !pcm.is_empty() {
                         on_audio(pcm);
                     }
                },
                err_fn,
                None,
            ),
            cpal::SampleFormat::U16 => device.build_input_stream(
                &stream_config,
                move |data: &[u16], _: &_| {
                     if !is_running.load(Ordering::SeqCst) { return; }
                     // U16 is 0..65535, center 32768
                     let f32_data: Vec<f32> = data.iter().map(|&x| (x as f32 - 32768.0) / 32768.0).collect();
                     let pcm = process_f32_data(&f32_data, channels, sample_rate, target_sample_rate);
                     if !pcm.is_empty() {
                         on_audio(pcm);
                     }
                },
                err_fn,
                None,
            ),
            _ => return Err("Unsupported sample format".to_string()),
        }.map_err(|e| format!("Failed to build input stream: {}", e))?;

        stream.play().map_err(|e| format!("Failed to play stream: {}", e))?;
        self.stream = Some(stream);

        Ok(())
    }

    pub fn stop(&mut self) {
        self.is_running.store(false, Ordering::SeqCst);
        // Dropping the stream stops it
        self.stream = None;
    }
}

// Simple resampler: Downmix to Mono -> Decimate/Interpolate to 16kHz -> f32 to i16 bytes
fn process_f32_data(data: &[f32], channels: usize, input_rate: u32, target_rate: u32) -> Vec<u8> {
    if data.is_empty() { return Vec::new(); }

    // 1. Downmix to Mono
    let mono_data: Vec<f32> = if channels == 1 {
        data.to_vec()
    } else {
        data.chunks(channels)
            .map(|chunk| chunk.iter().sum::<f32>() / channels as f32)
            .collect()
    };

    // 2. Resample
    // Simple nearest neighbor / dropping samples if input_rate > target_rate
    let ratio = input_rate as f32 / target_rate as f32;
    let output_len = (mono_data.len() as f32 / ratio).ceil() as usize;
    let mut resampled = Vec::with_capacity(output_len);

    for i in 0..output_len {
        let index = (i as f32 * ratio) as usize;
        if index < mono_data.len() {
            resampled.push(mono_data[index]);
        }
    }

    // 3. Convert to i16 bytes (Little Endian)
    let mut bytes = Vec::with_capacity(resampled.len() * 2);
    for sample in resampled {
        let clamped = sample.max(-1.0).min(1.0);
        let val = (clamped * 32767.0) as i16;
        bytes.extend_from_slice(&val.to_le_bytes());
    }

    bytes
}
