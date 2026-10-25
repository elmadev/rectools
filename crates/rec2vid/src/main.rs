mod encoder;
mod gpu;

use audio::{Mixer, ReplayAudio, SAMPLE_RATE, Sounds};
use encoder::{Encoder, run_ffmpeg};
use game_data::{GameFolder, Ghost, Loaded};
use render::camera::{self, AutoFrame};
use render::{
    CameraMode, DynamicOptions, GhostStyle, RenderOptions, Scene, SceneOptions, TextureZoom,
    ViewParams,
};
use replay::{MILLISECONDS_TO_PHYS_TIME, Session};
use std::path::{Path, PathBuf};
use std::process::ExitCode;
use std::sync::Arc;

const EXIT_USAGE: u8 = 1;
const EXIT_FFMPEG: u8 = 2;

struct Options {
    input: String,
    output: String,
    nick: String,
    merge: Vec<Ghost>,
    audio: bool,
    data: PathBuf,
    width: u32,
    height: u32,
    fps: u32,
    zoom: f64,
    camera: CameraMode,
    timer: bool,
    zoom_textures: bool,
    minimap: bool,
    last_apple_time: bool,
    pause_end: bool,
    pictures_in_background: bool,
    still_objects: bool,
    translucent_ghosts: bool,
    auto_zoom: bool,
    merge_dir: Option<PathBuf>,
    follow_fastest: bool,
    only_finishes: bool,
    default_ground: bool,
    default_sky: bool,
    frames: Option<u64>,
    ffmpeg_args: Vec<String>,
}

fn print_usage() {
    println!(
        "Usage: rec2vid <input.rec> <output-file> [options] [-- <ffmpeg args>...]

Renders a replay to a video file using ffmpeg, which must be on your PATH or next to
rec2vid.

Options:
  --no-audio          render video only
  --merge <rec>       draw another replay as a ghost (repeatable)
  --merge-dir <dir>   draw every replay of the same level in <dir> as a ghost
  --follow-fastest    follow the replay that finishes fastest, the others are ghosts
  --only-finishes     leave out ghosts that don't finish the level
  --nick <nick>       shirt (bmp/<nick>.bmp) for the replay named before it
  --data <dir>        game folder with elma.res, lev/, lgr/, rec/ and bmp/, or with
                      across.res, lev/ and rec/ (default: .)
  --size <w>x<h>      video size in pixels (default: 800x600)
  --fps <n>           frames per second (default: 30)
  --zoom <z>          zoom, any positive number (default: 1)
  --no-zoom-textures  the ground, sky and qgrass textures keep their size at zoom 1,
                      like elma-classic with Zoom Textures off
  --camera <mode>     classic or centered (default: classic)
  --no-timer          hide the timer
  --minimap           show the minimap
  --last-apple-time   show the game's last apple time in the bottom right (not for Across)
  --pause-end         hold the last frame for one second
  --pictures-in-background
                      draw the bikes over every picture
  --still-objects     apples and the exit don't bob up and down
  --translucent-ghosts
                      draw the --merge ghosts blue and see-through
  --auto-zoom         zoom out when needed to keep every ghost in view
  --default-ground    use the LGR's ground texture
  --default-sky       use the LGR's sky texture
  --frames <n>        stop after n frames
  -h, --help          show this help

Arguments after -- are passed to ffmpeg and override the built-in
encoding defaults (-c:v libx264 -pix_fmt yuv420p -crf 18).

Examples:
  rec2vid rec/blabla.rec out/video.avi -- -c:v libx264 -crf 22
  rec2vid rec/a.rec out/a.mp4 --merge rec/b.rec --merge c.rec
  rec2vid a.rec out/a.mp4 --nick Bene --merge b.rec --nick Zweq
  rec2vid a.rec 'frames/%05d.png' --no-audio --zoom 2.5"
    );
}

