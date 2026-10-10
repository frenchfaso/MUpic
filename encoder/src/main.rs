//! Native frontend: FFmpeg handles input decoding; Rust owns all MUpic encoding.
mod native_lut;

use mupic_encoder::*;
use std::{
    env,
    fs::{self, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
};

fn help() {
    println!("MUpic — 16 colors is all you need!\n\nUsage: mupic-encoder INPUT -o OUTPUT.mup [options]\n       mupic-encoder --prepare-lut [--cache-dir PATH]\n\n  --duration SECONDS  Convert a shorter clip (default: whole video)\n  --start SECONDS     Start at this timestamp (default: 0)\n  --threads auto|N    Exact color-search workers (default: auto)\n  --cache-dir PATH    Cache the bundled RGB24 lookup table here\n  --no-cache          Use exact color search without the lookup table\n  --prepare-lut       Populate the lookup cache without converting\n  --gain-db DB       Offline PCM gain (default: 0)\n  --title TITLE      Up to 63 ASCII characters (default: input stem)\n  --force            Replace an existing output\n  --help             Show this help\n\nOutput: native SD MUP9, PAL/3 (~16.708 fps), fixed Bayer8x8 Yliluoma author\npreset + neutral guard, 360x200 canvas, 356x200 maximum active picture.\nThe complete RGB24 lookup table is bundled losslessly compressed and\nexpanded into the system user cache; conversion does not generate it.\nRequires FFmpeg and ffprobe on PATH for conversion.");
}
#[derive(Debug)]
struct Args {
    input: PathBuf,
    output: PathBuf,
    duration: Option<f64>,
    start: f64,
    threads: usize,
    cache_dir: Option<PathBuf>,
    no_cache: bool,
    gain: f64,
    title: Option<String>,
    force: bool,
}
enum Action {
    Encode(Args),
    PrepareLut { threads: usize, cache_dir: PathBuf },
}
fn args() -> Result<Option<Action>, String> {
    let mut iter = env::args().skip(1);
    let mut input = None;
    let mut output = None;
    let mut duration = None;
    let mut start = 0.0;
    let mut threads = std::thread::available_parallelism().map_or(1, usize::from);
    let mut cache_dir = None;
    let mut no_cache = false;
    let mut prepare_lut = false;
    let mut gain = 0.0;
    let mut title = None;
    let mut force = false;
    while let Some(arg) = iter.next() {
        let mut next = || {
            iter.next()
                .ok_or_else(|| format!("Missing value after {arg}"))
        };
        match arg.as_str() {
            "--help" | "-h" => {
                help();
                return Ok(None);
            }
            "-o" | "--output" => output = Some(PathBuf::from(next()?)),
            "--duration" => {
                duration = Some(next()?.parse::<f64>().map_err(|_| "Invalid duration")?)
            }
            "--start" => start = next()?.parse::<f64>().map_err(|_| "Invalid start")?,
            "--threads" => {
                let value = next()?;
                if value != "auto" {
                    threads = value.parse().map_err(|_| "Invalid thread count")?;
                }
            }
            "--gain-db" => gain = next()?.parse::<f64>().map_err(|_| "Invalid gain")?,
            "--cache-dir" => {
                let value = next()?;
                if value.is_empty() {
                    return Err("Cache directory must not be empty".into());
                }
                cache_dir = Some(PathBuf::from(value));
            }
            "--no-cache" => no_cache = true,
            "--prepare-lut" => prepare_lut = true,
            "--title" => title = Some(next()?),
            "--force" => force = true,
            value if value.starts_with('-') => return Err(format!("Unknown option: {value}")),
            value => {
                if input.replace(PathBuf::from(value)).is_some() {
                    return Err("Provide exactly one input video".into());
                }
            }
        }
    }
    if threads == 0 || !start.is_finite() || start < 0.0 || !gain.is_finite() {
        return Err("Invalid thread count, start or gain".into());
    }
    if no_cache && (cache_dir.is_some() || prepare_lut) {
        return Err("--no-cache cannot be combined with --cache-dir or --prepare-lut".into());
    }
    let cache_dir = cache_dir.or_else(native_lut::default_directory);
    if prepare_lut {
        if input.is_some()
            || output.is_some()
            || duration.is_some()
            || start != 0.0
            || gain != 0.0
            || title.is_some()
            || force
        {
            return Err("--prepare-lut accepts only --threads and --cache-dir".into());
        }
        return Ok(Some(Action::PrepareLut {
            threads,
            cache_dir: cache_dir.ok_or("Cannot determine the user cache; provide --cache-dir")?,
        }));
    }
    let input = input.ok_or("Provide an input video; use --help for usage")?;
    let output = output.ok_or("Provide -o OUTPUT.mup")?;
    Ok(Some(Action::Encode(Args {
        input,
        output,
        duration,
        start,
        threads,
        cache_dir,
        no_cache,
        gain,
        title,
        force,
    })))
}
fn probe(input: &Path, args: &[&str]) -> Result<String, String> {
    let result = Command::new("ffprobe")
        .args(["-v", "error"])
        .args(args)
        .arg(input)
        .output()
        .map_err(|e| format!("Cannot run ffprobe: {e}; install FFmpeg first"))?;
    if !result.status.success() {
        return Err(format!(
            "ffprobe failed: {}",
            String::from_utf8_lossy(&result.stderr)
        ));
    }
    Ok(String::from_utf8_lossy(&result.stdout).trim().to_string())
}
fn preflight_output(temporary: &Path) -> Result<(), String> {
    let mut file = OpenOptions::new()
        .write(true)
        .read(true)
        .create_new(true)
        .open(temporary)
        .map_err(|error| {
            format!(
                "Cannot create temporary output {}: {error}",
                temporary.display()
            )
        })?;
    let writable = file.write_all(&[0]).and_then(|()| file.flush());
    drop(file);
    // Remove only the file acquired by create_new, including after write errors.
    let removed = fs::remove_file(temporary);
    writable.map_err(|error| format!("Cannot write temporary output: {error}"))?;
    removed.map_err(|error| format!("Cannot remove temporary output: {error}"))
}
struct DecoderChild(std::process::Child);
impl std::ops::Deref for DecoderChild {
    type Target = std::process::Child;
    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for DecoderChild {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl Drop for DecoderChild {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}
fn run(args: Args) -> Result<(), String> {
    if args.output.exists() && !args.force {
        return Err(format!(
            "Output already exists: {}; use --force to replace it",
            args.output.display()
        ));
    }
    if args
        .output
        .canonicalize()
        .ok()
        .zip(args.input.canonicalize().ok())
        .is_some_and(|(a, b)| a == b)
    {
        return Err("Output must not overwrite the source video".into());
    }
    let aspect = probe(
        &args.input,
        &[
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height,sample_aspect_ratio",
            "-of",
            "csv=p=0",
        ],
    )?;
    let parts: Vec<_> = aspect
        .lines()
        .next()
        .ok_or("Video has no video stream")?
        .split(',')
        .collect();
    let width: f64 = parts
        .first()
        .ok_or("Missing width")?
        .parse()
        .map_err(|_| "Invalid width")?;
    let height: f64 = parts
        .get(1)
        .ok_or("Missing height")?
        .parse()
        .map_err(|_| "Invalid height")?;
    let sar = parts
        .get(2)
        .and_then(|v| v.split_once(':'))
        .and_then(|(a, b)| Some(a.parse::<f64>().ok()? / b.parse::<f64>().ok()?))
        .filter(|v| v.is_finite() && *v > 0.0)
        .unwrap_or(1.0);
    let dar = width * sar / height;
    if !dar.is_finite() || dar <= 0.0 {
        return Err("Invalid video aspect ratio".into());
    }
    let (active_w, active_h) = if dar >= 356.0 / 200.0 {
        (356, ((356.0 / dar).round() as usize).clamp(1, 200))
    } else {
        (
            (((200.0 * dar).round() as usize) / 2 * 2).clamp(2, 356),
            200,
        )
    };
    let left = (360 - active_w) / 2;
    let top = (200 - active_h) / 2;
    let full_duration = probe(
        &args.input,
        &[
            "-show_entries",
            "format=duration",
            "-of",
            "default=nw=1:nk=1",
        ],
    )?
    .parse::<f64>()
    .ok()
    .filter(|v| v.is_finite() && *v > 0.0);
    let duration = args
        .duration
        .or_else(|| full_duration.map(|d| d - args.start))
        .ok_or("Input duration is unknown; provide --duration")?;
    if full_duration.is_some_and(|full| args.start >= full || duration > full - args.start + 0.5) {
        return Err("Requested clip exceeds input duration".into());
    }
    let count = frame_count(duration)?;
    let seconds = count as f64 / content_fps();
    let audio_present = !probe(
        &args.input,
        &[
            "-select_streams",
            "a:0",
            "-show_entries",
            "stream=index",
            "-of",
            "csv=p=0",
        ],
    )?
    .is_empty();
    let title = args.title.unwrap_or_else(|| {
        args.input
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .chars()
            .map(|c| if c.is_ascii() && c != '\0' { c } else { '?' })
            .take(63)
            .collect()
    });
    if !title.is_ascii() || title.contains('\0') || title.len() > 63 {
        return Err("Title must contain at most 63 ASCII bytes".into());
    }
    let temporary = args.output.with_file_name(format!(
        ".{}.partial-{}",
        args.output
            .file_name()
            .unwrap_or_default()
            .to_string_lossy(),
        std::process::id()
    ));
    preflight_output(&temporary)?;
    let mut quantizer = Quantizer::new(args.threads)?;
    if !args.no_cache {
        let prepared = native_lut::LutCache::new(args.cache_dir).prepare(&mut quantizer, false);
        if let Err(error) = prepared {
            quantizer.clear_lut();
            eprintln!(
                "MUpic: RGB24 lookup table unavailable ({error}); using exact CPU color search."
            );
        }
    }
    let pcm = if audio_present {
        let audio = Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-nostdin",
                "-ss",
                &args.start.to_string(),
                "-i",
            ])
            .arg(&args.input)
            .args([
                "-map",
                "0:a:0",
                "-vn",
                "-af",
                &format!("volume={}dB,apad", args.gain),
                "-t",
                &seconds.to_string(),
                "-ac",
                "2",
                "-ar",
                &AUDIO_RATE.to_string(),
                "-f",
                "s16le",
                "pipe:1",
            ])
            .output()
            .map_err(|e| format!("Cannot run FFmpeg: {e}"))?;
        if !audio.status.success() {
            return Err(format!(
                "Audio decoding failed: {}",
                String::from_utf8_lossy(&audio.stderr)
            ));
        }
        audio.stdout
    } else {
        eprintln!("Input has no audio track; encoding silence.");
        vec![]
    };
    let mut encoder = Encoder::new(count, pcm, title)?;
    let filter = format!("tpad=stop_mode=clone:stop_duration={seconds},fps={},scale={active_w}:{active_h}:flags=lanczos,setsar=1,pad=360:200:{left}:{top}:black,format=rgb24", content_fps());
    let mut child = DecoderChild(
        Command::new("ffmpeg")
            .args([
                "-v",
                "error",
                "-nostdin",
                "-ss",
                &args.start.to_string(),
                "-i",
            ])
            .arg(&args.input)
            .args([
                "-an",
                "-vf",
                &filter,
                "-frames:v",
                &count.to_string(),
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgb24",
                "pipe:1",
            ])
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit())
            .spawn()
            .map_err(|e| format!("Cannot run FFmpeg: {e}"))?,
    );
    let mut source = child.stdout.take().ok_or("FFmpeg did not expose stdout")?;
    let mut output = OpenOptions::new()
        .write(true)
        .read(true)
        .create_new(true)
        .open(&temporary)
        .map_err(|e| {
            format!(
                "Cannot create temporary output {}: {e}",
                temporary.display()
            )
        })?;
    let work = (|| -> Result<(), String> {
        output
            .seek(SeekFrom::Start(encoder.stream_offset() as u64))
            .map_err(|e| e.to_string())?;
        eprintln!(
            "Encoding {count} frames at {:.6} fps, active {active_w}x{active_h}, {} CPU workers.",
            content_fps(),
            args.threads
        );
        let mut batch = vec![0; RGB_BYTES * 8];
        let mut done = 0;
        while done < count {
            let n = 8.min(count - done);
            let bytes = &mut batch[..n * RGB_BYTES];
            source
                .read_exact(bytes)
                .map_err(|e| format!("Truncated video before frame {done}: {e}"))?;
            quantizer.preplan(bytes)?;
            for rgb in bytes.chunks_exact(RGB_BYTES) {
                let packed = quantizer.pack_plans(rgb);
                encoder.push(&packed)?;
                output
                    .write_all(&encoder.take_output())
                    .map_err(|e| e.to_string())?;
                done += 1;
            }
            eprintln!(
                "{done}/{count} frames; {} adapted; peak {}/{} cycles",
                encoder.adapted_frames, encoder.peak_slots, RATE_BUDGET
            );
        }
        drop(source);
        if !child.wait().map_err(|e| e.to_string())?.success() {
            return Err("FFmpeg video decoder failed".into());
        }
        let prefix = encoder.finish()?;
        output
            .write_all(&encoder.take_output())
            .map_err(|e| e.to_string())?;
        output.seek(SeekFrom::Start(0)).map_err(|e| e.to_string())?;
        output.write_all(&prefix).map_err(|e| e.to_string())?;
        output.flush().map_err(|e| e.to_string())?;
        output.sync_all().map_err(|e| e.to_string())?;
        #[cfg(target_os = "windows")]
        if args.force && args.output.exists() {
            fs::remove_file(&args.output).map_err(|e| e.to_string())?;
        }
        if args.output.exists() && !args.force {
            return Err("Output was created during encoding; refusing to overwrite it".into());
        }
        fs::rename(&temporary, &args.output).map_err(|e| format!("Cannot publish output: {e}"))?;
        eprintln!(
            "Saved {} ({:.2} MiB), {:.3}s, {} adapted frames, peak {}/{} cycles.",
            args.output.display(),
            output.metadata().map_err(|e| e.to_string())?.len() as f64 / 1048576.0,
            seconds,
            encoder.adapted_frames,
            encoder.peak_slots,
            RATE_BUDGET
        );
        Ok(())
    })();
    if work.is_err() {
        let _ = child.kill();
        let _ = child.wait();
        drop(output);
        let _ = fs::remove_file(&temporary);
    }
    work
}
fn main() {
    let result = args().and_then(|action| match action {
        None => Ok(()),
        Some(Action::Encode(args)) => run(args),
        Some(Action::PrepareLut { threads, cache_dir }) => {
            let mut quantizer = Quantizer::new(threads)?;
            native_lut::LutCache::new(Some(cache_dir)).prepare(&mut quantizer, true)
        }
    });
    if let Err(error) = result {
        eprintln!("MUpic: {error}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preflight_rejects_missing_parent_and_preserves_existing_files() {
        let directory = env::temp_dir().join(format!(
            "mupic-output-preflight-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir(&directory).unwrap();
        let output = directory.join("output.mup");
        let temporary = directory.join(".output.mup.partial");
        fs::write(&output, b"original output").unwrap();
        assert!(preflight_output(&directory.join("missing/output.partial")).is_err());
        fs::write(&temporary, b"another operation").unwrap();
        assert!(preflight_output(&temporary).is_err());
        assert_eq!(fs::read(&temporary).unwrap(), b"another operation");
        fs::remove_file(&temporary).unwrap();
        preflight_output(&temporary).unwrap();
        assert!(!temporary.exists());
        assert_eq!(fs::read(&output).unwrap(), b"original output");
        fs::remove_dir_all(directory).unwrap();
    }
}
