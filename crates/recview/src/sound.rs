// Plays the game's mixer through the default output device
use audio::{ReplayAudio, SAMPLE_RATE, Sounds};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{FromSample, SizedSample};
use replay::{Session, Step};
use std::collections::VecDeque;
use std::sync::{Arc, Mutex};

// How far the mixer runs ahead of the speaker, about 0.1 seconds
const QUEUE_SAMPLES: usize = 1200;

type Queue = Arc<Mutex<VecDeque<i16>>>;

pub struct Speaker {
    _stream: cpal::Stream,
    queue: Queue,
    audio: ReplayAudio,
    buffer: Vec<i16>,
    pub muted: bool,
}

// Linear resampling from the mixer's rate to the device's, into every channel
fn callback<T: SizedSample + FromSample<f32>>(
    queue: Queue,
    device_rate: u32,
    channels: usize,
) -> impl FnMut(&mut [T], &cpal::OutputCallbackInfo) + Send + 'static {
    let step = SAMPLE_RATE as f64 / device_rate as f64;
    let mut position = 0.0;
    let mut current = 0i16;
    let mut next = 0i16;
    move |data: &mut [T], _| {
        let mut queue = queue.lock().unwrap();
        for frame in data.chunks_mut(channels) {
            let value = current as f32 + (next as f32 - current as f32) * position as f32;
            let sample = T::from_sample(value / 32768.0);
            frame.fill(sample);
            position += step;
            while position >= 1.0 {
                position -= 1.0;
                current = next;
                next = queue.pop_front().unwrap_or(0);
            }
        }
    }
}

impl Speaker {
    pub fn open(sounds: Arc<Sounds>, riders: usize) -> Result<Speaker, String> {
        let device = cpal::default_host()
            .default_output_device()
            .ok_or("no audio output device")?;
        let supported = device.default_output_config().map_err(|e| e.to_string())?;
        let config = supported.config();
        let queue: Queue = Arc::new(Mutex::new(VecDeque::new()));
        let rate = config.sample_rate;
        let channels = config.channels as usize;
        let on_error = |e| eprintln!("recview: audio: {e}");
        let stream = match supported.sample_format() {
            cpal::SampleFormat::F32 => device.build_output_stream(
                config,
                callback::<f32>(queue.clone(), rate, channels),
                on_error,
                None,
            ),
            cpal::SampleFormat::I16 => device.build_output_stream(
                config,
                callback::<i16>(queue.clone(), rate, channels),
                on_error,
                None,
            ),
            cpal::SampleFormat::U16 => device.build_output_stream(
                config,
                callback::<u16>(queue.clone(), rate, channels),
                on_error,
                None,
            ),
            other => return Err(format!("unsupported audio sample format: {other}")),
        }
        .map_err(|e| e.to_string())?;
        stream.play().map_err(|e| e.to_string())?;

        Ok(Speaker {
            _stream: stream,
            queue,
            audio: ReplayAudio::new(sounds, riders),
            buffer: Vec::new(),
            muted: false,
        })
    }

    pub fn seeked(&mut self, session: &Session) {
        self.audio.seeked(session);
    }

    // Called once a frame, with the step the replay moved forward by, if any
    pub fn update(&mut self, session: &Session, step: Option<&Step>, playing: bool) {
        if let Some(step) = step {
            self.audio.play_sounds(step);
        }
        self.audio.follow(session, step);

        let mut queue = self.queue.lock().unwrap();
        let missing = QUEUE_SAMPLES.saturating_sub(queue.len());
        if missing == 0 {
            return;
        }
        self.buffer.resize(missing, 0);
        if playing && !self.muted {
            self.audio.mixer.mix(&mut self.buffer);
        } else {
            self.buffer.fill(0);
        }
        queue.extend(&self.buffer);
    }
}
