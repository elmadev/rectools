// Times how long a replay takes to open and how fast its frames draw, offscreen and without
// vsync, so changes to the renderer can be compared
use elma_formats::lev::ObjectType;
use game_data::{GameFolder, Ghost};
use render::camera::{self, AutoFrame};
use render::wgpu;
use render::{
    Align, CameraMode, Font, Output, RenderOptions, Renderer, Scene, SceneOptions, Text,
    TextureZoom, ViewParams,
};
use replay::{MILLISECONDS_TO_PHYS_TIME, Session};
use std::path::PathBuf;
use std::process::ExitCode;
use std::time::{Duration, Instant};

const FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8Unorm;

const USAGE: &str = "\
Usage: bench <rec or lev> [options]

Opens the replay and draws every frame at 60 fps of replay time as fast as the GPU goes,
then prints how long opening took and the frame times. A level on its own is drawn with
the camera going across it from one end to the other, in --frames frames (default: 600).

Options:
  --data <dir>        game folder, as for rec2vid (default: .)
  --merge-dir <dir>   draw every replay of the same level in <dir> as a ghost
  --size <w>x<h>      size in pixels (default: 1920x1080)
  --zoom <z>          zoom (default: 1)
  --frames <n>        stop after n frames
  --auto-zoom         zoom to fit every bike, like recview's auto zoom
  --passes            also time each pass on the GPU, where it can
  --texts             write 15 lines of text over every frame, as a game's info panel and
                      chat are, set again each frame
";

struct Options {
    input: String,
    data: PathBuf,
    merge_dir: Option<PathBuf>,
    width: u32,
    height: u32,
    zoom: f64,
    frames: Option<usize>,
    auto_zoom: bool,
    passes: bool,
    texts: bool,
}

fn parse() -> Result<Options, String> {
    let mut opts = Options {
        input: String::new(),
        data: PathBuf::from("."),
        merge_dir: None,
        width: 1920,
        height: 1080,
        zoom: 1.0,
        frames: None,
        auto_zoom: false,
        passes: false,
        texts: false,
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or(format!("{name} needs a value"));
        match arg.as_str() {
            "--data" => opts.data = PathBuf::from(value("--data")?),
            "--merge-dir" => opts.merge_dir = Some(PathBuf::from(value("--merge-dir")?)),
            "--size" => {
                let size = value("--size")?;
                let (w, h) = size
                    .split_once('x')
                    .and_then(|(w, h)| Some((w.parse().ok()?, h.parse().ok()?)))
                    .filter(|&(w, h)| w > 0 && h > 0)
                    .ok_or(format!("invalid size: {size}"))?;
                (opts.width, opts.height) = (w, h);
            }
            "--zoom" => {
                let zoom = value("--zoom")?;
                opts.zoom = zoom
                    .parse()
                    .ok()
                    .filter(|z: &f64| *z > 0.0)
                    .ok_or(format!("invalid zoom: {zoom}"))?;
            }
            "--frames" => {
                let frames = value("--frames")?;
                opts.frames = Some(
                    frames
                        .parse()
                        .map_err(|_| format!("invalid frames: {frames}"))?,
                );
            }
            "--auto-zoom" => opts.auto_zoom = true,
            "--passes" => opts.passes = true,
            "--texts" => opts.texts = true,
            "-h" | "--help" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            _ if arg.starts_with('-') => return Err(format!("unknown option: {arg}")),
            _ if opts.input.is_empty() => opts.input = arg,
            _ => return Err(format!("unexpected argument: {arg}")),
        }
    }
    if opts.input.is_empty() {
        return Err("no replay given".into());
    }
    Ok(opts)
}

