use elma_formats::rec::WavEvent;
use elma_formats::res::Res;
use elma_formats::wav::decode_wav;
use replay::{Session, Step};
use std::sync::Arc;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum Error {
    #[error("{0} is missing")]
    Missing(String),
    #[error("{0} is too short")]
    TooShort(String),
    #[error("{name}: {source}")]
    Wav {
        name: String,
        source: elma_formats::Error,
    },
}

// Mono, signed 16-bit
pub const SAMPLE_RATE: u32 = 11025;

const WAV_FADE_LENGTH: usize = 100;
const MAX_WAV_EVENTS: usize = 5;

// The game mixes into 16-bit shorts with wrap-around and truncating casts
fn to_short(value: f64) -> i16 {
    value as i32 as i16
}

#[derive(Debug, Clone)]
struct Wav {
    samples: Vec<i16>,
}

impl Wav {
    fn load(
        get: Files,
        name: &str,
        max_volume: f64,
        start: usize,
        end: Option<usize>,
    ) -> Result<Wav, Error> {
        let data = get(name).ok_or_else(|| Error::Missing(name.into()))?;
        let all = decode_wav(&data).map_err(|source| Error::Wav {
            name: name.into(),
            source,
        })?;
        let end = end.unwrap_or(all.len());
        if end > all.len() || start > end {
            return Err(Error::TooShort(name.into()));
        }
        let mut samples = all[start..end].to_vec();
        let max_amplitude = samples
            .iter()
            .map(|&s| (s as i32).abs())
            .max()
            .unwrap_or(1)
            .max(1);
        let scale = 32000.0 * max_volume / max_amplitude as f64;
        for s in &mut samples {
            *s = to_short(*s as f64 * scale);
        }
        Ok(Wav { samples })
    }

    fn len(&self) -> usize {
        self.samples.len()
    }

    // Cut the last `fade` samples and fade them into the start
    fn make_loop(&mut self, fade: usize) {
        let n = self.len();
        for i in 0..fade {
            let f = i as f64 / fade as f64;
            self.samples[i] = to_short(
                f * self.samples[i] as f64 + (1.0 - f) * self.samples[n - fade + i] as f64,
            );
        }
        self.samples.truncate(n - fade);
    }

    fn fade_into(&mut self, next: &Wav, fade: usize) {
        let n = self.len();
        for i in 0..fade {
            let f = i as f64 / fade as f64;
            self.samples[n - fade + i] =
                to_short((1.0 - f) * self.samples[i] as f64 + f * next.samples[i] as f64);
        }
    }

    fn scale(&mut self, factor: f64) {
        for s in &mut self.samples {
            *s = to_short(*s as f64 * factor);
        }
    }
}

// Looping audio played back at a variable speed, for the throttle
#[derive(Debug, Clone)]
struct Wav2 {
    samples: Vec<i16>,
    deltas: Vec<i16>,
}

impl Wav2 {
    fn new(source: &Wav) -> Wav2 {
        let samples = source.samples.clone();
        let n = samples.len();
        let deltas = (0..n)
            .map(|i| samples[(i + 1) % n].wrapping_sub(samples[i]))
            .collect();
        Wav2 { samples, deltas }
    }

    fn next_sample(&self, index: &mut f64, dt: f64) -> i16 {
        *index += dt;
        if *index >= self.samples.len() as f64 {
            *index -= self.samples.len() as f64;
        }
        let whole = (*index as usize).min(self.samples.len() - 1);
        let fraction = *index - whole as f64;
        self.samples[whole].wrapping_add(to_short(self.deltas[whole] as f64 * fraction))
    }
}

// Finds a game file by name
type Files<'a> = &'a dyn Fn(&str) -> Option<Vec<u8>>;

// Every sound the game mixes, loaded from elma.res
#[derive(Debug, Clone)]
pub struct Sounds {
    bump: Wav,
    dead: Wav,
    win: Wav,
    food: Wav,
    turn: Wav,
    volt: Wav,
    friction: Wav,
    ignition: Wav,
    idle: Wav,
    gas_start: Wav,
    gas: Wav2,
}

impl Sounds {
    pub fn load(res: &Res) -> Result<Sounds, Error> {
        Self::load_files(&|name| res.get(name).map(<[u8]>::to_vec), false)
    }