fn parse_arguments() -> Result<Options, String> {
    let mut opts = Options {
        input: String::new(),
        output: String::new(),
        nick: String::new(),
        merge: Vec::new(),
        audio: true,
        data: PathBuf::from("."),
        width: 800,
        height: 600,
        fps: 30,
        zoom: 1.0,
        camera: CameraMode::Classic,
        timer: true,
        zoom_textures: true,
        minimap: false,
        last_apple_time: false,
        pause_end: false,
        pictures_in_background: false,
        still_objects: false,
        translucent_ghosts: false,
        auto_zoom: false,
        merge_dir: None,
        follow_fastest: false,
        only_finishes: false,
        default_ground: false,
        default_sky: false,
        frames: None,
        ffmpeg_args: Vec::new(),
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "--" => {
                opts.ffmpeg_args.extend(args.by_ref());
                break;
            }
            "-h" | "--help" => {
                print_usage();
                std::process::exit(0);
            }
            "--no-audio" => opts.audio = false,
            "--merge" => opts.merge.push(Ghost {
                path: value("--merge")?,
                nick: String::new(),
            }),
            "--nick" => {
                let nick = value("--nick")?;
                match opts.merge.last_mut() {
                    Some(merge) => merge.nick = nick,
                    None => opts.nick = nick,
                }
            }
            "--data" => opts.data = PathBuf::from(value("--data")?),
            "--size" => {
                let size = value("--size")?;
                let (w, h) = size
                    .split_once('x')
                    .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
                    .ok_or_else(|| format!("invalid size: {size}"))?;
                if w < 16 || h < 16 || w > 16384 || h > 16384 {
                    return Err(format!("invalid size: {size}"));
                }
                (opts.width, opts.height) = (w, h);
            }
            "--fps" => {
                let fps = value("--fps")?;
                opts.fps = fps
                    .parse()
                    .ok()
                    .filter(|&f| f > 0 && f <= 1000)
                    .ok_or_else(|| format!("invalid fps: {fps}"))?;
            }
            "--zoom" => {
                let zoom = value("--zoom")?;
                opts.zoom = zoom
                    .parse()
                    .ok()
                    .filter(|&z: &f64| z > 0.0 && z.is_finite())
                    .ok_or_else(|| format!("invalid zoom: {zoom}"))?;
            }
            "--camera" => opts.camera = value("--camera")?.parse()?,
            "--frames" => {
                let frames = value("--frames")?;
                opts.frames = Some(
                    frames
                        .parse()
                        .map_err(|_| format!("invalid frame count: {frames}"))?,
                );
            }
            "--no-timer" => opts.timer = false,
            "--no-zoom-textures" => opts.zoom_textures = false,
            "--minimap" => opts.minimap = true,
            "--last-apple-time" => opts.last_apple_time = true,
            "--pause-end" => opts.pause_end = true,
            "--pictures-in-background" => opts.pictures_in_background = true,
            "--still-objects" => opts.still_objects = true,
            "--translucent-ghosts" => opts.translucent_ghosts = true,
            "--auto-zoom" => opts.auto_zoom = true,
            "--merge-dir" => opts.merge_dir = Some(PathBuf::from(value("--merge-dir")?)),
            "--follow-fastest" => opts.follow_fastest = true,
            "--only-finishes" => opts.only_finishes = true,
            "--default-ground" => opts.default_ground = true,
            "--default-sky" => opts.default_sky = true,
            _ if arg.starts_with('-') => return Err(format!("unknown option: {arg}")),
            _ if opts.input.is_empty() => opts.input = arg,
            _ if opts.output.is_empty() => opts.output = arg,
            _ => return Err(format!("unexpected argument: {arg}")),
        }
    }
    Ok(opts)
}

// An exit code and what went wrong
type Failure = (u8, String);

fn usage(e: impl std::fmt::Display) -> Failure {
    (EXIT_USAGE, e.to_string())
}

fn ffmpeg(e: impl std::fmt::Display) -> Failure {
    (EXIT_FFMPEG, e.to_string())
}

// Ends the progress line first
fn frame_error(e: impl std::fmt::Display) -> Failure {
    eprintln!();
    ffmpeg(e)
}

fn main() -> ExitCode {
    let opts = match parse_arguments() {
        Ok(opts) => opts,
        Err(message) => return fail(usage(message)),
    };
    if opts.input.is_empty() || opts.output.is_empty() {
        print_usage();
        return ExitCode::from(EXIT_USAGE);
    }
    match run(&opts) {
        Ok(()) => ExitCode::SUCCESS,
        Err(failure) => fail(failure),
    }
}

fn fail((code, message): Failure) -> ExitCode {
    eprintln!("rec2vid: {message}");
    ExitCode::from(code)
}

