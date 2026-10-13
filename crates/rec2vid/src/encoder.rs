use std::fs::File;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::process::{Child, ChildStdin, Command, Stdio};

// An ffmpeg next to rec2vid, as it can come in the Windows zip, or the one on the PATH
fn ffmpeg_program() -> std::path::PathBuf {
    let name = format!("ffmpeg{}", std::env::consts::EXE_SUFFIX);
    std::env::current_exe()
        .ok()
        .and_then(|exe| Some(exe.parent()?.join(&name)))
        .filter(|path| path.is_file())
        .unwrap_or_else(|| name.into())
}

fn spawn_ffmpeg(args: &[String], stdin: Stdio) -> Result<Child, String> {
    Command::new(ffmpeg_program())
        .args(args)
        .stdin(stdin)
        .spawn()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => {
                "failed to start ffmpeg. Is it installed and on your PATH, or next to rec2vid?"
                    .to_string()
            }
            _ => format!("failed to start ffmpeg: {e}"),
        })
}

// Whether ffmpeg can be run, checked before any work so a missing one is found at once
pub fn check_ffmpeg() -> Result<(), String> {
    let status = Command::new(ffmpeg_program())
        .arg("-version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|e| match e.kind() {
            std::io::ErrorKind::NotFound => {
                "cannot find ffmpeg. Install it, and put it on your PATH or next to rec2vid."
                    .to_string()
            }
            _ => format!("failed to start ffmpeg: {e}"),
        })?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("ffmpeg -version exited with {status}"))
    }
}

pub fn run_ffmpeg(args: &[String]) -> Result<(), String> {
    let status = spawn_ffmpeg(args, Stdio::null())?
        .wait()
        .map_err(|e| e.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("ffmpeg exited with {status}"))
    }
}

// Pipes raw RGB frames into ffmpeg and collects the audio samples
pub struct Encoder {
    child: Child,
    stdin: Option<BufWriter<ChildStdin>>,
    audio: Option<BufWriter<File>>,
    frames_written: u64,
    total_frames: u64,
}

impl Encoder {
    pub fn start(
        width: u32,
        height: u32,
        fps: u32,
        video_path: &Path,
        extra_args: &[String],
        audio_path: Option<&Path>,
        total_frames: u64,
    ) -> Result<Encoder, String> {
        let mut args: Vec<String> = [
            "-y",
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "rawvideo",
            "-pixel_format",
            "rgb24",
            "-video_size",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect();
        args.push(format!("{width}x{height}"));
        args.extend([
            "-framerate".into(),
            fps.to_string(),
            "-i".into(),
            "-".into(),
        ]);
        // Defaults first: ffmpeg lets a later occurrence of an output option win,
        // so anything the user passed after -- overrides these. Image sequences keep
        // ffmpeg's own codec for their format.
        let extension = video_path
            .extension()
            .map(|e| e.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        let is_image = ["png", "jpg", "jpeg", "bmp", "ppm", "tif", "tiff", "webp"]
            .contains(&extension.as_str());
        if !is_image {
            args.extend(
                ["-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "18"]
                    .iter()
                    .map(|s| s.to_string()),
            );
        }
        args.extend(extra_args.iter().cloned());
        args.push(video_path.to_string_lossy().into());

        let mut child = spawn_ffmpeg(&args, Stdio::piped())?;
        let stdin = child
            .stdin
            .take()
            .map(|s| BufWriter::with_capacity(1 << 20, s));
        let audio = match audio_path {
            Some(path) => Some(BufWriter::new(File::create(path).map_err(|e| {
                format!(
                    "failed to open temporary audio file {}: {e}",
                    path.display()
                )
            })?)),
            None => None,
        };
        Ok(Encoder {
            child,
            stdin,
            audio,
            frames_written: 0,
            total_frames,
        })
    }

    pub fn write_frame(&mut self, rgb: &[u8]) -> Result<(), String> {
        let stdin = self.stdin.as_mut().ok_or("ffmpeg is not running")?;
        stdin
            .write_all(rgb)
            .map_err(|_| "ffmpeg closed the pipe before the render finished".to_string())?;
        self.frames_written += 1;
        // The total is estimated from the replay length
        self.total_frames = self.total_frames.max(self.frames_written);
        print!(
            "\rframe {}/{} ({}%)",
            self.frames_written,
            self.total_frames,
            self.frames_written * 100 / self.total_frames
        );
        let _ = std::io::stdout().flush();
        Ok(())
    }

    pub fn write_audio(&mut self, samples: &[i16]) {
        if let Some(audio) = &mut self.audio {
            for s in samples {
                let _ = audio.write_all(&s.to_le_bytes());
            }
        }
    }

    pub fn finish(mut self) -> Result<(), String> {
        if let Some(mut audio) = self.audio.take() {
            audio
                .flush()
                .map_err(|e| format!("failed to write the temporary audio file: {e}"))?;
        }
        if self.frames_written > 0 {
            println!();
        }
        if let Some(mut stdin) = self.stdin.take() {
            let _ = stdin.flush();
        }
        let status = self.child.wait().map_err(|e| e.to_string())?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("ffmpeg exited with {status}"))
        }
    }
}