    // The sounds from the game's files: Across's own, or Elma's from elma.res
    pub fn load_files(get: Files, across: bool) -> Result<Sounds, Error> {
        let res = get;
        let mut friction = Wav::load(res, "dorzsol.wav", 0.44, 0, None)?;
        // The loop keeps what is left after cutting the fade off its end
        if friction.len() <= WAV_FADE_LENGTH {
            return Err(Error::TooShort("dorzsol.wav".into()));
        }
        friction.make_loop(WAV_FADE_LENGTH);

        const VOLUME: f64 = 0.26;
        const HARL_MAX_INDEX_1: usize = 7526;
        const HARL_MAX_INDEX_2: usize = 34648;
        const HARL_MAX_INDEX_3: usize = 38766;
        const HARL2_MIN_INDEX: usize = 14490;
        const HARL2_MAX_INDEX: usize = 18906;
        let ignition = Wav::load(
            res,
            "harl.wav",
            VOLUME,
            0,
            Some(HARL_MAX_INDEX_1 + WAV_FADE_LENGTH),
        )?;
        let mut idle = Wav::load(
            res,
            "harl.wav",
            VOLUME,
            HARL_MAX_INDEX_1,
            Some(HARL_MAX_INDEX_2),
        )?;
        let mut gas_start = Wav::load(
            res,
            "harl.wav",
            VOLUME,
            HARL_MAX_INDEX_2 - WAV_FADE_LENGTH,
            Some(HARL_MAX_INDEX_3),
        )?;
        let mut gas = Wav::load(
            res,
            "harl2.wav",
            VOLUME,
            HARL2_MIN_INDEX,
            Some(HARL2_MAX_INDEX),
        )?;
        idle.make_loop(WAV_FADE_LENGTH);
        gas.make_loop(WAV_FADE_LENGTH);
        gas_start.fade_into(&gas, WAV_FADE_LENGTH);
        // A change from Across: a quieter idle motor
        if !across {
            idle.scale(0.4);
        }

        Ok(Sounds {
            bump: Wav::load(res, "utodes.wav", 0.25, 0, None)?,
            dead: Wav::load(res, "torik.wav", 0.34, 0, None)?,
            win: Wav::load(res, "siker.wav", 0.8, 0, None)?,
            food: Wav::load(res, "eves.wav", 0.5, 0, None)?,
            turn: Wav::load(res, "fordul.wav", 0.3, 0, None)?,
            volt: Wav::load(res, "ugras.wav", 0.34, 0, None)?,
            friction,
            ignition,
            idle,
            gas_start,
            gas: Wav2::new(&gas),
        })
    }