fn run(opts: &Options) -> Result<(), Failure> {
    let output = PathBuf::from(&opts.output);
    if output.extension().is_none() {
        return Err(usage(
            "the output file needs an extension, e.g. .avi or .mp4",
        ));
    }
    encoder::check_ffmpeg().map_err(ffmpeg)?;
    let (loaded, folder) = load(opts)?;
    let sounds = if opts.audio {
        let sounds = Sounds::load_files(&|name| folder.file(name), folder.is_across());
        Some(sounds.map_err(|e| usage(format!("cannot load the sounds: {e}")))?)
    } else {
        None
    };

    if let Some(parent) = output.parent()
        && !parent.as_os_str().is_empty()
        && let Err(e) = std::fs::create_dir_all(parent)
    {
        return Err(usage(format!("failed to create {}: {e}", parent.display())));
    }

    // With audio the video is encoded to a temporary file first and muxed with the
    // samples afterwards, because ffmpeg can only read one pipe portably
    let stem = output
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let extension = output
        .extension()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let temp_video = output.with_file_name(format!("{stem}.rec2vid-tmp.{extension}"));
    let temp_audio = output.with_file_name(format!("{stem}.rec2vid-tmp.pcm"));

    print!(
        "Rendering {} to {} at {}x{}, {} fps{}",
        loaded.path.display(),
        output.display(),
        opts.width,
        opts.height,
        opts.fps,
        if opts.audio { "" } else { ", no audio" }
    );
    let ghosts = loaded.riders.len() - loaded.rec.rides.len();
    if ghosts > 0 {
        print!(", {} ghost{}", ghosts, if ghosts == 1 { "" } else { "s" });
    }
    println!();

    let video_target = if opts.audio { &temp_video } else { &output };
    let temp_audio_target = opts.audio.then_some(temp_audio.as_path());
    let encoder = render(opts, loaded, sounds, video_target, temp_audio_target)?;
    let remove_temporaries = || {
        let _ = std::fs::remove_file(&temp_video);
        let _ = std::fs::remove_file(&temp_audio);
    };
    if let Err(e) = encoder.finish() {
        remove_temporaries();
        return Err(ffmpeg(e));
    }
    if opts.audio {
        println!("Muxing audio...");
        let status = mux_audio(&temp_video, &temp_audio, &output);
        remove_temporaries();
        status.map_err(|e| ffmpeg(format!("{e} while muxing audio")))?;
    }
    println!("Wrote {}", output.display());
    Ok(())
}

// The replay with its ghosts, after --merge-dir, --only-finishes and --follow-fastest
fn load(opts: &Options) -> Result<(Loaded, GameFolder), Failure> {
    let folder = GameFolder::open_for(&opts.data, &opts.input).map_err(usage)?;
    let mut loaded = folder
        .load(&opts.input, &opts.nick, &opts.merge, None)
        .map_err(usage)?;
    for warning in &loaded.warnings {
        eprintln!("rec2vid: {warning}");
    }

    if let Some(dir) = &opts.merge_dir {
        let open: Vec<PathBuf> = std::iter::once(loaded.path.clone())
            .chain(
                opts.merge
                    .iter()
                    .filter_map(|m| folder.resolve_replay(&m.path)),
            )
            .map(|p| game_data::canonical(&p))
            .collect();
        let replays = game_data::same_level_replays(dir, loaded.rec.level_id).map_err(usage)?;
        let mut merged = 0;
        for (path, rec) in replays {
            if !open.contains(&game_data::canonical(&path)) {
                loaded.add_ghost(&rec);
                merged += 1;
            }
        }
        println!("Merging {merged} replays from {}", dir.display());
    }
    if opts.only_finishes {
        let before = loaded.riders.len() - loaded.rec.rides.len();
        let left_out = loaded.retain_finishers();
        println!("Leaving out {left_out} of {before} ghosts that don't finish");
    }
    if opts.follow_fastest {
        match loaded.follow_fastest(opts.only_finishes) {
            Some(time) => {
                let centiseconds = (time * replay::TIME_TO_CENTISECONDS) as i64;
                println!(
                    "Following the fastest finish, {}",
                    String::from_utf8_lossy(&render::format_centiseconds(centiseconds))
                );
            }
            None => eprintln!(
                "rec2vid: none of the replays finish, following {}",
                loaded.path.display()
            ),
        }
    }
    Ok((loaded, folder))
}