fn main() -> ExitCode {
    let opts = match parse() {
        Ok(opts) => opts,
        Err(e) => {
            eprintln!("bench: {e}\n\n{USAGE}");
            return ExitCode::from(1);
        }
    };
    match run(&opts) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("bench: {e}");
            ExitCode::from(1)
        }
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn run(opts: &Options) -> Result<(), String> {
    let start = Instant::now();

    let level_only = opts.input.to_ascii_lowercase().ends_with(".lev");
    let mut loaded = if level_only {
        let folder = GameFolder::open(&opts.data).map_err(|e| e.to_string())?;
        folder.load_level(&opts.input).map_err(|e| e.to_string())?
    } else {
        let folder = GameFolder::open_for(&opts.data, &opts.input).map_err(|e| e.to_string())?;
        let no_ghosts: &[Ghost] = &[];
        folder
            .load(&opts.input, "", no_ghosts, None)
            .map_err(|e| e.to_string())?
    };
    if let Some(dir) = &opts.merge_dir {
        let replays =
            game_data::same_level_replays(dir, loaded.rec.level_id).map_err(|e| e.to_string())?;
        let own = game_data::canonical(&loaded.path);
        for (path, rec) in replays {
            if game_data::canonical(&path) != own {
                loaded.add_ghost(&rec);
            }
        }
    }
    let load_time = start.elapsed();

    let t = Instant::now();
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle());
    let (mut renderer, adapter) = pollster::block_on(Renderer::request(&instance, None, "bench"))
        .map_err(|e| e.to_string())?;
    let gpu_time = t.elapsed();
    let timing = opts.passes && renderer.enable_pass_timing();
    if opts.passes && !timing {
        eprintln!("bench: this GPU can't time passes");
    }

    let t = Instant::now();
    let ghosts = loaded.ghosts();
    let shirts = loaded.shirts();
    let mut session = Session::new(&loaded.level, &loaded.rec, &ghosts, loaded.settings);
    let scene = Scene::new(
        &loaded.look,
        &loaded.level,
        &shirts,
        &SceneOptions::default(),
        renderer.atlas_limit(),
    )
    .map_err(|e| e.to_string())?;
    let scene_time = t.elapsed();

    let t = Instant::now();
    let scene = renderer.upload_scene(scene);
    let targets = renderer.create_targets(opts.width, opts.height);
    let output = renderer.device().create_texture(&wgpu::TextureDescriptor {
        label: Some("output"),
        size: wgpu::Extent3d {
            width: opts.width,
            height: opts.height,
            depth_or_array_layers: 1,
        },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: FORMAT,
        usage: wgpu::TextureUsages::RENDER_ATTACHMENT,
        view_formats: &[],
    });
    let output_view = output.create_view(&Default::default());
    let upload_time = t.elapsed();

    let view = ViewParams {
        follow: 0,
        viewport: [0, 0, opts.width, opts.height],
        zoom: opts.zoom,
        camera: CameraMode::Classic,
        pan: Default::default(),
        timer: true,
        minimap: false,
        best_time: loaded.best_time,
        center: None,
        last_apple_time: false,
    };
    let options = RenderOptions {
        textures: if opts.auto_zoom {
            TextureZoom::Stable
        } else {
            TextureZoom::Game
        },
        ..Default::default()
    };

    // A level on its own: the camera goes across it at the height of the start
    let frames = opts.frames.unwrap_or(600);
    let sweep = level_only
        .then(|| {
            let (min, max) = camera::level_bounds(&loaded.level)?;
            let start = loaded
                .level
                .objects
                .iter()
                .find(|o| o.kind == ObjectType::Start)?;
            Some((min.x, max.x, -start.r.y))
        })
        .flatten();

    const FPS: f64 = 60.0;
    let mut auto_frame = AutoFrame::default();
    let mut cpu = Vec::new();
    let mut total = Vec::new();
    let mut first_frame = None;
    // Each pass's times, in the order the passes first came
    let mut passes: Vec<(&'static str, Vec<Duration>)> = Vec::new();
    loop {
        if (opts.frames.is_some() || level_only) && total.len() >= frames {
            break;
        }
        let frame_start = Instant::now();
        let time = total.len() as f64 * (MILLISECONDS_TO_PHYS_TIME * 1000.0) / FPS;
        if session.step(time).done && !level_only {
            break;
        }
        let mut base = view;
        if let Some((left, right, y)) = sweep {
            let along = total.len() as f64 / (frames.max(2) - 1) as f64;
            base.center = Some(replay::Vec2::new(left + (right - left) * along, y));
        }
        let views = camera::layout_views(
            &session,
            base,
            (opts.width, opts.height),
            true,
            opts.auto_zoom.then_some((&mut auto_frame, 1.0 / FPS)),
        );
        if opts.texts {
            let lines: Vec<String> = (0..15)
                .map(|i| format!("line {i}: frame {} of the replay, speed 123.4", total.len()))
                .collect();
            let font = Font::small();
            let texts: Vec<Text> = lines
                .iter()
                .enumerate()
                .map(|(i, line)| Text {
                    font,
                    text: line.as_bytes(),
                    x: 10,
                    y: opts.height as i32 - 30 - i as i32 * font.line_height(),
                    align: Align::Left,
                })
                .collect();
            renderer.set_texts(&texts);
        }
        renderer
            .render(
                &scene,
                &targets,
                &session,
                &views,
                &options,
                Output {
                    view: &output_view,
                    format: FORMAT,
                },
            )
            .map_err(|e| e.to_string())?;
        let cpu_done = frame_start.elapsed();
        // Waits for the GPU, so each frame is timed on its own, without overlapping the next
        renderer
            .device()
            .poll(wgpu::PollType::wait_indefinitely())
            .map_err(|e| e.to_string())?;
        let done = frame_start.elapsed();
        // The first frame is left out below, so its passes are too
        if timing && first_frame.is_some() {
            for (name, ms) in renderer.pass_times().unwrap_or_default() {
                let time = Duration::from_secs_f64(ms / 1000.0);
                match passes.iter_mut().find(|(n, _)| *n == name) {
                    Some((_, times)) => times.push(time),
                    None => passes.push((name, vec![Duration::ZERO, time])),
                }
            }
        }
        first_frame.get_or_insert(done);
        cpu.push(cpu_done);
        total.push(done);
    }
    let launch = start.elapsed() - total.iter().skip(1).sum::<Duration>();

    let info = adapter.get_info();
    println!(
        "{} on {} ({:?}), {}x{}, {} bikes",
        opts.input,
        info.name,
        info.backend,
        opts.width,
        opts.height,
        session.riders.len() + session.ghosts.len()
    );
    println!();
    println!("Launch            {:8.2} ms", ms(launch));
    println!("  load files      {:8.2} ms", ms(load_time));
    println!("  open GPU        {:8.2} ms", ms(gpu_time));
    println!("  build scene     {:8.2} ms", ms(scene_time));
    println!("  upload          {:8.2} ms", ms(upload_time));
    println!(
        "  first frame     {:8.2} ms",
        ms(first_frame.unwrap_or_default())
    );
    println!();
    // The first frame compiles pipelines, so it's left out of the steady state
    if total.len() < 2 {
        println!("Too few frames to time");
        return Ok(());
    }
    // Frames also show how many a second that is
    let report = |name: &str, times: &[Duration], frames: bool| {
        let mut sorted: Vec<f64> = times[1..].iter().map(|&d| ms(d)).collect();
        sorted.sort_by(f64::total_cmp);
        let pct = |p: f64| sorted[((sorted.len() - 1) as f64 * p).round() as usize];
        let mean = sorted.iter().sum::<f64>() / sorted.len() as f64;
        let fps = if frames {
            format!("  ({:.0} fps)", 1000.0 / mean)
        } else {
            String::new()
        };
        println!(
            "{name:<12} mean {mean:6.3}  p50 {:6.3}  p99 {:6.3}  max {:6.3} ms{fps}",
            pct(0.5),
            pct(0.99),
            sorted[sorted.len() - 1],
        );
    };
    println!("{} frames", total.len() - 1);
    report("CPU", &cpu, true);
    report("CPU+GPU", &total, true);
    if !passes.is_empty() {
        // A tiled GPU, like Apple's, starts a pass before the one before it ends, so these
        // overlap and can add up to more than the frame
        println!();
        println!("GPU passes, from start to end:");
        for (name, times) in &passes {
            report(name, times, false);
        }
    }
    Ok(())
}