    fn event(&self, event: WavEvent) -> Option<&Wav> {
        match event {
            WavEvent::Bump => Some(&self.bump),
            WavEvent::Dead => Some(&self.dead),
            WavEvent::Win => Some(&self.win),
            WavEvent::Food => Some(&self.food),
            WavEvent::Turn => Some(&self.turn),
            WavEvent::RightVolt | WavEvent::LeftVolt => Some(&self.volt),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum MotorState {
    Ignition,
    Idle,
    IdleToGasTransition,
    GasStart,
    Gassing,
}

#[derive(Debug, Clone, Copy)]
struct Motor {
    enabled: bool,
    frequency_prev: f64,
    frequency_next: f64,
    gas: bool,
    state: MotorState,
    index_ignition: usize,
    index_idle: usize,
    index_gas_start: usize,
    gas_index: f64,
}

impl Default for Motor {
    fn default() -> Self {
        Motor {
            enabled: false,
            frequency_prev: 1.0,
            frequency_next: 1.0,
            gas: false,
            state: MotorState::Ignition,
            index_ignition: 0,
            index_idle: 0,
            index_gas_start: 0,
            gas_index: 0.0,
        }
    }
}

#[derive(Debug, Clone, Copy)]
struct Playing {
    event: WavEvent,
    index: usize,
    volume: f64,
}

// Port of the game's mixer: two motors, the friction squeak and up to five sound effects
pub struct Mixer {
    sounds: Arc<Sounds>,
    motors: [Motor; 2],
    friction_prev: f64,
    friction_next: f64,
    friction_index: usize,
    events: [Option<Playing>; MAX_WAV_EVENTS],
}

fn mix(buffer: &mut [i16], source: &[i16]) {
    for (b, &s) in buffer.iter_mut().zip(source) {
        *b = b.wrapping_add(s);
    }
}

fn mix_volume(buffer: &mut [i16], source: &[i16], volume: f64) {
    for (b, &s) in buffer.iter_mut().zip(source) {
        *b = b.wrapping_add(to_short(s as f64 * volume));
    }
}

// Mixes `wav` from `index` into the rest of the buffer after `copied`. Returns true when the
// wav ended inside it, with `copied` moved past its end; otherwise the buffer is full and
// `index` moved on.
fn play(buffer: &mut [i16], copied: &mut usize, wav: &Wav, index: &mut usize) -> bool {
    let length = buffer.len().saturating_sub(*copied);
    let remaining = wav.len() - *index;
    if length >= remaining {
        mix(
            &mut buffer[*copied..*copied + remaining],
            &wav.samples[*index..],
        );
        *copied += remaining;
        true
    } else {
        mix(
            &mut buffer[*copied..],
            &wav.samples[*index..*index + length],
        );
        *index += length;
        false
    }
}

impl Mixer {
    pub fn new(sounds: Arc<Sounds>) -> Mixer {
        Mixer {
            sounds,
            motors: [Motor::default(); 2],
            friction_prev: 0.0,
            friction_next: 0.0,
            friction_index: 0,
            events: [None; MAX_WAV_EVENTS],
        }
    }

    pub fn start_motor(&mut self, motor: usize) {
        self.reset_motor(motor, true);
    }

    pub fn stop_motor(&mut self, motor: usize) {
        self.reset_motor(motor, false);
    }

    // Back to the ignition, keeping the other indices and the gas
    fn reset_motor(&mut self, motor: usize, enabled: bool) {
        let m = &mut self.motors[motor];
        m.enabled = enabled;
        m.state = MotorState::Ignition;
        m.index_ignition = 0;
        m.frequency_prev = 1.0;
        m.frequency_next = 1.0;
    }

    pub fn is_motor_running(&self, motor: usize) -> bool {
        self.motors[motor].enabled
    }

    // Playback speed of the throttle sound, 1.0 to 2.0
    pub fn set_motor_frequency(&mut self, motor: usize, frequency: f64, gas: bool) {
        let m = &mut self.motors[motor];
        m.gas = gas;
        let frequency = frequency.min(2.0);
        m.frequency_next = if frequency < 1.0 { 0.0 } else { frequency };
    }

    pub fn set_friction_volume(&mut self, volume: f64) {
        self.friction_next = volume.clamp(0.0, 1.0);
    }

    pub fn start_wav(&mut self, event: WavEvent, volume: f64) {
        if !(volume > 0.0 && volume < 1.0) || self.sounds.event(event).is_none() {
            return;
        }
        if let Some(slot) = self.events.iter_mut().find(|e| e.is_none()) {
            *slot = Some(Playing {
                event,
                index: 0,
                volume,
            });
        }
    }

    pub fn mix(&mut self, buffer: &mut [i16]) {
        buffer.fill(0);
        for motor in 0..2 {
            self.mix_motor(motor, buffer);
        }
        self.mix_friction(buffer);
        for slot in &mut self.events {
            let Some(mut playing) = *slot else {
                continue;
            };
            let wav = self.sounds.event(playing.event).unwrap();
            let length = buffer.len().min(wav.len() - playing.index);
            mix_volume(
                &mut buffer[..length],
                &wav.samples[playing.index..],
                playing.volume,
            );
            playing.index += length;
            *slot = (length == buffer.len() && playing.index < wav.len()).then_some(playing);
        }
    }

    fn mix_motor(&mut self, motor: usize, buffer: &mut [i16]) {
        let s = &self.sounds;
        let m = &mut self.motors[motor];
        if !m.enabled {
            return;
        }
        let n = buffer.len();
        let mut copied = 0usize;
        loop {
            match m.state {
                MotorState::Ignition => {
                    // The motor only starts over at the beginning of a buffer
                    debug_assert_eq!(copied, 0);
                    if !play(buffer, &mut copied, &s.ignition, &mut m.index_ignition) {
                        return;
                    }
                    m.state = MotorState::Idle;
                    m.index_idle = WAV_FADE_LENGTH;
                }
                MotorState::Idle => {
                    if m.gas {
                        m.state = MotorState::IdleToGasTransition;
                        m.index_gas_start = 0;
                    } else if play(buffer, &mut copied, &s.idle, &mut m.index_idle) {
                        m.index_idle = 0;
                    } else {
                        return;
                    }
                }
                MotorState::IdleToGasTransition => {
                    let length = n.saturating_sub(copied);
                    let mut end = m.index_gas_start + length;
                    if end > WAV_FADE_LENGTH {
                        end = WAV_FADE_LENGTH;
                        m.state = MotorState::GasStart;
                    }
                    let mut faded = 0;
                    for i in m.index_gas_start..end {
                        if m.index_idle >= s.idle.len() {
                            m.index_idle = 0;
                        }
                        let f = i as f64 / WAV_FADE_LENGTH as f64;
                        let value = f * s.gas_start.samples[i] as f64
                            + (1.0 - f) * s.idle.samples[m.index_idle] as f64;
                        let b = &mut buffer[copied + faded];
                        *b = b.wrapping_add(to_short(value));
                        m.index_idle += 1;
                        faded += 1;
                    }
                    copied += faded;
                    m.index_gas_start += faded;
                    // The game checks this with the fade counted twice
                    if copied + faded == n {
                        return;
                    }
                    if copied >= n {
                        return;
                    }
                }
                MotorState::GasStart => {
                    if !play(buffer, &mut copied, &s.gas_start, &mut m.index_gas_start) {
                        return;
                    }
                    m.state = MotorState::Gassing;
                    m.gas_index = WAV_FADE_LENGTH as f64;
                    m.frequency_prev = 1.0;
                }
                MotorState::Gassing => {
                    let length = n.saturating_sub(copied);
                    if !m.gas && length > WAV_FADE_LENGTH {
                        m.state = MotorState::Idle;
                        m.index_idle = WAV_FADE_LENGTH;
                        let dt = m.frequency_prev;
                        for i in 0..WAV_FADE_LENGTH {
                            let gas = s.gas.next_sample(&mut m.gas_index, dt);
                            let idle = s.idle.samples[i];
                            let f = i as f64 / WAV_FADE_LENGTH as f64;
                            let b = &mut buffer[copied + i];
                            *b = b.wrapping_add(to_short(f * idle as f64 + (1.0 - f) * gas as f64));
                        }
                        copied += WAV_FADE_LENGTH;
                        continue;
                    }
                    let mut dt = m.frequency_prev;
                    let ddt = if length > 30 {
                        (m.frequency_next - dt) / length as f64
                    } else {
                        0.0
                    };
                    for i in 0..length {
                        let sample = s.gas.next_sample(&mut m.gas_index, dt);
                        let b = &mut buffer[copied + i];
                        *b = b.wrapping_add(sample);
                        dt += ddt;
                    }
                    m.frequency_prev = dt;
                    return;
                }
            }
            if copied >= n {
                return;
            }
        }
    }

    fn mix_friction(&mut self, buffer: &mut [i16]) {
        if self.friction_next < 0.1 && self.friction_prev < 0.1 {
            self.friction_prev = 0.0;
            return;
        }
        let samples = &self.sounds.friction.samples;
        let mut volume = self.friction_prev;
        let delta = (self.friction_next - volume) / buffer.len() as f64;
        for b in buffer.iter_mut() {
            let sample = samples[self.friction_index];
            self.friction_index = (self.friction_index + 1) % samples.len();
            *b = b.wrapping_add(to_short(sample as f64 * volume));
            volume += delta;
        }
        self.friction_prev = volume;
    }
}

// Plays a replay through the mixer: its sound events, the motors of the first two bikes
// starting and stopping, their pitch and the friction
pub struct ReplayAudio {
    pub mixer: Mixer,
    dead: [bool; 2],
}

impl ReplayAudio {
    pub fn new(sounds: Arc<Sounds>, riders: usize) -> ReplayAudio {
        let mut mixer = Mixer::new(sounds);
        for motor in 0..riders.min(2) {
            mixer.start_motor(motor);
        }
        ReplayAudio {
            mixer,
            dead: [false; 2],
        }
    }

    pub fn play_sounds(&mut self, step: &Step) {
        for sound in &step.sounds {
            self.mixer.start_wav(sound.event, sound.volume as f64);
        }
    }

    // Follows the bikes, after a step if they moved by one
    pub fn follow(&mut self, session: &Session, step: Option<&Step>) {
        for (i, &alive) in step.iter().flat_map(|s| &s.alive).enumerate().take(2) {
            self.set_alive(i, alive);
        }
        let mut friction = 0.0;
        for (i, rider) in session.riders.iter().enumerate().take(2) {
            self.mixer
                .set_motor_frequency(i, rider.pose.motor_frequency, rider.pose.gas);
            friction += rider.pose.friction_volume;
        }
        self.mixer.set_friction_volume(friction);
    }

    // After a jump, the bikes that are still riding get their motor back
    pub fn seeked(&mut self, session: &Session) {
        for (i, rider) in session.riders.iter().enumerate().take(2) {
            let dead = session.time >= rider.duration();
            if self.dead[i] && !dead {
                self.mixer.start_motor(i);
            }
            self.dead[i] = dead;
        }
    }

    // Silences the motors and friction, as when the replay is over
    pub fn stop(&mut self) {
        self.mixer.stop_motor(0);
        self.mixer.stop_motor(1);
        self.mixer.set_friction_volume(0.0);
    }

    fn set_alive(&mut self, i: usize, alive: bool) {
        if self.dead[i] == alive {
            self.dead[i] = !alive;
            if alive {
                self.mixer.start_motor(i);
            } else {
                self.mixer.stop_motor(i);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sounds() -> Sounds {
        let wav = |n: usize| Wav {
            samples: (0..n).map(|i| ((i % 50) as i16 - 25) * 100).collect(),
        };
        Sounds {
            bump: wav(300),
            dead: wav(300),
            win: wav(300),
            food: wav(300),
            turn: wav(300),
            volt: wav(300),
            friction: wav(500),
            ignition: wav(1000),
            idle: wav(2000),
            gas_start: wav(1500),
            gas: Wav2::new(&wav(1200)),
        }
    }

    #[test]
    fn mixes_through_every_motor_state() {
        let mut mixer = Mixer::new(Arc::new(sounds()));
        mixer.start_motor(0);
        let mut buffer = vec![0i16; 367];
        for frame in 0..200 {
            mixer.set_motor_frequency(0, 1.0 + (frame % 10) as f64 / 10.0, (frame / 20) % 2 == 1);
            mixer.set_friction_volume(if frame % 7 == 0 { 0.5 } else { 0.0 });
            if frame % 13 == 0 {
                mixer.start_wav(WavEvent::Food, 0.99);
            }
            mixer.mix(&mut buffer);
        }
        assert!(buffer.iter().any(|&s| s != 0));
    }

    // A mono 16-bit wav of `n` samples
    fn wav_file(n: usize) -> Vec<u8> {
        let mut fmt = Vec::new();
        for field in [1i16, 1] {
            fmt.extend(field.to_le_bytes());
        }
        fmt.extend((SAMPLE_RATE as i32).to_le_bytes());
        fmt.extend((SAMPLE_RATE as i32 * 2).to_le_bytes());
        fmt.extend(2i16.to_le_bytes());
        fmt.extend(16i16.to_le_bytes());
        let mut out = b"RIFF\0\0\0\0WAVEfmt ".to_vec();
        out.extend(16u32.to_le_bytes());
        out.extend(fmt);
        out.extend(b"data");
        out.extend((n as u32 * 2).to_le_bytes());
        out.extend((0..n).flat_map(|i| (((i % 50) as i16 - 25) * 100).to_le_bytes()));
        out
    }

    fn load_with(friction: usize, effects: usize) -> Result<Sounds, Error> {
        Sounds::load_files(
            &|name| {
                Some(wav_file(match name {
                    "harl.wav" | "harl2.wav" => 40000,
                    "dorzsol.wav" => friction,
                    _ => effects,
                }))
            },
            false,
        )
    }

    #[test]
    fn rejects_a_friction_sound_shorter_than_its_fade() {
        assert!(load_with(WAV_FADE_LENGTH, 300).is_err());
        assert!(load_with(0, 300).is_err());
    }

    #[test]
    fn plays_empty_sound_effects() {
        let mut mixer = Mixer::new(Arc::new(load_with(WAV_FADE_LENGTH + 1, 0).unwrap()));
        mixer.start_wav(WavEvent::Food, 0.99);
        mixer.set_friction_volume(0.5);
        mixer.mix(&mut [0i16; 367]);
    }

    // A two player replay through ReplayAudio the way rec2vid plays it, 30 frames a second:
    // sound events, the motors' pitch and gas, friction, and the second bike's motor stopping
    // when its recording ends. Every sample is hashed, so any change to how a replay sounds
    // shows. When a change is meant, update GOLDEN to what the test prints.
    #[test]
    fn a_replay_sounds_the_same() {
        use elma_formats::lev::{Gravity, Level, Object, ObjectType, TopTens, Vec2};
        use elma_formats::rec::{Event, FLAG_GAS, FRAME_RATE, Frame, Rec, Ride};
        const GOLDEN: u64 = 0xecf1261e80be30fa;

        let level = Level {
            level_id: 1,
            name: String::new(),
            lgr: String::new(),
            foreground: String::new(),
            background: String::new(),
            polygons: vec![],
            objects: vec![Object {
                r: Vec2 { x: 0.0, y: 0.0 },
                kind: ObjectType::Start,
                gravity: Gravity::None,
                animation: 0,
                motion: None,
            }],
            sprites: vec![],
            topology_errors: false,
            top_tens: TopTens::default(),
        };
        let frames = |n: usize, phase: usize| -> Vec<Frame> {
            (0..n)
                .map(|i| Frame {
                    motor_frequency: ((i * 7 + phase) % 200) as u8 + 20,
                    friction_volume: if (i + phase) % 11 < 3 { 120 } else { 0 },
                    flags: if (i / 15 + phase).is_multiple_of(2) {
                        FLAG_GAS
                    } else {
                        0
                    },
                    ..Frame::default()
                })
                .collect()
        };
        let sound = |frame: usize, event_id, volume| Event {
            time: frame as f64 * replay::FRAME_INDEX_TO_TIME,
            object_id: -1,
            event_id,
            volume,
        };
        let rec = Rec {
            multiplayer: true,
            flagtag: false,
            level_id: 1,
            level_filename: "test.lev".into(),
            rides: vec![
                Ride {
                    frames: frames(90, 0),
                    events: vec![
                        sound(10, WavEvent::Bump, 0.8),
                        sound(25, WavEvent::Turn, 0.99),
                        sound(40, WavEvent::RightVolt, 0.99),
                        sound(70, WavEvent::Food, 0.99),
                    ],
                    frame_rate: FRAME_RATE,
                },
                Ride {
                    frames: frames(45, 3),
                    events: vec![sound(30, WavEvent::Dead, 0.99)],
                    frame_rate: FRAME_RATE,
                },
            ],
        };
        let mut session = Session::new(&level, &rec, &[], replay::Settings::default());
        let mut audio = ReplayAudio::new(Arc::new(sounds()), session.riders.len());
        let fps = 30.0;
        let mut samples = Vec::new();
        let mut written = 0usize;
        for frame in 0.. {
            let time = frame as f64 * (replay::MILLISECONDS_TO_PHYS_TIME * 1000.0) / fps;
            let step = session.step(time);
            audio.play_sounds(&step);
            if step.done {
                break;
            }
            audio.follow(&session, Some(&step));
            let target = ((frame + 1) as f64 * SAMPLE_RATE as f64 / fps).round() as usize;
            let mut buffer = vec![0i16; target - written];
            audio.mixer.mix(&mut buffer);
            written = target;
            samples.extend(buffer);
        }
        assert!(samples.len() > SAMPLE_RATE as usize);
        assert!(samples.iter().any(|&s| s != 0));
        // FNV-1a over the samples
        let hash = samples
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .fold(0xcbf29ce484222325u64, |h, b| {
                (h ^ b as u64).wrapping_mul(0x100000001b3)
            });
        assert_eq!(
            hash, GOLDEN,
            "the replay sounds different: its hash is {hash:#x}"
        );
    }

    #[test]
    fn silent_without_motors() {
        let mut mixer = Mixer::new(Arc::new(sounds()));
        let mut buffer = vec![1i16; 100];
        mixer.mix(&mut buffer);
        assert!(buffer.iter().all(|&s| s == 0));
    }
}