// Encodes every frame, and the sound into `temp_audio` if there is any
fn render(
    opts: &Options,
    loaded: Loaded,
    sounds: Option<Sounds>,
    video_target: &Path,
    temp_audio: Option<&Path>,
) -> Result<Encoder, Failure> {
    let ghosts = loaded.ghosts();
    let shirts = loaded.shirts();
    let Loaded {
        rec,
        level,
        look,
        best_time,
        settings,
        ..
    } = loaded;
    let mut session = Session::new(&level, &rec, &ghosts, settings);
    let mut gpu = gpu::Headless::new(opts.width, opts.height).map_err(usage)?;
    let options = SceneOptions {
        default_ground: opts.default_ground,
        default_sky: opts.default_sky,
        ..SceneOptions::default()
    };
    let scene = Scene::new(&look, &level, &shirts, &options, gpu.renderer.atlas_limit());
    let scene = gpu.renderer.upload_scene(scene.map_err(usage)?);

    let view = ViewParams {
        follow: 0,
        viewport: [0, 0, opts.width, opts.height],
        zoom: opts.zoom,
        camera: opts.camera,
        pan: Default::default(),
        timer: opts.timer,
        minimap: opts.minimap,
        best_time,
        center: None,
        last_apple_time: opts.last_apple_time,
    };
    let render_options = RenderOptions {
        dynamic: DynamicOptions {
            pictures_in_background: opts.pictures_in_background,
            still_objects: opts.still_objects,
            ..Default::default()
        },
        ghosts: GhostStyle::new(opts.translucent_ghosts),
        // At a fixed zoom the game's texture alignment matches the original frames at any zoom;
        // when the zoom keeps changing it makes textures jump around
        textures: if opts.auto_zoom {
            TextureZoom::Stable
        } else {
            TextureZoom::Game
        },
        zoom_textures: opts.zoom_textures,
        // The game's 1 pixel lines
        hud_scale: 1,
        hud_bottom_inset: 0,
    };

    let fps = opts.fps as f64;
    let replay_frames = session.duration() / (MILLISECONDS_TO_PHYS_TIME * 1000.0);
    let mut total_frames = (replay_frames * fps).ceil() as u64 + 1;
    if opts.pause_end {
        total_frames += opts.fps as u64;
    }
    if let Some(frames) = opts.frames {
        total_frames = total_frames.min(frames);
    }

    let mut encoder = Encoder::start(
        opts.width,
        opts.height,
        opts.fps,
        video_target,
        &opts.ffmpeg_args,
        temp_audio,
        total_frames,
    )
    .map_err(ffmpeg)?;

    let mut audio = sounds.map(|s| ReplayAudio::new(Arc::new(s), session.riders.len()));
    let mut samples_written: u64 = 0;
    let mut audio_buffer = Vec::new();
    let mut write_audio = |encoder: &mut Encoder, mixer: &mut Mixer, frame: u64| {
        let target = ((frame + 1) as f64 * SAMPLE_RATE as f64 / fps).round() as u64;
        audio_buffer.resize((target - samples_written) as usize, 0);
        mixer.mix(&mut audio_buffer);
        samples_written = target;
        encoder.write_audio(&audio_buffer);
    };

    let mut auto_frame = AutoFrame::default();
    let mut frame: u64 = 0;
    let mut stopped_early = false;
    loop {
        if opts.frames.is_some_and(|n| frame >= n) {
            stopped_early = true;
            break;
        }
        let time = frame as f64 * (MILLISECONDS_TO_PHYS_TIME * 1000.0) / fps;
        let step = session.step(time);
        if let Some(audio) = &mut audio {
            audio.play_sounds(&step);
        }
        if step.done {
            break;
        }
        if let Some(audio) = &mut audio {
            audio.follow(&session, Some(&step));
            write_audio(&mut encoder, &mut audio.mixer, frame);
        }
        let views = camera::layout_views(
            &session,
            view,
            (opts.width, opts.height),
            true,
            opts.auto_zoom.then_some((&mut auto_frame, 1.0 / fps)),
        );
        let pixels = gpu
            .render(&scene, &session, &views, &render_options)
            .map_err(usage)?;
        encoder.write_frame(pixels).map_err(frame_error)?;
        frame += 1;
    }

    if !stopped_early && opts.pause_end {
        if let Some(audio) = &mut audio {
            audio.stop();
        }
        let pixels = gpu.last_frame().to_vec();
        for i in 0..opts.fps as u64 {
            if let Some(audio) = &mut audio {
                write_audio(&mut encoder, &mut audio.mixer, frame + i);
            }
            encoder.write_frame(&pixels).map_err(frame_error)?;
        }
    }
    Ok(encoder)
}

fn mux_audio(temp_video: &Path, temp_audio: &Path, output: &Path) -> Result<(), String> {
    run_ffmpeg(&[
        "-y".into(),
        "-hide_banner".into(),
        "-loglevel".into(),
        "error".into(),
        "-i".into(),
        temp_video.to_string_lossy().into(),
        "-f".into(),
        "s16le".into(),
        "-ar".into(),
        SAMPLE_RATE.to_string(),
        "-ac".into(),
        "1".into(),
        "-i".into(),
        temp_audio.to_string_lossy().into(),
        "-c:v".into(),
        "copy".into(),
        output.to_string_lossy().into(),
    ])
}
