//! MUpic span v1: offline colour conversion and small 6502 span packets.
#![allow(dead_code)]
//! The FFmpeg child only decodes/resizes input media. Quantisation, packing,
//! packet selection and reference decoding are implemented here in Rust.
use std::collections::HashMap;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
pub mod author_dither;
mod buffered;
mod dict4_rate;
mod small_wide;
mod wide;
mod wide_dict4;
mod wide_lz4;
mod wide_raw;
mod wide_skip;
use author_dither::AuthorDither;

const WIDTH: usize = 320;
const HEIGHT: usize = 200;
const FRAME_BYTES: usize = 32000;
const REGIONS: [(usize, usize, usize); 2] = [(0, 26624, 0x2000), (26624, 5376, 0xa800)];
const PEPTO: [i32; 16] = [0, 32, 10, 20, 12, 16, 8, 24, 12, 8, 16, 10, 15, 24, 15, 20];
// Includes dispatch/setup/advance; body figures conservatively include load crossing
// for FLIP2 and per-page operand updates/control for aligned FLIP_PAGES.
const FLIP2_HEADER_CYCLES: usize = 190;
const FLIP2_PAIR_CYCLES: usize = 28;
const FLIP_PAGE_HEADER_CYCLES: usize = 180;
const FLIP_PAGE_CYCLES: usize = 2980;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ColourMode {
    Hardware,
    Temporal,
    Spatial,
}
impl ColourMode {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "hardware" => Ok(Self::Hardware),
            "temporal" => Ok(Self::Temporal),
            "spatial" => Ok(Self::Spatial),
            _ => Err("colour-mode must be hardware, temporal or spatial".into()),
        }
    }
    fn name(self) -> &'static str {
        match self {
            Self::Hardware => "hardware",
            Self::Temporal => "temporal",
            Self::Spatial => "spatial",
        }
    }
}

#[derive(Clone, Debug)]
struct Options {
    input: PathBuf,
    output: PathBuf,
    palette: PathBuf,
    start: f64,
    duration: f64,
    fps: f64,
    colour_mode: ColourMode,
    phase_repeat: usize,
    frame_ticks: Option<usize>,
    fit_canvas: bool,
    align: String,
    pixel_aspect: f64,
    audio_rate: u32,
    audio_gain_db: f64,
    scale: usize,
    packet_budget: usize,
    strict_budget: bool,
    luma_limit: i32,
    rgb_luma_limit: Option<f64>,
    dither: String,
    dither_contrast_weight: f64,
    phase_layout: String,
    neutral_guard: bool,
    audio_offset: usize,
    audio_divider: u16,
    native_mux: bool,
    cache_ticks: Option<usize>,
    buffered: bool,
    tick_budget: usize,
    threads: Option<usize>,
    upic_width: usize,
    wide_lz4_source: Option<PathBuf>,
    wide_raw_source: Option<PathBuf>,
    wide_skip_source: Option<PathBuf>,
    wide360_raw_source: Option<PathBuf>,
    wide_dict4_source: Option<PathBuf>,
    wide360_dict: bool,
    streaming_dict4: bool,
    dict4_full_height: bool,
    dict4_rate_budget: Option<usize>,
    dict4_prefetch: usize,
    source_frame_limit: Option<usize>,
    lz4_library: Option<PathBuf>,
    lz4_coalesce: bool,
}

fn usage() {
    println!(
        "MUpic encoder span v1 — Rust/std-only, FFmpeg input decoder\n\
Usage: mupic-encoder --input VIDEO --output DIRECTORY [options]\n\
  --start SECONDS         Input seek (default 60)\n\
  --duration SECONDS      Clip duration (default 4)\n\
  --fps RATE             Logical source frames/s (default 25)\n\
  --colour-mode MODE     hardware, temporal (default), or static spatial checker\n\
  --phase-repeat N       Repeat each AB pair 1..8 times (default 1)\n\
  --frame-ticks N        Hold hardware/spatial content for N PAL ticks (1..255)\n\
                         fps defaults to PAL tick rate/N and must match it\n\
  --fit-canvas           Preserve source DAR in a 320x200 canvas with black bars\n\
  --align POSITION       start, center (default), or end on both canvas axes\n\
  --pixel-aspect RATIO   Output pixel width/height (default 1; requires fit-canvas)\n\
  --audio-rate HZ         Stereo signed 16-bit PCM sample rate (default 22050)\n\
  --audio-gain-db DB      Offline PCM gain, rounded/clamped to S16 (default 0)\n\
  --audio-offset BYTES    PCM offset (default32768; cache-ticks20:131072)\n\
  --cache-ticks N         Fixed256 KiB mux:20 ticks; RAW360 PAL/3 uses16\n\
  --audio-divider VALUE   UltimateAudio divider (default 281)\n\
  --no-native-mux         Emit raw video/PCM only\n\
  --buffered              Buffered v2 back-slot updates; hardware/scale1/320x169\n\
  --tick-budget SLOTS     Buffered estimated total CPU/DMA slots (default 490000)\n\
  --palette PATH         Ultimate VIC-II palette JSON\n\
  --scale2               Quantise at 160x90, expand to 320x180\n\
  --scale4               Quantise at 80x45, expand to 320x180\n\
  --luma-limit LEVEL     Pair luminance difference 0,4,8,16,32 (default 16)\n\
  --rgb-luma-limit FLOAT Additional relative Rec.709 linear-light Y limit\n\
  --dither MODE          none, jjn (default), hardware yliluoma or yliluoma-author\n\
  --dither-contrast-weight W  Yliluoma contrast penalty weight (default 0.1)\n\
  --threads auto|N       Author RGB planning workers (default auto, positive N)\n\
  --upic-width N         Legacy320, isolated360x200 RAW v7 or384x256 author mode\n\
  --wide-lz4-source DIR  Repack existing384 PAL/5 oracle.frames +PCM with HC12\n\
  --wide-raw-source DIR  Reuse384 PAL/5 frames with direct active-column DMA v5\n\
  --wide-skip-source DIR Reuse384 PAL/4 frames, omit exact unchanged columns v6\n\
  --wide360-raw-source DIR Reuse genuine360 PAL/3 orPAL/4 author frames v7\n\
  --wide360-dict         Direct360 PAL/3 author conversion to dictionary v8\n\
                         Requires --frame-ticks 3; cache20 default,16 optional\n\
  --streaming-dict4      Bounded-memory fresh v8 conversion; compact SHA256 oracles\n\
                         Requires --wide360-dict; current player limit65535 ticks\n\
  --dict4-full-height    Native SD codec9: all200 rows, square display pixels\n\
  --wide-dict4-source DIR Reuse true360 PAL/3 frames with word dictionary v8\n\
  --dict4-rate-budget N  Allow perceptual word substitutions to fit N CPU/DMA slots\n\
  --dict4-prefetch N     Dictionary player prefetch bytes/row:12(default) or13\n\
  --source-frame-limit N  Restrict reuse probe to first N content frames\n\
  --lz4-library PATH    Optional liblz4 shared library for HC12 reuse mode\n\
  --lz4-coalesce        Replan HC short matches as literals using measured costs\n\
  --phase-layout MODE    uniform or checker (default uniform)\n\
  --neutral-guard        Restrict neutral source colours to neutral pairs\n\
  --packet-budget BYTES  Player packet RAM capacity (default 12288)\n\
  --strict-budget        Fail after writing statistics if a packet exceeds RAM\n\
  --help                 Show this help\n\
Output: stream.bin (LE u16 length + span packets), initial.bin, oracle.phases,\n\
audio.pcm (stereo S16LE), mixed.rgb, source.rgb, manifest.json.\n\
Native output: stream.mup (video + PCM in 64 KiB cache blocks), cache-mux.json.\n\
Each packet: destination-minus-one LE u16, count u8, tag u8, data.\n\
Tags: 0 = count literal bytes, 1 = repeated byte, 2 = two alternating bytes;\n\
3 = count/4 bytes each expanding low-colour twice then high-colour twice;\n\
4 = count/8 bytes each expanding low-colour four times then high-colour four times.\n\
5/6 = EXPAND4 duplicated to both neighbouring columns (count 64/8 rows).\n\
7 = count/2 bytes each expanding V then swapped-nibble V (count even).\n\
8 = nibble-swap existing pairs in place (even count 2..254, even destination).\n\
9 = nibble-swap count aligned 256-byte pages in place. Tags 8/9 have no data;\n\
their input must contain adjacent pairs V, swapped-nibble V.\n\
Four zero bytes terminate the packet. Counts are 1..255.\n\
Temporal mode uses AB phases. Hardware/spatial mode holds one image per frame;\n\
oracle.frames additionally stores its logical content frames before holds.\n\
stream.bin starts from initial.bin and loops from the final display tick to it."
    );
}

fn parse_options() -> Result<Option<Options>, String> {
    parse_options_args(env::args().skip(1))
}
fn parse_options_args(mut args: impl Iterator<Item = String>) -> Result<Option<Options>, String> {
    let mut fps_explicit = false;
    let mut phase_layout_explicit = false;
    let mut audio_offset_explicit = false;
    let mut tick_budget_explicit = false;
    let mut o = Options {
        input: PathBuf::new(),
        output: PathBuf::new(),
        palette: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../assets/palette.json"),
        start: 60.0,
        duration: 4.0,
        fps: 25.0,
        colour_mode: ColourMode::Temporal,
        phase_repeat: 1,
        frame_ticks: None,
        fit_canvas: false,
        align: "center".into(),
        pixel_aspect: 1.0,
        audio_rate: 22050,
        audio_gain_db: 0.0,
        scale: 1,
        packet_budget: 12288,
        strict_budget: false,
        luma_limit: 16,
        rgb_luma_limit: None,
        dither: "jjn".into(),
        dither_contrast_weight: YLILUOMA_CONTRAST_WEIGHT,
        phase_layout: "uniform".into(),
        neutral_guard: false,
        audio_offset: 32768,
        audio_divider: 281,
        native_mux: true,
        cache_ticks: None,
        buffered: false,
        tick_budget: 490000,
        threads: None,
        upic_width: 320,
        wide_lz4_source: None,
        wide_raw_source: None,
        wide_skip_source: None,
        wide360_raw_source: None,
        wide_dict4_source: None,
        wide360_dict: false,
        streaming_dict4: false,
        dict4_full_height: false,
        dict4_rate_budget: None,
        dict4_prefetch: 12,
        source_frame_limit: None,
        lz4_library: None,
        lz4_coalesce: false,
    };
    while let Some(arg) = args.next() {
        if arg == "--help" || arg == "-h" {
            usage();
            return Ok(None);
        }
        match arg.as_str() {
            "--scale2" => o.scale = 2,
            "--scale4" => o.scale = 4,
            "--strict-budget" => o.strict_budget = true,
            "--neutral-guard" => o.neutral_guard = true,
            "--no-native-mux" => o.native_mux = false,
            "--fit-canvas" => o.fit_canvas = true,
            "--buffered" => o.buffered = true,
            "--wide360-dict" => o.wide360_dict = true,
            "--streaming-dict4" => o.streaming_dict4 = true,
            "--dict4-full-height" => o.dict4_full_height = true,
            "--lz4-coalesce" => o.lz4_coalesce = true,
            "--input"
            | "--output"
            | "--palette"
            | "--start"
            | "--duration"
            | "--fps"
            | "--audio-rate"
            | "--packet-budget"
            | "--luma-limit"
            | "--dither"
            | "--phase-layout"
            | "--audio-offset"
            | "--audio-divider"
            | "--phase-repeat"
            | "--rgb-luma-limit"
            | "--colour-mode"
            | "--frame-ticks"
            | "--align"
            | "--pixel-aspect"
            | "--tick-budget"
            | "--cache-ticks"
            | "--dither-contrast-weight"
            | "--threads"
            | "--upic-width"
            | "--wide-lz4-source"
            | "--wide-raw-source"
            | "--wide-skip-source"
            | "--wide360-raw-source"
            | "--wide-dict4-source"
            | "--dict4-rate-budget"
            | "--dict4-prefetch"
            | "--source-frame-limit"
            | "--lz4-library"
            | "--audio-gain-db" => {
                let v = args
                    .next()
                    .ok_or_else(|| format!("Missing value for {arg}"))?;
                match arg.as_str() {
                    "--input" => o.input = v.into(),
                    "--wide-lz4-source" => o.wide_lz4_source = Some(v.into()),
                    "--wide-raw-source" => o.wide_raw_source = Some(v.into()),
                    "--wide-skip-source" => o.wide_skip_source = Some(v.into()),
                    "--wide360-raw-source" => o.wide360_raw_source = Some(v.into()),
                    "--wide-dict4-source" => o.wide_dict4_source = Some(v.into()),
                    "--dict4-rate-budget" => {
                        o.dict4_rate_budget =
                            Some(v.parse().map_err(|_| "Invalid dictionary rate budget")?)
                    }
                    "--dict4-prefetch" => {
                        o.dict4_prefetch = v.parse().map_err(|_| "Invalid dictionary prefetch")?
                    }
                    "--source-frame-limit" => {
                        o.source_frame_limit =
                            Some(v.parse().map_err(|_| "Invalid source frame limit")?)
                    }
                    "--lz4-library" => o.lz4_library = Some(v.into()),
                    "--output" => o.output = v.into(),
                    "--palette" => o.palette = v.into(),
                    "--start" => o.start = v.parse().map_err(|_| "Invalid start")?,
                    "--duration" => o.duration = v.parse().map_err(|_| "Invalid duration")?,
                    "--fps" => {
                        o.fps = v.parse().map_err(|_| "Invalid fps")?;
                        fps_explicit = true;
                    }
                    "--colour-mode" => o.colour_mode = ColourMode::parse(&v)?,
                    "--frame-ticks" => {
                        o.frame_ticks = Some(v.parse().map_err(|_| "Invalid frame ticks")?)
                    }
                    "--align" => o.align = v,
                    "--pixel-aspect" => {
                        o.pixel_aspect = ratio(&v).ok_or("Invalid pixel aspect ratio")?
                    }
                    "--tick-budget" => {
                        tick_budget_explicit = true;
                        o.tick_budget = v.parse().map_err(|_| "Invalid tick budget")?
                    }
                    "--cache-ticks" => {
                        o.cache_ticks = Some(v.parse().map_err(|_| "Invalid cache ticks")?)
                    }
                    "--phase-repeat" => {
                        o.phase_repeat = v.parse().map_err(|_| "Invalid phase repeat")?
                    }
                    "--audio-rate" => o.audio_rate = v.parse().map_err(|_| "Invalid audio rate")?,
                    "--audio-gain-db" => {
                        o.audio_gain_db = v.parse().map_err(|_| "Invalid audio gain")?
                    }
                    "--packet-budget" => {
                        o.packet_budget = v.parse().map_err(|_| "Invalid packet budget")?
                    }
                    "--luma-limit" => o.luma_limit = v.parse().map_err(|_| "Invalid luma limit")?,
                    "--rgb-luma-limit" => {
                        o.rgb_luma_limit = Some(v.parse().map_err(|_| "Invalid RGB luma limit")?)
                    }
                    "--dither" => o.dither = v,
                    "--upic-width" => o.upic_width = v.parse().map_err(|_| "Invalid UPIC width")?,
                    "--threads" => {
                        o.threads = if v == "auto" {
                            None
                        } else {
                            let count = v
                                .parse::<usize>()
                                .map_err(|_| "threads must be auto or a positive integer")?;
                            if count == 0 {
                                return Err("threads must be auto or a positive integer".into());
                            }
                            Some(count)
                        }
                    }
                    "--dither-contrast-weight" => {
                        o.dither_contrast_weight =
                            v.parse().map_err(|_| "Invalid contrast weight")?
                    }
                    "--phase-layout" => {
                        o.phase_layout = v;
                        phase_layout_explicit = true;
                    }
                    "--audio-offset" => {
                        audio_offset_explicit = true;
                        o.audio_offset = if let Some(hex) = v.strip_prefix("0x") {
                            usize::from_str_radix(hex, 16).map_err(|_| "Invalid audio offset")?
                        } else {
                            v.parse().map_err(|_| "Invalid audio offset")?
                        }
                    }
                    "--audio-divider" => {
                        o.audio_divider = v.parse().map_err(|_| "Invalid audio divider")?
                    }
                    _ => unreachable!(),
                }
            }
            _ => return Err(format!("Unknown option {arg}")),
        }
    }
    if o.dict4_full_height && (!o.streaming_dict4 || o.pixel_aspect != 1.0) {
        return Err(
            "dict4-full-height requires streaming-dict4 and pixel-aspect1 for native SD pixels"
                .into(),
        );
    }
    if o.dict4_full_height && !tick_budget_explicit {
        o.tick_budget = 425000;
    }
    if o.dict4_full_height
        && (o.tick_budget > 436590 || o.dict4_rate_budget.is_some_and(|n| n > 436590))
    {
        return Err("Native SD codec9 budget must fit its110PALblanklines (436590slots)".into());
    }
    if o.streaming_dict4 && !o.wide360_dict {
        return Err("streaming-dict4 requires a fresh wide360-dict conversion".into());
    }
    if ![12, 13].contains(&o.dict4_prefetch)
        || o.dict4_rate_budget.is_some_and(|n| n == 0 || n > 476280)
        || ((o.dict4_rate_budget.is_some() || o.dict4_prefetch != 12)
            && !o.wide360_dict
            && o.wide_dict4_source.is_none())
    {
        return Err(
            "Dictionary rate/prefetch options require v8, prefetch12/13, and budget1..476280"
                .into(),
        );
    }
    if o.wide360_dict {
        if o.frame_ticks != Some(3) || ![320, 360].contains(&o.upic_width) {
            return Err("wide360-dict requires explicit --frame-ticks 3 and360 geometry".into());
        }
        if o.wide_lz4_source.is_some()
            || o.wide_raw_source.is_some()
            || o.wide_skip_source.is_some()
            || o.wide360_raw_source.is_some()
            || o.wide_dict4_source.is_some()
        {
            return Err("wide360-dict fresh conversion cannot combine with source reuse".into());
        }
        o.upic_width = 360;
    }
    if o.wide_lz4_source.is_some()
        || o.wide_raw_source.is_some()
        || o.wide_skip_source.is_some()
        || o.wide360_raw_source.is_some()
        || o.wide_dict4_source.is_some()
    {
        if [
            o.wide_lz4_source.is_some(),
            o.wide_raw_source.is_some(),
            o.wide_skip_source.is_some(),
            o.wide360_raw_source.is_some(),
            o.wide_dict4_source.is_some(),
        ]
        .into_iter()
        .filter(|v| *v)
        .count()
            != 1
        {
            return Err("Choose one wide-lz4/raw/skip/360-raw/dict4-source".into());
        }
        if o.wide_lz4_source.is_none() && (o.lz4_coalesce || o.lz4_library.is_some()) {
            return Err("lz4-coalesce/library require wide-lz4-source".into());
        }
        if o.output.as_os_str().is_empty() {
            return Err("--output is required".into());
        }
        let dict = o.wide_dict4_source.is_some();
        let small = o.wide360_raw_source.is_some() || dict;
        let reuse_ticks = if small {
            o.frame_ticks.unwrap_or(if dict { 3 } else { 4 })
        } else if o.wide_skip_source.is_some() {
            4
        } else {
            5
        };
        if dict && reuse_ticks != 3 {
            return Err("wide-dict4-source requires genuine PAL/3".into());
        }
        if small && ![3, 4].contains(&reuse_ticks) {
            return Err("wide360 reuse supports PAL/3 or PAL/4".into());
        }
        let cache_ticks = if dict {
            o.cache_ticks.unwrap_or(20)
        } else if small && reuse_ticks == 3 {
            16
        } else {
            20
        };
        if dict && ![16, 20].contains(&cache_ticks) {
            return Err("wide-dict4-source cache-ticks must be16 or20".into());
        }
        if o.frame_ticks.is_some_and(|n| n != reuse_ticks)
            || (fps_explicit && (o.fps / (pal_tick_hz() / reuse_ticks as f64) - 1.).abs() > 1e-6)
            || o.source_frame_limit == Some(0)
            || o.tick_budget == 0
            || o.buffered
            || o.scale != 1
        {
            return Err(format!("wide reuse requires PAL/{reuse_ticks}, scale1, positive frame limit/tick budget, and single-front transport"));
        }
        if o.cache_ticks.is_some_and(|n| n != cache_ticks) {
            return Err(format!("wide reuse requires cache-ticks{cache_ticks}"));
        }
        o.frame_ticks = Some(reuse_ticks);
        o.fps = pal_tick_hz() / reuse_ticks as f64;
        o.upic_width = if small { 360 } else { 384 };
        o.cache_ticks = Some(cache_ticks);
        if !audio_offset_explicit {
            o.audio_offset = if o.wide_lz4_source.is_none() {
                196608
            } else {
                131072
            };
        }
        if !(if dict {
            [196608, 225280].contains(&o.audio_offset)
        } else {
            [131072, 196608].contains(&o.audio_offset)
        }) || o.audio_divider != 281
        {
            return Err(
                "Wide reuse requires its supported audio offset and divider281 (v8:196608/225280)"
                    .into(),
            );
        }
        return Ok(Some(o));
    }
    if o.source_frame_limit.is_some() || o.lz4_library.is_some() || o.lz4_coalesce {
        return Err("source-frame-limit and lz4-library require wide-lz4-source".into());
    }
    if o.input.as_os_str().is_empty() || o.output.as_os_str().is_empty() {
        return Err("--input and --output are required".into());
    }
    if o.colour_mode != ColourMode::Temporal && !fps_explicit {
        o.fps = pal_tick_hz() / o.frame_ticks.unwrap_or(1) as f64;
    }
    if o.upic_width == 360 {
        let ticks = o.frame_ticks.unwrap_or(4);
        if ![3, 4].contains(&ticks) {
            return Err("upic-width360 supports frame-ticks3 or4".into());
        }
        o.frame_ticks = Some(ticks);
        if !fps_explicit {
            o.fps = pal_tick_hz() / ticks as f64;
        }
        let cache = if o.wide360_dict {
            o.cache_ticks.unwrap_or(20)
        } else if ticks == 3 {
            16
        } else {
            20
        };
        if o.wide360_dict && ![16, 20].contains(&cache) {
            return Err("wide360-dict cache-ticks must be16 or20".into());
        }
        if o.cache_ticks.is_some_and(|n| n != cache) {
            return Err(format!(
                "upic-width360 PAL/{ticks} requires cache-ticks{cache}"
            ));
        }
        o.cache_ticks = Some(cache);
        if !audio_offset_explicit {
            o.audio_offset = 196608;
        }
    }
    if o.cache_ticks.is_some() && o.upic_width != 360 {
        if o.cache_ticks != Some(20) || !(o.buffered || o.upic_width == 384) || !o.native_mux {
            return Err("cache-ticks currently supports20 only, with buffered native mux".into());
        }
        if !audio_offset_explicit {
            o.audio_offset = 131072;
        }
    }
    if o.colour_mode == ColourMode::Spatial {
        if phase_layout_explicit && o.phase_layout != "checker" {
            return Err("spatial colour mode requires checker phase-layout".into());
        }
        o.phase_layout = "checker".into();
    }
    if !o.start.is_finite()
        || o.start < 0.0
        || !o.duration.is_finite()
        || o.duration <= 0.0
        || !o.fps.is_finite()
        || o.fps <= 0.0
        || o.fps > pal_tick_hz()
    {
        return Err(format!(
            "Require finite start ≥ 0, duration > 0, and 0 < fps ≤ {}",
            pal_tick_hz()
        ));
    }
    if o.audio_rate < 8000 || o.audio_rate > 96000 || o.packet_budget < 4 {
        return Err("Invalid audio rate or packet budget".into());
    }
    audio_gain_multiplier(o.audio_gain_db)?;
    if !o.dither_contrast_weight.is_finite() || o.dither_contrast_weight < 0.0 {
        return Err("dither-contrast-weight must be finite and nonnegative".into());
    }
    let cache_block_bytes = if o.cache_ticks.is_some() {
        FIXED_CACHE_BLOCK_BYTES
    } else {
        CACHE_BLOCK_BYTES
    };
    if o.audio_offset < 8
        || o.audio_offset > cache_block_bytes - 4
        || !o.audio_offset.is_multiple_of(4)
    {
        return Err(format!(
            "audio-offset must be a multiple of four in8..{}",
            cache_block_bytes - 4
        ));
    }
    if ![0, 4, 8, 16, 32].contains(&o.luma_limit) {
        return Err("luma-limit must be 0,4,8,16,32".into());
    }
    if !(1..=8).contains(&o.phase_repeat) {
        return Err("phase-repeat must be an integer in 1..8".into());
    }
    if o.rgb_luma_limit
        .is_some_and(|limit| !limit.is_finite() || limit < 0.0)
    {
        return Err("rgb-luma-limit must be finite and nonnegative".into());
    }
    validate_cadence(&o)?;
    if !["start", "center", "end"].contains(&o.align.as_str()) {
        return Err("align must be start, center or end".into());
    }
    if !o.pixel_aspect.is_finite() || o.pixel_aspect <= 0.0 {
        return Err("pixel-aspect must be finite and positive".into());
    }
    if !o.fit_canvas && o.pixel_aspect != 1.0 {
        return Err("pixel-aspect requires fit-canvas".into());
    }
    if !["none", "jjn", "yliluoma", "yliluoma-author"].contains(&o.dither.as_str()) {
        return Err("dither must be none, jjn, yliluoma or yliluoma-author".into());
    }
    if ["yliluoma", "yliluoma-author"].contains(&o.dither.as_str())
        && o.colour_mode != ColourMode::Hardware
    {
        return Err("yliluoma and yliluoma-author dither require hardware colour mode".into());
    }
    if o.phase_layout != "uniform" && o.phase_layout != "checker" {
        return Err("phase-layout must be uniform or checker".into());
    }
    if o.tick_budget == 0 {
        return Err("tick-budget must be positive".into());
    }
    if o.buffered && (o.colour_mode != ColourMode::Hardware || o.scale != 1 || !o.fit_canvas) {
        return Err("buffered requires hardware colour mode, scale1 and fit-canvas".into());
    }
    if ![320, 360, 384].contains(&o.upic_width) {
        return Err("upic-width currently supports320,360 or384".into());
    }
    if [360, 384].contains(&o.upic_width)
        && (o.buffered
            || o.colour_mode != ColourMode::Hardware
            || o.scale != 1
            || !o.fit_canvas
            || o.dither != "yliluoma-author")
    {
        return Err(
            "upic-width360/384 requires single-front hardware, scale1, fit-canvas and yliluoma-author"
                .into(),
        );
    }
    Ok(Some(o))
}

fn ratio(text: &str) -> Option<f64> {
    let value = if let Some((numerator, denominator)) = text.split_once([':', '/']) {
        numerator.parse::<f64>().ok()? / denominator.parse::<f64>().ok()?
    } else {
        text.parse::<f64>().ok()?
    };
    (value.is_finite() && value > 0.0).then_some(value)
}

#[derive(Clone, Debug)]
struct InputAspect {
    width: usize,
    height: usize,
    sample_aspect: f64,
    display_aspect: f64,
}
fn parse_probe_aspect(text: &str) -> Result<InputAspect, String> {
    let fields = text
        .lines()
        .filter_map(|line| line.split_once('='))
        .collect::<HashMap<_, _>>();
    let dimension = |key| {
        fields
            .get(key)
            .and_then(|v| v.parse::<usize>().ok())
            .filter(|v| *v > 0)
            .ok_or_else(|| format!("ffprobe returned invalid {key}"))
    };
    let width = dimension("width")?;
    let height = dimension("height")?;
    let sample_aspect = fields
        .get("sample_aspect_ratio")
        .and_then(|v| ratio(v))
        .unwrap_or(1.0);
    let display_aspect = fields
        .get("display_aspect_ratio")
        .and_then(|v| ratio(v))
        .unwrap_or(width as f64 * sample_aspect / height as f64);
    Ok(InputAspect {
        width,
        height,
        sample_aspect,
        display_aspect,
    })
}
fn probe_aspect(input: &Path) -> Result<InputAspect, String> {
    let result = Command::new("ffprobe")
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=width,height,sample_aspect_ratio,display_aspect_ratio",
            "-of",
            "default=noprint_wrappers=1",
        ])
        .arg(input)
        .output()
        .map_err(|e| format!("Cannot run ffprobe for {}: {e}", input.display()))?;
    if !result.status.success() {
        return Err(format!(
            "ffprobe failed: {}",
            String::from_utf8_lossy(&result.stderr)
        ));
    }
    parse_probe_aspect(&String::from_utf8_lossy(&result.stdout))
}

#[derive(Clone, Debug)]
struct Geometry {
    width: usize,
    height: usize,
    left: usize,
    top: usize,
    quant_width: usize,
    quant_height: usize,
    source_aspect: Option<InputAspect>,
}
fn aligned_offset(space: usize, align: &str) -> Result<usize, String> {
    match align {
        "start" => Ok(0),
        "center" => Ok(space / 2),
        "end" => Ok(space),
        _ => Err("align must be start, center or end".into()),
    }
}
fn fitted_geometry(
    aspect: InputAspect,
    pixel_aspect: f64,
    align: &str,
    scale: usize,
) -> Result<Geometry, String> {
    fitted_geometry_canvas(aspect, pixel_aspect, align, scale, WIDTH, HEIGHT)
}
fn fitted_geometry_canvas(
    aspect: InputAspect,
    pixel_aspect: f64,
    align: &str,
    scale: usize,
    canvas_width: usize,
    canvas_height: usize,
) -> Result<Geometry, String> {
    if !pixel_aspect.is_finite() || pixel_aspect <= 0.0 || ![1, 2, 4].contains(&scale) {
        return Err("Invalid canvas pixel aspect or scale".into());
    }
    let raster_aspect = aspect.display_aspect / pixel_aspect;
    let (ideal_width, ideal_height) = if raster_aspect > canvas_width as f64 / canvas_height as f64
    {
        (canvas_width as f64, canvas_width as f64 / raster_aspect)
    } else {
        (canvas_height as f64 * raster_aspect, canvas_height as f64)
    };
    let quant_width =
        ((ideal_width / scale as f64).round() as usize).clamp(1, canvas_width / scale);
    let quant_height =
        ((ideal_height / scale as f64).round() as usize).clamp(1, canvas_height / scale);
    let width = quant_width * scale;
    let height = quant_height * scale;
    Ok(Geometry {
        width,
        height,
        left: aligned_offset(canvas_width - width, align)?,
        top: aligned_offset(canvas_height - height, align)?,
        quant_width,
        quant_height,
        source_aspect: Some(aspect),
    })
}
fn geometry(o: &Options) -> Result<Geometry, String> {
    if o.fit_canvas {
        fitted_geometry(probe_aspect(&o.input)?, o.pixel_aspect, &o.align, o.scale)
    } else {
        Ok(Geometry {
            width: 320,
            height: 180,
            left: 0,
            top: if o.scale == 4 { 8 } else { 10 },
            quant_width: 320 / o.scale,
            quant_height: 180 / o.scale,
            source_aspect: None,
        })
    }
}

fn read_palette(path: &Path) -> Result<[[u8; 3]; 16], String> {
    // Narrow parser for the repository's palette schema; reject incomplete data.
    let data = fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut colours = Vec::new();
    let mut rest = data.as_str();
    while let Some(at) = rest.find("\"rgb\"") {
        rest = &rest[at + 5..];
        let begin = rest.find('[').ok_or("Malformed rgb array")?;
        let end = rest[begin..].find(']').ok_or("Malformed rgb array")? + begin;
        let numbers: Result<Vec<u8>, _> = rest[begin + 1..end]
            .split(',')
            .map(|v| v.trim().parse::<u8>())
            .collect();
        let numbers = numbers.map_err(|_| "Malformed rgb component")?;
        if numbers.len() != 3 {
            return Err("rgb array must have three components".into());
        }
        colours.push([numbers[0], numbers[1], numbers[2]]);
        rest = &rest[end + 1..];
    }
    colours
        .try_into()
        .map_err(|v: Vec<_>| format!("Expected 16 RGB entries, found {}", v.len()))
}

fn linear(v: u8) -> f64 {
    let v = f64::from(v) / 255.0;
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}
fn srgb(v: f64) -> u8 {
    let v = if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    };
    (v * 255.0).round().clamp(0.0, 255.0) as u8
}
fn lab([r, g, b]: [f64; 3]) -> [f64; 3] {
    let l = (0.4122214708 * r + 0.5363325363 * g + 0.0514459929 * b).cbrt();
    let m = (0.2119034982 * r + 0.6806995451 * g + 0.1073969566 * b).cbrt();
    let s = (0.0883024619 * r + 0.2817188376 * g + 0.6299787005 * b).cbrt();
    [
        0.2104542553 * l + 0.7936177850 * m - 0.0040720468 * s,
        1.9779984951 * l - 2.4285922050 * m + 0.4505937099 * s,
        0.0259040371 * l + 0.7827717662 * m - 0.8086757660 * s,
    ]
}

struct Pair {
    a: u8,
    b: u8,
    lab: [f64; 3],
    linear: [f64; 3],
    rgb: [u8; 3],
    neutral: bool,
    luminance: [f64; 2],
    relative_luminance_delta: f64,
}
fn luminance([r, g, b]: [f64; 3]) -> f64 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}
fn pairs(palette: &[[u8; 3]; 16], limit: i32, rgb_limit: Option<f64>) -> Vec<Pair> {
    let colours = palette.map(|c| c.map(linear));
    let mut result = Vec::new();
    for a in 0..16 {
        for b in a..16 {
            if (PEPTO[a] - PEPTO[b]).abs() > limit {
                continue;
            }
            let luminance = [luminance(colours[a]), luminance(colours[b])];
            let mean = (luminance[0] + luminance[1]) * 0.5;
            let relative_luminance_delta = if mean == 0.0 {
                0.0
            } else {
                (luminance[0] - luminance[1]).abs() / mean
            };
            if a != b && rgb_limit.is_some_and(|limit| relative_luminance_delta > limit) {
                continue;
            }
            let mixed = std::array::from_fn(|i| (colours[a][i] + colours[b][i]) * 0.5);
            let neutral = [0, 1, 11, 12, 15].contains(&a) && [0, 1, 11, 12, 15].contains(&b);
            result.push(Pair {
                a: a as u8,
                b: b as u8,
                lab: lab(mixed),
                linear: mixed,
                rgb: mixed.map(srgb),
                neutral,
                luminance,
                relative_luminance_delta,
            });
        }
    }
    result
}
fn colour_pairs(palette: &[[u8; 3]; 16], o: &Options) -> Vec<Pair> {
    let mut result = pairs(palette, o.luma_limit, o.rgb_luma_limit);
    if o.colour_mode == ColourMode::Hardware {
        result.retain(|p| p.a == p.b);
    }
    result
}

// Inspired by Yliluoma's positional two-colour planning, rather than a port of
// its published colour metric. Linear-light sRGB means and squared OKLab error
// are combined with this empirical contrast penalty. A fixed physical Bayer
// coordinate selects a hardware colour; no neighbouring errors or time axis.
const YLILUOMA_CONTRAST_WEIGHT: f64 = 0.1;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct MixPlan {
    a: u8,
    b: u8,
    ratio: u8,
}
impl MixPlan {
    fn colour(self, x: usize, y: usize) -> u8 {
        if bayer8(x, y) < self.ratio {
            self.b
        } else {
            self.a
        }
    }
}
fn bayer8(x: usize, y: usize) -> u8 {
    let mut value = 0;
    for bit in 0..3 {
        value |= ((x >> bit) & 1) << (4 - bit * 2);
        value |= (((x ^ y) >> bit) & 1) << (5 - bit * 2);
    }
    value as u8
}
#[derive(Clone, Copy)]
struct MixCandidate {
    // Fourth-coordinate distance from query zero contributes the penalty.
    point: [f64; 4],
    plan: MixPlan,
    neutral: bool,
}
struct MixNode {
    candidate: usize,
    axis: usize,
    left: Option<usize>,
    right: Option<usize>,
}
struct MixTree {
    nodes: Vec<MixNode>,
    root: Option<usize>,
}
impl MixTree {
    fn new(candidates: &[MixCandidate], neutral_only: bool) -> Self {
        let mut indices = candidates
            .iter()
            .enumerate()
            .filter_map(|(i, c)| (!neutral_only || c.neutral).then_some(i))
            .collect::<Vec<_>>();
        let mut tree = Self {
            nodes: Vec::with_capacity(indices.len()),
            root: None,
        };
        tree.root = tree.build(candidates, &mut indices, 0);
        tree
    }
    fn build(
        &mut self,
        candidates: &[MixCandidate],
        indices: &mut [usize],
        depth: usize,
    ) -> Option<usize> {
        if indices.is_empty() {
            return None;
        }
        let axis = depth % 4;
        indices.sort_unstable_by(|a, b| {
            candidates[*a].point[axis]
                .total_cmp(&candidates[*b].point[axis])
                .then(a.cmp(b))
        });
        let middle = indices.len() / 2;
        let index = self.nodes.len();
        self.nodes.push(MixNode {
            candidate: indices[middle],
            axis,
            left: None,
            right: None,
        });
        let (left, rest) = indices.split_at_mut(middle);
        let right = &mut rest[1..];
        self.nodes[index].left = self.build(candidates, left, depth + 1);
        self.nodes[index].right = self.build(candidates, right, depth + 1);
        Some(index)
    }
    fn nearest(&self, candidates: &[MixCandidate], target: [f64; 4]) -> usize {
        let mut best = (f64::INFINITY, usize::MAX);
        self.search(candidates, target, self.root, &mut best);
        assert_ne!(best.1, usize::MAX);
        best.1
    }
    fn search(
        &self,
        candidates: &[MixCandidate],
        target: [f64; 4],
        index: Option<usize>,
        best: &mut (f64, usize),
    ) {
        let Some(index) = index else { return };
        let node = &self.nodes[index];
        let candidate = &candidates[node.candidate];
        let distance = point_distance(target, candidate.point);
        if distance < best.0 || (distance == best.0 && node.candidate < best.1) {
            *best = (distance, node.candidate);
        }
        let delta = target[node.axis] - candidate.point[node.axis];
        let (near, far) = if delta <= 0.0 {
            (node.left, node.right)
        } else {
            (node.right, node.left)
        };
        self.search(candidates, target, near, best);
        if delta * delta <= best.0 {
            self.search(candidates, target, far, best);
        }
    }
}
fn point_distance(a: [f64; 4], b: [f64; 4]) -> f64 {
    (0..4).map(|i| (a[i] - b[i]).powi(2)).sum()
}
struct PositionalDither {
    candidates: Vec<MixCandidate>,
    tree: MixTree,
    neutral_tree: MixTree,
    cache: HashMap<u32, u16>,
    neutral_guard: bool,
    contrast_weight: f64,
}
impl PositionalDither {
    fn new(palette: &[[u8; 3]; 16], neutral_guard: bool, contrast_weight: f64) -> Self {
        let linear_colours = palette.map(|p| p.map(linear));
        let labs = linear_colours.map(lab);
        let neutral = |index| [0, 1, 11, 12, 15].contains(&index);
        let mut candidates = Vec::with_capacity(16 + 120 * 63);
        for (a, colour_lab) in labs.iter().enumerate() {
            candidates.push(MixCandidate {
                point: [colour_lab[0], colour_lab[1], colour_lab[2], 0.0],
                plan: MixPlan {
                    a: a as u8,
                    b: a as u8,
                    ratio: 0,
                },
                neutral: neutral(a),
            });
        }
        for first in 0..16 {
            for second in first + 1..16 {
                // Fixed luminance ordering gives every pair the same spatial
                // orientation across all pixels and all frames.
                let (a, b) =
                    if luminance(linear_colours[first]) <= luminance(linear_colours[second]) {
                        (first, second)
                    } else {
                        (second, first)
                    };
                let contrast: f64 = (0..3).map(|i| (labs[a][i] - labs[b][i]).powi(2)).sum();
                for ratio in 1..64 {
                    let fraction = f64::from(ratio) / 64.0;
                    let mean = std::array::from_fn(|i| {
                        linear_colours[a][i] * (1.0 - fraction) + linear_colours[b][i] * fraction
                    });
                    let mixed = lab(mean);
                    let penalty = contrast_weight * fraction * (1.0 - fraction) * contrast;
                    candidates.push(MixCandidate {
                        point: [mixed[0], mixed[1], mixed[2], penalty.sqrt()],
                        plan: MixPlan {
                            a: a as u8,
                            b: b as u8,
                            ratio,
                        },
                        neutral: neutral(a) && neutral(b),
                    });
                }
            }
        }
        Self {
            tree: MixTree::new(&candidates, false),
            neutral_tree: MixTree::new(&candidates, true),
            candidates,
            cache: HashMap::new(),
            neutral_guard,
            contrast_weight,
        }
    }
    fn plan(&mut self, rgb: [u8; 3]) -> usize {
        let key = u32::from(rgb[0]) << 16 | u32::from(rgb[1]) << 8 | u32::from(rgb[2]);
        if let Some(index) = self.cache.get(&key) {
            return usize::from(*index);
        }
        let target = lab(rgb.map(linear));
        let neutral =
            self.neutral_guard && rgb.iter().max().unwrap() - rgb.iter().min().unwrap() <= 8;
        let tree = if neutral {
            &self.neutral_tree
        } else {
            &self.tree
        };
        let index = tree.nearest(&self.candidates, [target[0], target[1], target[2], 0.0]);
        self.cache.insert(key, index as u16);
        index
    }
    fn frame_plans(&mut self, frame: &[u8]) -> Vec<usize> {
        frame
            .chunks_exact(3)
            .map(|p| self.plan([p[0], p[1], p[2]]))
            .collect()
    }
}
fn nearest_linear(rgb: [f64; 3], pairs: &[Pair], neutral: bool) -> u8 {
    let target = lab(rgb);
    let mut best = 0;
    let mut distance = f64::INFINITY;
    for (i, pair) in pairs.iter().enumerate() {
        if neutral && !pair.neutral {
            continue;
        }
        let d: f64 = (0..3)
            .map(|j| (target[j] - pair.lab[j]).powi(2))
            .sum::<f64>()
            + if pair.a == pair.b { 0.0 } else { 1e-10 };
        if d < distance {
            best = i as u8;
            distance = d;
        }
    }
    best
}
fn nearest(rgb: [u8; 3], pairs: &[Pair], neutral_guard: bool) -> u8 {
    let neutral = neutral_guard && rgb.iter().max().unwrap() - rgb.iter().min().unwrap() <= 8;
    nearest_linear(rgb.map(linear), pairs, neutral)
}
fn quantise(
    frame: &[u8],
    width: usize,
    height: usize,
    pairs: &[Pair],
    dither: &str,
    neutral_guard: bool,
    cache: &mut HashMap<u32, u8>,
) -> Vec<u8> {
    if dither == "none" {
        return frame
            .chunks_exact(3)
            .map(|pixel| {
                let key =
                    (u32::from(pixel[0]) << 16) | (u32::from(pixel[1]) << 8) | u32::from(pixel[2]);
                *cache.entry(key).or_insert_with(|| {
                    nearest([pixel[0], pixel[1], pixel[2]], pairs, neutral_guard)
                })
            })
            .collect();
    }
    let mut work: Vec<[f64; 3]> = frame
        .chunks_exact(3)
        .map(|p| [linear(p[0]), linear(p[1]), linear(p[2])])
        .collect();
    let mut indices = vec![0; width * height];
    const JJN: [(isize, usize, f64); 12] = [
        (1, 0, 7.0),
        (2, 0, 5.0),
        (-2, 1, 3.0),
        (-1, 1, 5.0),
        (0, 1, 7.0),
        (1, 1, 5.0),
        (2, 1, 3.0),
        (-2, 2, 1.0),
        (-1, 2, 3.0),
        (0, 2, 5.0),
        (1, 2, 3.0),
        (2, 2, 1.0),
    ];
    for y in 0..height {
        let direction: isize = if y & 1 == 0 { 1 } else { -1 };
        for step in 0..width {
            let x = if direction == 1 {
                step
            } else {
                width - 1 - step
            };
            let at = y * width + x;
            let corrected = work[at].map(|v| v.clamp(0.0, 1.0));
            let source = &frame[at * 3..at * 3 + 3];
            let neutral =
                neutral_guard && source.iter().max().unwrap() - source.iter().min().unwrap() <= 8;
            let index = nearest_linear(corrected, pairs, neutral);
            indices[at] = index;
            let error: [f64; 3] =
                std::array::from_fn(|c| corrected[c] - pairs[index as usize].linear[c]);
            for (dx, dy, weight) in JJN {
                let nx = x as isize + dx * direction;
                let ny = y + dy;
                if nx < 0 || nx >= width as isize || ny >= height {
                    continue;
                }
                let next = ny * width + nx as usize;
                for c in 0..3 {
                    work[next][c] += error[c] * weight / 48.0;
                }
            }
        }
    }
    indices
}

fn pack(indices: &[u8]) -> Vec<u8> {
    assert_eq!(indices.len(), WIDTH * HEIGHT);
    let mut result = Vec::with_capacity(FRAME_BYTES);
    for (top, bottom) in [(0, 64), (64, 128), (128, 192), (192, 200)] {
        for column in 0..WIDTH / 2 {
            for row in top..bottom {
                result.push(
                    indices[row * WIDTH + column * 2] | indices[row * WIDTH + column * 2 + 1] << 4,
                );
            }
        }
    }
    result
}

#[derive(Default, Clone, Debug)]
struct PacketStats {
    copied: usize,
    filled: usize,
    alternating: usize,
    expanded2: usize,
    expanded4: usize,
    doubled4: usize,
    doubled_runs: usize,
    checker2: usize,
    checker_runs: usize,
    flip2: usize,
    flip2_runs: usize,
    flip_pages: usize,
    flip_page_runs: usize,
    runs: usize,
    changed: usize,
}
impl PacketStats {
    fn estimated_cycles(&self) -> usize {
        // Conservative inner-loop model; native player counters are authoritative.
        self.copied * 14
            + self.filled * 9
            + self.alternating * 12
            + self.expanded2 * 12
            + self.expanded4 * 8
            + self.doubled4 * 7
            + self.doubled_runs * 70
            + self.checker2 * 35 / 2
            + self.checker_runs * 90
            + self.flip2 / 2 * FLIP2_PAIR_CYCLES
            + self.flip_pages * FLIP_PAGE_CYCLES
            + self.flip2_runs * (FLIP2_HEADER_CYCLES - 90)
            + self.flip_page_runs * (FLIP_PAGE_HEADER_CYCLES - 90)
            + self.runs * 90
            + 80
    }
    fn writes(&self) -> usize {
        self.copied
            + self.filled
            + self.alternating
            + self.expanded2
            + self.expanded4
            + self.doubled4
            + self.checker2
            + self.flip_bytes()
    }
    fn flip_bytes(&self) -> usize {
        self.flip2 + self.flip_pages * 256
    }
    fn flip_runs(&self) -> usize {
        self.flip2_runs + self.flip_page_runs
    }
}
fn pattern_length(data: &[u8], at: usize, period: usize) -> usize {
    let limit = (data.len() - at).min(255);
    let mut length = period.min(limit);
    while length < limit && data[at + length] == data[at + length % period] {
        length += 1;
    }
    length
}
fn expanded_length(data: &[u8], at: usize, repeat: usize) -> usize {
    let unit = repeat * 2;
    let limit = (data.len() - at).min(255) / unit * unit;
    let mut length = 0;
    while length < limit {
        let first = data[at + length];
        let second = data[at + length + repeat];
        if first & 15 != first >> 4 || second & 15 != second >> 4 {
            break;
        }
        if !(0..repeat)
            .all(|i| data[at + length + i] == first && data[at + length + repeat + i] == second)
        {
            break;
        }
        length += unit;
    }
    length
}
fn checker_length(data: &[u8], at: usize) -> usize {
    let limit = (data.len() - at).min(254) / 2 * 2;
    let mut length = 0;
    while length < limit && data[at + length + 1] == data[at + length].rotate_left(4) {
        length += 2;
    }
    length
}
fn flip_lengths(before: &[u8], after: &[u8]) -> Vec<usize> {
    assert_eq!(before.len(), after.len());
    let mut lengths = vec![0; before.len() + 2];
    // Suffix lengths let page runs cross unchanged symmetric pairs without
    // repeatedly scanning the image for every DP candidate.
    for at in (0..before.len().saturating_sub(1)).rev() {
        if before[at + 1] == before[at].rotate_left(4)
            && after[at] == before[at + 1]
            && after[at + 1] == before[at]
        {
            lengths[at] = 2 + lengths[at + 2];
        }
    }
    lengths
}
fn encode_packet(before: &[u8], after: &[u8]) -> (Vec<u8>, PacketStats) {
    assert_eq!(before.len(), FRAME_BYTES);
    assert_eq!(after.len(), FRAME_BYTES);
    if before == after {
        return (vec![0; 4], PacketStats::default());
    }
    let mut result = Vec::new();
    let mut stats = PacketStats {
        changed: before.iter().zip(after).filter(|(a, b)| a != b).count(),
        ..Default::default()
    };
    for (start, length, address) in REGIONS {
        let before = &before[start..start + length];
        let after = &after[start..start + length];
        let flips = flip_lengths(before, after);
        let mut costs = vec![usize::MAX; length + 1];
        let mut choices = vec![(0usize, 255u8); length];
        costs[length] = 0;
        // Cost is eight times (63*DMA bytes + estimated CPU cycles),
        // so every command is compared on its expected blank consumption.
        // The offline encoder can afford this search; the player remains tiny.
        for at in (0..length).rev() {
            let unchanged = before[at] == after[at];
            let mut best = if unchanged { costs[at + 1] } else { usize::MAX };
            let mut selected = if unchanged { (1, 255) } else { (0, 255) };
            let mut consider = |count: usize, tag: u8, header: usize, per_byte: usize| {
                let cost = header + per_byte * count + costs[at + count];
                if cost < best {
                    best = cost;
                    selected = (count, tag);
                }
            };
            if !unchanged {
                for count in 1..=(length - at).min(255) {
                    consider(count, 0, 2736, 616);
                }
                for count in 1..=pattern_length(after, at, 1) {
                    consider(count, 1, 3240, 72);
                }
                for count in 2..=pattern_length(after, at, 2) {
                    consider(count, 2, 3744, 96);
                }
                for count in (4..=expanded_length(after, at, 2)).step_by(4) {
                    consider(count, 3, 2736, 222);
                }
                for count in (8..=expanded_length(after, at, 4)).step_by(8) {
                    consider(count, 4, 2736, 127);
                }
                for count in (2..=checker_length(after, at)).step_by(2) {
                    // One encoded byte per pair, 35 CPU cycles per pair,
                    // and 180 cycles for complete command processing.
                    consider(count, 7, 3456, 392);
                }
            }
            // In-place commands are valid only for exact AB swaps. They may
            // include unchanged pairs/pages when that saves command overhead.
            let physical = address + at;
            if physical.is_multiple_of(2) {
                for count in (2..=flips[at].min(254)).step_by(2) {
                    consider(
                        count,
                        8,
                        (63 * 4 + FLIP2_HEADER_CYCLES) * 8,
                        FLIP2_PAIR_CYCLES * 4,
                    );
                }
            }
            if physical.is_multiple_of(256) {
                for pages in 1..=(flips[at] / 256).min(255) {
                    // A page has 128 pairs. Keeping the per-page setup in the
                    // page cost also accounts for runs longer than one page.
                    consider(
                        pages * 256,
                        9,
                        (63 * 4 + FLIP_PAGE_HEADER_CYCLES + pages * FLIP_PAGE_CYCLES) * 8,
                        0,
                    );
                }
            }
            let compact = start + at;
            let (stride, tag) = if compact < 30720 { (64, 5) } else { (8, 6) };
            if compact % (stride * 2) == 0
                && at + stride * 2 <= length
                && expanded_length(after, at, 4) >= stride
                && after[at..at + stride] == after[at + stride..at + stride * 2]
            {
                // Four-byte command header plus one packed byte per eight rows.
                // Two adjacent columns are written using the same colour lookup.
                consider(stride * 2, tag, (63 * (4 + stride / 8) + 160) * 8, 7 * 8);
            }
            costs[at] = best;
            choices[at] = selected;
        }
        let mut at = 0;
        while at < length {
            let (count, tag) = choices[at];
            if tag == 255 {
                at += count;
                continue;
            }
            result.extend_from_slice(&((address + at - 1) as u16).to_le_bytes());
            let command_count = match tag {
                5 | 6 => count / 2,
                9 => count / 256,
                _ => count,
            };
            result.extend_from_slice(&[command_count as u8, tag]);
            match tag {
                0 => {
                    result.extend_from_slice(&after[at..at + count]);
                    stats.copied += count;
                }
                1 => {
                    result.push(after[at]);
                    stats.filled += count;
                }
                2 => {
                    result.extend_from_slice(&after[at..at + 2]);
                    stats.alternating += count;
                }
                3 | 4 => {
                    let repeat = if tag == 3 { 2 } else { 4 };
                    for pos in (at..at + count).step_by(repeat * 2) {
                        result.push((after[pos] & 15) | (after[pos + repeat] & 15) << 4);
                    }
                    if tag == 3 {
                        stats.expanded2 += count;
                    } else {
                        stats.expanded4 += count;
                    }
                }
                5 | 6 => {
                    for pos in (at..at + command_count).step_by(8) {
                        result.push((after[pos] & 15) | (after[pos + 4] & 15) << 4);
                    }
                    stats.doubled4 += count;
                    stats.doubled_runs += 1;
                }
                7 => {
                    for pos in (at..at + count).step_by(2) {
                        result.push(after[pos]);
                    }
                    stats.checker2 += count;
                    stats.checker_runs += 1;
                }
                8 => {
                    stats.flip2 += count;
                    stats.flip2_runs += 1;
                }
                9 => {
                    stats.flip_pages += command_count;
                    stats.flip_page_runs += 1;
                }
                _ => unreachable!(),
            }
            stats.runs += 1;
            at += count;
        }
    }
    result.extend_from_slice(&[0; 4]);
    (result, stats)
}

fn decode_packet(frame: &mut [u8], packet: &[u8]) -> Result<(), String> {
    if frame.len() != FRAME_BYTES {
        return Err("Invalid frame length".into());
    }
    let mut at = 0;
    loop {
        let header = packet
            .get(at..at + 4)
            .ok_or("Truncated header or missing terminator")?;
        at += 4;
        let destination = usize::from(u16::from_le_bytes([header[0], header[1]])) + 1;
        let count = usize::from(header[2]);
        let tag = header[3];
        if count == 0 {
            if header != [0, 0, 0, 0] || at != packet.len() {
                return Err("Invalid terminator or trailing data".into());
            }
            return Ok(());
        }
        let output_count = match tag {
            5 | 6 => count * 2,
            9 => count * 256,
            _ => count,
        };
        let (_, _, address) = REGIONS
            .iter()
            .find(|(_, length, address)| {
                destination >= *address && destination + output_count <= *address + *length
            })
            .ok_or("Destination outside framebuffer")?;
        let region = REGIONS.iter().find(|(_, _, a)| a == address).unwrap();
        let offset = region.0 + destination - address;
        let bytes = match tag {
            0 => count,
            1 => 1,
            2 => 2,
            3 if count % 4 == 0 => count / 4,
            4 if count % 8 == 0 => count / 8,
            5 if count == 64 && offset < 30720 && offset % 128 == 0 => count / 8,
            6 if count == 8 && offset >= 30720 && offset % 16 == 0 => count / 8,
            7 if count % 2 == 0 => count / 2,
            8 if count % 2 == 0 && destination % 2 == 0 => 0,
            9 if destination % 256 == 0 => 0,
            _ => return Err("Unknown command tag".into()),
        };
        let data = packet.get(at..at + bytes).ok_or("Truncated command data")?;
        at += bytes;
        if tag == 8 || tag == 9 {
            let input = &mut frame[offset..offset + output_count];
            // Validate the entire command before mutation, including pairs the
            // encoder coalesced although their two nibbles are identical.
            if !input
                .chunks_exact(2)
                .all(|pair| pair[1] == pair[0].rotate_left(4))
            {
                return Err("Invalid flip command input pair".into());
            }
            for value in input {
                *value = value.rotate_left(4);
            }
            continue;
        }
        if tag == 5 || tag == 6 {
            for i in 0..count {
                let value = data[i / 8];
                let colour = if i % 8 < 4 { value & 15 } else { value >> 4 };
                frame[offset + i] = colour | colour << 4;
                frame[offset + count + i] = colour | colour << 4;
            }
            continue;
        }
        if tag == 7 {
            for i in 0..count / 2 {
                frame[offset + i * 2] = data[i];
                frame[offset + i * 2 + 1] = data[i].rotate_left(4);
            }
            continue;
        }
        for i in 0..count {
            frame[offset + i] = if tag >= 3 {
                let repeat = if tag == 3 { 2 } else { 4 };
                let value = data[i / (repeat * 2)];
                let colour = if i % (repeat * 2) < repeat {
                    value & 15
                } else {
                    value >> 4
                };
                colour | colour << 4
            } else {
                data[if tag == 0 {
                    i
                } else if tag == 1 {
                    0
                } else {
                    i & 1
                }]
            };
        }
    }
}

const CACHE_BLOCK_BYTES: usize = 65536;
const FIXED_CACHE_BLOCK_BYTES: usize = 262144;
const PAL_PHI2_HZ: u64 = 985248;
const PAL_FRAME_PHI2: u64 = 312 * 63;
const SAMPLER_CLOCK_HZ: u64 = 6250000;

fn pal_tick_hz() -> f64 {
    PAL_PHI2_HZ as f64 / PAL_FRAME_PHI2 as f64
}
fn validate_cadence(o: &Options) -> Result<(), String> {
    if o.colour_mode == ColourMode::Temporal {
        if o.frame_ticks.is_some() {
            return Err(
                "frame-ticks is for hardware or spatial colour mode; temporal uses phase-repeat"
                    .into(),
            );
        }
        return validate_native_phase_rate(o.fps, o.phase_repeat, o.native_mux);
    }
    let ticks = o.frame_ticks.unwrap_or(1);
    if !(1..=255).contains(&ticks) {
        return Err("frame-ticks must be an integer in 1..255".into());
    }
    if o.phase_repeat != 1 {
        return Err("hardware/spatial colour mode uses frame-ticks, not phase-repeat".into());
    }
    let expected_fps = pal_tick_hz() / ticks as f64;
    if ((o.fps - expected_fps) / expected_fps).abs() > 1e-6 {
        return Err(format!(
            "{} colour mode with {ticks} frame-ticks requires fps near {expected_fps}",
            o.colour_mode.name()
        ));
    }
    Ok(())
}
fn ticks_per_content_frame(o: &Options) -> usize {
    if o.colour_mode == ColourMode::Temporal {
        2 * o.phase_repeat
    } else {
        o.frame_ticks.unwrap_or(1)
    }
}
fn display_hz(o: &Options) -> f64 {
    if o.colour_mode == ColourMode::Temporal {
        phase_fps(o.fps, o.phase_repeat)
    } else {
        o.fps * ticks_per_content_frame(o) as f64
    }
}
fn phase_fps(source_fps: f64, repeat: usize) -> f64 {
    source_fps * 2.0 * repeat as f64
}
fn validate_native_phase_rate(
    source_fps: f64,
    repeat: usize,
    native_mux: bool,
) -> Result<(), String> {
    let pal_phase_hz = PAL_PHI2_HZ as f64 / PAL_FRAME_PHI2 as f64;
    if native_mux
        && repeat > 1
        && ((phase_fps(source_fps, repeat) - pal_phase_hz) / pal_phase_hz).abs() > 1e-6
    {
        return Err(format!("Native PAL mux with phase-repeat {repeat} requires fps near {}; use --no-native-mux for another phase rate",pal_phase_hz/(2.0*repeat as f64)));
    }
    Ok(())
}
fn append_ab<T: Clone>(phases: &mut Vec<T>, a: &T, b: &T, repeat: usize) {
    for _ in 0..repeat {
        phases.push(a.clone());
        phases.push(b.clone());
    }
}
fn append_held<T: Clone>(frames: &mut Vec<T>, frame: &T, ticks: usize) {
    frames.extend(std::iter::repeat_n(frame.clone(), ticks));
}
fn display_order(o: &Options) -> String {
    if o.colour_mode == ColourMode::Temporal {
        return phase_order(o.phase_repeat);
    }
    let mut labels = Vec::with_capacity(ticks_per_content_frame(o) * 2);
    for frame in 0..2 {
        labels.extend(std::iter::repeat_n(
            format!("F{frame}"),
            ticks_per_content_frame(o),
        ));
    }
    format!("{},...", labels.join(","))
}
fn phase_order(repeat: usize) -> String {
    let mut labels = Vec::with_capacity(repeat * 4);
    for frame in 0..2 {
        for _ in 0..repeat {
            labels.push(format!("A{frame}"));
            labels.push(format!("B{frame}"));
        }
    }
    format!("{},...", labels.join(","))
}

fn sample_at_tick(tick: usize, divider: u16) -> usize {
    // Exact rational arithmetic with ties-to-even rounding, matching the
    // cumulative Python prototype without accumulating per-block drift.
    let numerator = tick as u128 * u128::from(SAMPLER_CLOCK_HZ) * u128::from(PAL_FRAME_PHI2);
    let denominator = (u128::from(divider) + 2) * u128::from(PAL_PHI2_HZ);
    let whole = numerator / denominator;
    let remainder = numerator % denominator;
    (whole
        + u128::from(
            remainder * 2 > denominator
                || (remainder * 2 == denominator && !whole.is_multiple_of(2)),
        )) as usize
}

fn records_from_raw(raw: &[u8]) -> Result<Vec<Vec<u8>>, String> {
    if !raw.len().is_multiple_of(CACHE_BLOCK_BYTES) {
        return Err("Raw video stream must contain whole 64 KiB blocks".into());
    }
    let mut records = Vec::new();
    let mut at = 0;
    while at < raw.len() {
        let header = raw.get(at..at + 2).ok_or("Truncated video record length")?;
        let size = usize::from(u16::from_le_bytes([header[0], header[1]]));
        if size == 0 {
            at = (at / CACHE_BLOCK_BYTES + 1) * CACHE_BLOCK_BYTES;
            continue;
        }
        let end = at + size + 2;
        // Buffered v2's empty held tick is flags + zero chunk length (3 bytes).
        if size < 2 || end > raw.len() || at / CACHE_BLOCK_BYTES != (end - 1) / CACHE_BLOCK_BYTES {
            return Err(
                "Invalid video record length or record crossing a raw block boundary".into(),
            );
        }
        records.push(raw[at..end].to_vec());
        at = end;
    }
    Ok(records)
}

struct CacheMux {
    file: Vec<u8>,
    block_records: Vec<usize>,
    audio_lengths: Vec<usize>,
    record_offsets: Vec<usize>,
    useful_video_bytes: usize,
    padded_audio_bytes: usize,
    block_bytes: usize,
    fixed_ticks_per_block: Option<usize>,
}
impl CacheMux {
    fn metadata(&self, audio_offset: usize, divider: u16, source_audio_hz: u32) -> String {
        self.metadata_file_bytes(audio_offset, divider, source_audio_hz, self.file.len())
    }
    fn metadata_file_bytes(
        &self,
        audio_offset: usize,
        divider: u16,
        source_audio_hz: u32,
        file_bytes: usize,
    ) -> String {
        let list = |values: &[usize]| {
            format!(
                "[{}]",
                values
                    .iter()
                    .map(usize::to_string)
                    .collect::<Vec<_>>()
                    .join(",")
            )
        };
        let sample_hz = SAMPLER_CLOCK_HZ as f64 / (f64::from(divider) + 2.0);
        let phase_hz = PAL_PHI2_HZ as f64 / PAL_FRAME_PHI2 as f64;
        let useful_audio_bytes: usize = self.audio_lengths.iter().sum();
        let block_bytes = self.block_bytes;
        let cache_bytes = block_bytes * 2;
        let extra=self.fixed_ticks_per_block.map_or_else(String::new,|ticks|format!(",\n  \"cache_slot_bytes\":{block_bytes},\n  \"fixed_ticks_per_block\":{ticks},\n  \"audio_timestamp_model\":{{\"phase_origin\":\"PAL tick0\",\"slice_boundary\":\"cumulative round-to-even tick*actual_sampler_hz/PAL_phase_hz, stereo4-byte aligned\",\"full_block_seconds\":{},\"sampler_clock_hz\":6250000,\"sampler_divider_addend\":2,\"pcm_resampled\":false,\"timing_note\":\"Fixed hand-off cadence; emulator/hardware sampler start quantization is measured separately\"}}",ticks as f64/phase_hz));
        format!("{{\n  \"format\":\"MUpic native cache v0\",\n  \"file\":\"stream.mup\",\n  \"block_records\":{},\n  \"audio_lengths\":{},\n  \"record_offsets\":{},\n  \"sample_hz\":{sample_hz},\n  \"phase_hz\":{phase_hz},\n  \"duration_seconds\":{},\n  \"block_bytes\":{block_bytes},\n  \"cache_bytes\":{cache_bytes},\n  \"audio_offset\":{audio_offset},\n  \"audio_divider\":{divider},\n  \"source_audio_hz\":{source_audio_hz},\n  \"file_bytes\":{},\n  \"useful_video_bytes\":{},\n  \"useful_audio_bytes\":{useful_audio_bytes},\n  \"padded_audio_bytes\":{},\n  \"padding_bytes\":{},\n  \"streaming_cache_wrap_required\":{}{extra}\n}}\n",
        list(&self.block_records),list(&self.audio_lengths),list(&self.record_offsets),self.record_offsets.len() as f64/phase_hz,file_bytes,self.useful_video_bytes,self.padded_audio_bytes,file_bytes-self.useful_video_bytes-useful_audio_bytes,self.block_records.len()>2)
    }
}
fn mux_cache_blocks(
    raw: &[u8],
    pcm: &[u8],
    expected_records: usize,
    divider: u16,
    audio_offset: usize,
) -> Result<CacheMux, String> {
    if !pcm.len().is_multiple_of(4) {
        return Err("PCM must be aligned to stereo S16LE samples".into());
    }
    if !(8..=65532).contains(&audio_offset) || !audio_offset.is_multiple_of(4) {
        return Err("Invalid native audio offset".into());
    }
    let packets = records_from_raw(raw)?;
    if packets.len() != expected_records || packets.is_empty() {
        return Err("Native mux video record count mismatch".into());
    }
    let mut mux = CacheMux {
        file: Vec::new(),
        block_records: Vec::new(),
        audio_lengths: Vec::new(),
        record_offsets: Vec::new(),
        useful_video_bytes: 0,
        padded_audio_bytes: 0,
        block_bytes: CACHE_BLOCK_BYTES,
        fixed_ticks_per_block: None,
    };
    let mut record = 0;
    let mut sample = 0;
    while record < packets.len() {
        if mux.block_records.len() == 255 {
            return Err("Native cache format supports at most 255 blocks; shorten the clip or use --no-native-mux".into());
        }
        let mut block = vec![0; CACHE_BLOCK_BYTES];
        let mut cursor = 0;
        let mut count = 0;
        while record + count < packets.len() {
            let packet = &packets[record + count];
            let end_sample = sample_at_tick(record + count + 1, divider);
            if cursor + packet.len() + 2 > audio_offset
                || (end_sample - sample) * 4 > CACHE_BLOCK_BYTES - audio_offset
            {
                break;
            }
            mux.record_offsets.push(mux.file.len() + cursor);
            block[cursor..cursor + packet.len()].copy_from_slice(packet);
            cursor += packet.len();
            count += 1;
            mux.useful_video_bytes += packet.len();
        }
        if count == 0 {
            return Err(format!("Video record {record} cannot fit together with one PAL tick of PCM in a native cache block"));
        }
        let end_sample = sample_at_tick(record + count, divider);
        let start_byte = sample * 4;
        let end_byte = end_sample * 4;
        let audio_bytes = end_byte - start_byte;
        let available_end = end_byte.min(pcm.len());
        let available = available_end.saturating_sub(start_byte);
        if available > 0 {
            block[audio_offset..audio_offset + available]
                .copy_from_slice(&pcm[start_byte..available_end]);
        }
        mux.padded_audio_bytes += audio_bytes - available;
        mux.audio_lengths.push(audio_bytes);
        mux.block_records.push(count);
        mux.file.extend(block);
        sample = end_sample;
        record += count;
    }
    Ok(mux)
}

fn mux_fixed_tick_blocks(
    raw: &[u8],
    pcm: &[u8],
    expected_records: usize,
    divider: u16,
    audio_offset: usize,
    ticks_per_block: usize,
) -> Result<CacheMux, String> {
    if !pcm.len().is_multiple_of(4) {
        return Err("PCM must be aligned to stereo S16LE samples".into());
    }
    if !(8..=FIXED_CACHE_BLOCK_BYTES - 4).contains(&audio_offset)
        || !audio_offset.is_multiple_of(4)
        || ticks_per_block == 0
    {
        return Err("Invalid fixed-tick cache audio offset or tick count".into());
    }
    let packets = records_from_raw(raw)?;
    if packets.len() != expected_records || packets.is_empty() {
        return Err("Fixed-tick mux video record count mismatch".into());
    }
    let mut mux = CacheMux {
        file: Vec::new(),
        block_records: Vec::new(),
        audio_lengths: Vec::new(),
        record_offsets: Vec::new(),
        useful_video_bytes: 0,
        padded_audio_bytes: 0,
        block_bytes: FIXED_CACHE_BLOCK_BYTES,
        fixed_ticks_per_block: Some(ticks_per_block),
    };
    let mut record = 0;
    while record < packets.len() {
        if mux.block_records.len() == 255 {
            return Err("Fixed-tick cache format supports at most255 blocks".into());
        }
        let count = ticks_per_block.min(packets.len() - record);
        let video_bytes = packets[record..record + count]
            .iter()
            .map(Vec::len)
            .sum::<usize>();
        if video_bytes + 2 > audio_offset {
            return Err(format!("Fixed-tick block at record{record}: {video_bytes} video bytes plus zero marker exceed audio offset{audio_offset}"));
        }
        let start_byte = sample_at_tick(record, divider) * 4;
        let end_byte = sample_at_tick(record + count, divider) * 4;
        let audio_bytes = end_byte - start_byte;
        if audio_bytes > FIXED_CACHE_BLOCK_BYTES - audio_offset {
            return Err(format!(
                "Fixed-tick block at record{record}: {audio_bytes} PCM bytes exceed audio area"
            ));
        }
        let mut block = vec![0; FIXED_CACHE_BLOCK_BYTES];
        let mut cursor = 0;
        for packet in &packets[record..record + count] {
            mux.record_offsets.push(mux.file.len() + cursor);
            block[cursor..cursor + packet.len()].copy_from_slice(packet);
            cursor += packet.len();
        }
        let available_end = end_byte.min(pcm.len());
        let available = available_end.saturating_sub(start_byte);
        if available > 0 {
            block[audio_offset..audio_offset + available]
                .copy_from_slice(&pcm[start_byte..available_end]);
        }
        mux.padded_audio_bytes += audio_bytes - available;
        mux.audio_lengths.push(audio_bytes);
        mux.block_records.push(count);
        mux.useful_video_bytes += video_bytes;
        mux.file.extend(block);
        record += count;
    }
    Ok(mux)
}

fn audio_gain_multiplier(gain_db: f64) -> Result<f64, String> {
    let multiplier = 10.0_f64.powf(gain_db / 20.0);
    if !gain_db.is_finite() || !multiplier.is_finite() {
        return Err("audio-gain-db and its linear multiplier must be finite".into());
    }
    Ok(multiplier)
}

#[derive(Default, Debug)]
struct AudioLevels {
    peak: [u32; 2],
    full_scale_samples: [usize; 2],
    sum_squared: [f64; 2],
}
impl AudioLevels {
    fn observe(&mut self, channel: usize, sample: i16) {
        self.peak[channel] = self.peak[channel].max(i32::from(sample).unsigned_abs());
        self.full_scale_samples[channel] += usize::from(matches!(sample, i16::MIN | i16::MAX));
        self.sum_squared[channel] += f64::from(sample).powi(2);
    }
    fn metadata(&self, frames: usize) -> String {
        let peak_dbfs = self.peak.map(|peak| {
            if peak == 0 {
                "null".into()
            } else {
                (20.0 * (f64::from(peak) / 32768.0).log10()).to_string()
            }
        });
        let rms = self.sum_squared.map(|sum| {
            if frames == 0 {
                0.0
            } else {
                (sum / frames as f64).sqrt()
            }
        });
        format!(
            "{{\"peak_abs_s16\":{:?},\"peak_dbfs\":[{},{}],\"rms_s16\":{:?},\"full_scale_samples\":{:?}}}",
            self.peak, peak_dbfs[0], peak_dbfs[1], rms, self.full_scale_samples
        )
    }
}
#[derive(Debug)]
struct AudioGain {
    multiplier: f64,
    frames: usize,
    before: AudioLevels,
    after: AudioLevels,
    clipped_samples: [usize; 2],
}
impl AudioGain {
    fn metadata(&self) -> String {
        format!(
            "{{\"linear_multiplier\":{},\"stereo_frames\":{},\"conversion\":\"S16LE sample*10^(dB/20), f64 round ties away from zero then clamp to -32768..32767; zero dB preserves bytes\",\"channel_order\":[\"L\",\"R\"],\"before\":{},\"after\":{},\"gain_clipped_samples\":{:?},\"gain_clipped_samples_total\":{}}}",
            self.multiplier, self.frames, self.before.metadata(self.frames), self.after.metadata(self.frames), self.clipped_samples, self.clipped_samples.iter().sum::<usize>()
        )
    }
}
fn apply_audio_gain(audio: &mut [u8], gain_db: f64) -> Result<AudioGain, String> {
    if !audio.len().is_multiple_of(4) {
        return Err("Audio gain requires complete interleaved stereo S16LE frames".into());
    }
    let multiplier = audio_gain_multiplier(gain_db)?;
    let mut measurement = AudioGain {
        multiplier,
        frames: audio.len() / 4,
        before: AudioLevels::default(),
        after: AudioLevels::default(),
        clipped_samples: [0; 2],
    };
    for (index, bytes) in audio.chunks_exact_mut(2).enumerate() {
        let channel = index % 2;
        let original = i16::from_le_bytes([bytes[0], bytes[1]]);
        measurement.before.observe(channel, original);
        let rounded = (f64::from(original) * multiplier).round();
        measurement.clipped_samples[channel] +=
            usize::from(rounded < f64::from(i16::MIN) || rounded > f64::from(i16::MAX));
        let amplified = rounded.clamp(f64::from(i16::MIN), f64::from(i16::MAX)) as i16;
        if gain_db != 0.0 {
            bytes.copy_from_slice(&amplified.to_le_bytes());
        }
        measurement.after.observe(channel, amplified);
    }
    Ok(measurement)
}

fn ffmpeg(args: &[String], input: &Path) -> Result<Vec<u8>, String> {
    let mut cmd = Command::new("ffmpeg");
    cmd.args(["-v", "error", "-nostdin", "-threads", "1"]);
    cmd.args(args).arg("pipe:1").stdin(Stdio::null());
    let result = cmd
        .output()
        .map_err(|e| format!("Cannot run ffmpeg for {}: {e}", input.display()))?;
    if !result.status.success() {
        return Err(format!(
            "FFmpeg failed: {}",
            String::from_utf8_lossy(&result.stderr)
        ));
    }
    Ok(result.stdout)
}
fn quoted(s: &str) -> String {
    let mut result = String::from("\"");
    for c in s.chars() {
        match c {
            '"' => result.push_str("\\\""),
            '\\' => result.push_str("\\\\"),
            '\n' => result.push_str("\\n"),
            '\r' => result.push_str("\\r"),
            '\t' => result.push_str("\\t"),
            c if c.is_control() => result.push_str(&format!("\\u{:04x}", c as u32)),
            c => result.push(c),
        }
    }
    result.push('"');
    result
}
fn write(path: &Path, data: impl AsRef<[u8]>) -> Result<(), String> {
    fs::write(path, data).map_err(|e| format!("{}: {e}", path.display()))
}

fn run(o: Options) -> Result<(), String> {
    if o.wide_dict4_source.is_some() || o.wide360_dict {
        return wide_dict4::run(&o);
    }
    if o.upic_width == 360 || o.wide360_raw_source.is_some() {
        return small_wide::run(&o);
    }
    if o.wide_skip_source.is_some() {
        return wide_skip::run(&o);
    }
    if o.wide_raw_source.is_some() {
        return wide_raw::run(&o);
    }
    if o.wide_lz4_source.is_some() {
        return wide_lz4::run(&o);
    }
    if o.upic_width == 384 {
        return wide::run(&o);
    }
    let palette = read_palette(&o.palette)?;
    let pairs = colour_pairs(&palette, &o);
    let geometry = geometry(&o)?;
    if o.buffered && (geometry.width != 320 || geometry.height != 169 || geometry.left != 0) {
        return Err("buffered first qualification requires active320x169 with left0".into());
    }
    let count_f = (o.duration * o.fps).round();
    if !(1.0..=10000.0).contains(&count_f) {
        return Err("Clip must contain 1..10000 frames".into());
    }
    let frame_count = count_f as usize;
    let duration = frame_count as f64 / o.fps;
    let source_w = geometry.quant_width;
    let source_h = geometry.quant_height;
    let filter = format!(
        "fps={},scale={source_w}:{source_h}:flags=lanczos,setsar=1,format=rgb24",
        o.fps
    );
    let common = vec![
        "-ss".into(),
        o.start.to_string(),
        "-i".into(),
        o.input.to_string_lossy().into_owned(),
    ];
    let mut video_args = common.clone();
    video_args.extend([
        "-an".into(),
        "-vf".into(),
        filter,
        "-frames:v".into(),
        frame_count.to_string(),
        "-f".into(),
        "rawvideo".into(),
        "-pix_fmt".into(),
        "rgb24".into(),
    ]);
    println!(
        "Decoding {frame_count} source frames at {} fps, {}x{} quantisation",
        o.fps, source_w, source_h
    );
    let decoded = ffmpeg(&video_args, &o.input)?;
    let source_bytes = source_w * source_h * 3;
    if decoded.len() != frame_count * source_bytes {
        return Err(format!(
            "Expected {frame_count} decoded frames, got {:.3}",
            decoded.len() as f64 / source_bytes as f64
        ));
    }
    let reference = if o.scale > 1 {
        let mut args = video_args.clone();
        let filter_at = args.iter().position(|s| s == "-vf").unwrap() + 1;
        args[filter_at] = format!(
            "fps={},scale={}:{}:flags=lanczos,setsar=1,format=rgb24",
            o.fps, geometry.width, geometry.height
        );
        let raw = ffmpeg(&args, &o.input)?;
        if raw.len() != frame_count * geometry.width * geometry.height * 3 {
            return Err("Full-resolution reference frame count mismatch".into());
        }
        Some(raw)
    } else {
        None
    };
    let mut audio_args = common;
    audio_args.extend([
        "-t".into(),
        duration.to_string(),
        "-vn".into(),
        "-ac".into(),
        "2".into(),
        "-ar".into(),
        o.audio_rate.to_string(),
        "-c:a".into(),
        "pcm_s16le".into(),
        "-f".into(),
        "s16le".into(),
    ]);
    let mut audio = ffmpeg(&audio_args, &o.input)?;
    if audio.len() % 4 != 0 {
        return Err("PCM is not aligned to stereo samples".into());
    }
    let expected_audio = (duration * o.audio_rate as f64).round() as usize * 4;
    if audio.len().abs_diff(expected_audio) > 4 {
        return Err(format!(
            "PCM duration mismatch: {} vs {expected_audio} bytes",
            audio.len()
        ));
    }
    let audio_gain = apply_audio_gain(&mut audio, o.audio_gain_db)?;
    fs::create_dir_all(&o.output).map_err(|e| e.to_string())?;
    let mut cache = HashMap::<u32, u8>::new();
    let mut positional = (o.dither == "yliluoma")
        .then(|| PositionalDither::new(&palette, o.neutral_guard, o.dither_contrast_weight));
    let mut author = if o.dither == "yliluoma-author" {
        Some(AuthorDither::new(&palette, o.neutral_guard)?)
    } else {
        None
    };
    if let Some(dither) = &mut author {
        let threads = o
            .threads
            .unwrap_or_else(|| std::thread::available_parallelism().map_or(1, usize::from));
        dither.set_threads(threads);
        println!("Preplanning all distinct source RGB values with {threads} author workers");
        dither.preplan_rgb(&decoded);
        println!("Author RGB planning complete; converting frames");
    }
    let mut phase_frames = Vec::with_capacity(frame_count * ticks_per_content_frame(&o));
    let mut logical_frames = Vec::new();
    let mut mixed_rgb = Vec::with_capacity(frame_count * WIDTH * HEIGHT * 3);
    let mut source_rgb = Vec::with_capacity(frame_count * WIDTH * HEIGHT * 3);
    let mut temporal_pixels = 0usize;
    let mut spatial_mix_pixels = 0usize;
    let mut error_sum = 0f64;
    for (frame_index, frame) in decoded.chunks_exact(source_bytes).enumerate() {
        let plans = if let Some(dither) = &mut author {
            Some(dither.frame_plans(frame))
        } else {
            positional.as_mut().map(|dither| dither.frame_plans(frame))
        };
        let virtuals = if plans.is_none() {
            quantise(
                frame,
                source_w,
                source_h,
                &pairs,
                &o.dither,
                o.neutral_guard,
                &mut cache,
            )
        } else {
            Vec::new()
        };
        for (i, pixel) in frame.chunks_exact(3).enumerate() {
            let rgb = [pixel[0], pixel[1], pixel[2]];
            let target = lab(rgb.map(linear));
            let estimate = if let Some(plans) = &plans {
                if let Some(dither) = &positional {
                    let point = dither.candidates[plans[i]].point;
                    [point[0], point[1], point[2]]
                } else {
                    lab(author.as_ref().unwrap().mean_linear(plans[i]))
                }
            } else {
                pairs[virtuals[i] as usize].lab
            };
            error_sum += (0..3)
                .map(|j| (target[j] - estimate[j]).powi(2))
                .sum::<f64>()
                .sqrt();
        }
        let mut phases = [vec![0u8; WIDTH * HEIGHT], vec![0u8; WIDTH * HEIGHT]];
        for y in 0..HEIGHT {
            for x in 0..WIDTH {
                let pos = y * WIDTH + x;
                if !(geometry.top..geometry.top + geometry.height).contains(&y)
                    || !(geometry.left..geometry.left + geometry.width).contains(&x)
                {
                    mixed_rgb.extend([0u8; 3]);
                    source_rgb.extend([0u8; 3]);
                    continue;
                }
                let active_x = x - geometry.left;
                let active_y = y - geometry.top;
                let source = active_y / o.scale * source_w + active_x / o.scale;
                if let Some(plans) = &plans {
                    let id = plans[source];
                    let (colour, is_mixed) = if let Some(dither) = &positional {
                        let plan = dither.candidates[id].plan;
                        (plan.colour(x, y), plan.a != plan.b)
                    } else {
                        let dither = author.as_ref().unwrap();
                        (dither.colour(id, x, y), dither.is_mixed(id))
                    };
                    phases[0][pos] = colour;
                    phases[1][pos] = colour;
                    spatial_mix_pixels += usize::from(is_mixed);
                    mixed_rgb.extend(palette[colour as usize]);
                } else {
                    let pair = &pairs[virtuals[source] as usize];
                    let parity = if o.phase_layout == "checker" {
                        (x + y) & 1
                    } else {
                        0
                    };
                    phases[parity][pos] = pair.a;
                    phases[parity ^ 1][pos] = pair.b;
                    temporal_pixels +=
                        usize::from(o.colour_mode == ColourMode::Temporal && pair.a != pair.b);
                    spatial_mix_pixels +=
                        usize::from(o.colour_mode == ColourMode::Spatial && pair.a != pair.b);
                    mixed_rgb.extend(if o.colour_mode == ColourMode::Spatial {
                        palette[phases[0][pos] as usize]
                    } else {
                        pair.rgb
                    });
                }
                if let Some(reference) = &reference {
                    let pos = (frame_index * geometry.width * geometry.height
                        + active_y * geometry.width
                        + active_x)
                        * 3;
                    source_rgb.extend_from_slice(&reference[pos..pos + 3]);
                } else {
                    source_rgb.extend_from_slice(&frame[source * 3..source * 3 + 3]);
                }
            }
        }
        let first = pack(&phases[0]);
        if o.colour_mode == ColourMode::Temporal {
            append_ab(&mut phase_frames, &first, &pack(&phases[1]), o.phase_repeat);
        } else {
            logical_frames.extend_from_slice(&first);
            append_held(&mut phase_frames, &first, ticks_per_content_frame(&o));
        }
    }
    let mut stream = Vec::new();
    let mut records = Vec::new();
    let mut unpadded_stream_bytes = 0;
    let mut max_packet = 0;
    let mut total = PacketStats::default();
    let mut max_cycles = 0;
    let mut max_writes = 0;
    let buffered_assets = if o.buffered {
        Some(buffered::encode(
            &logical_frames,
            geometry.top,
            ticks_per_content_frame(&o),
            o.tick_budget,
        )?)
    } else {
        None
    };
    if let Some(assets) = &buffered_assets {
        stream = assets.stream.clone();
        unpadded_stream_bytes = assets.total_record_bytes;
        for stats in &assets.records {
            max_packet = max_packet.max(stats.payload_bytes);
            max_cycles = max_cycles.max(stats.decoder_cycles);
            max_writes = max_writes.max(stats.writes);
            records.push(stats.json(phase_frames.len()));
            total.copied += stats.copied;
            total.filled += stats.filled;
            total.alternating += stats.alternating;
            total.runs += stats.commands;
            total.changed += stats.changed;
        }
    } else {
        for i in 0..phase_frames.len() {
            let next = (i + 1) % phase_frames.len();
            let (packet, stats) = encode_packet(&phase_frames[i], &phase_frames[next]);
            if packet.len() > u16::MAX as usize {
                return Err("Packet exceeds u16 length".into());
            }
            let mut check = phase_frames[i].clone();
            decode_packet(&mut check, &packet)?;
            if check != phase_frames[next] {
                return Err(format!("Reference decoder mismatch at phase {i}"));
            }
            if stream.len() % 65536 + packet.len() + 2 > 65534 {
                // Zero length is a block-end marker. Pad keeps UCI cache reads fixed-size.
                let end = (stream.len() / 65536 + 1) * 65536;
                stream.resize(end, 0);
            }
            let offset = stream.len();
            stream.extend_from_slice(&(packet.len() as u16).to_le_bytes());
            stream.extend_from_slice(&packet);
            unpadded_stream_bytes += packet.len() + 2;
            max_packet = max_packet.max(packet.len());
            max_cycles = max_cycles.max(stats.estimated_cycles());
            max_writes = max_writes.max(stats.writes());
            records.push(format!("    {{\"index\":{i},\"from_phase\":{i},\"to_phase\":{next},\"offset\":{offset},\"payload_offset\":{},\"payload_bytes\":{},\"record_bytes\":{},\"changed_bytes\":{},\"written_bytes\":{},\"runs\":{},\"copy_bytes\":{},\"fill_bytes\":{},\"alternating_bytes\":{},\"expanded2_bytes\":{},\"expanded4_bytes\":{},\"double_column_bytes\":{},\"checker2_bytes\":{},\"flip2_bytes\":{},\"flip2_runs\":{},\"flip_page_bytes\":{},\"flip_pages\":{},\"flip_page_runs\":{},\"flip_bytes\":{},\"flip_runs\":{},\"estimated_decoder_cycles\":{}}}", offset + 2, packet.len(), packet.len() + 2, stats.changed, stats.writes(), stats.runs, stats.copied, stats.filled, stats.alternating, stats.expanded2, stats.expanded4, stats.doubled4, stats.checker2, stats.flip2, stats.flip2_runs, stats.flip_pages*256, stats.flip_pages, stats.flip_page_runs, stats.flip_bytes(), stats.flip_runs(), stats.estimated_cycles()));
            total.copied += stats.copied;
            total.filled += stats.filled;
            total.alternating += stats.alternating;
            total.expanded2 += stats.expanded2;
            total.expanded4 += stats.expanded4;
            total.doubled4 += stats.doubled4;
            total.doubled_runs += stats.doubled_runs;
            total.checker2 += stats.checker2;
            total.checker_runs += stats.checker_runs;
            total.flip2 += stats.flip2;
            total.flip2_runs += stats.flip2_runs;
            total.flip_pages += stats.flip_pages;
            total.flip_page_runs += stats.flip_page_runs;
            total.runs += stats.runs;
            total.changed += stats.changed;
        }
    }
    if !o.buffered {
        let final_end = (stream.len() / 65536 + 1) * 65536;
        stream.resize(final_end, 0);
    }
    write(&o.output.join("initial.bin"), &phase_frames[0])?;
    if let Some(assets) = &buffered_assets {
        write(&o.output.join("initial.compact"), &assets.initial)?;
    }
    write(&o.output.join("oracle.phases"), phase_frames.concat())?;
    if o.colour_mode != ColourMode::Temporal {
        write(&o.output.join("oracle.frames"), &logical_frames)?;
    }
    write(&o.output.join("stream.bin"), &stream)?;
    write(&o.output.join("audio.pcm"), &audio)?;
    write(&o.output.join("mixed.rgb"), &mixed_rgb)?;
    write(&o.output.join("source.rgb"), &source_rgb)?;
    let colour_pairs = pairs
        .iter()
        .map(|p| format!("[{},{}]", p.a, p.b))
        .collect::<Vec<_>>()
        .join(",");
    let palette_rgb = palette
        .iter()
        .map(|c| format!("[{},{},{}]", c[0], c[1], c[2]))
        .collect::<Vec<_>>()
        .join(",");
    let pair_luminance_metrics = pairs
        .iter()
        .map(|p| {
            format!(
                "{{\"indices\":[{},{}],\"linear_y\":[{},{}],\"relative_y_delta\":{}}}",
                p.a, p.b, p.luminance[0], p.luminance[1], p.relative_luminance_delta
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let fields = vec![
        ("format", quoted(if o.buffered {"MUpic buffered v2"} else {"MUpic span v1"})),
        ("span_version", if o.buffered {"2".into()} else {"1".into()}),
        ("input", quoted(&o.input.to_string_lossy())),
        ("start_seconds", o.start.to_string()),
        ("duration_seconds", duration.to_string()),
        ("width", WIDTH.to_string()),
        ("height", HEIGHT.to_string()),
        ("active_width", geometry.width.to_string()),
        ("active_height", geometry.height.to_string()),
        ("active_top", geometry.top.to_string()),
        ("active_left", geometry.left.to_string()),
        ("fit_canvas", o.fit_canvas.to_string()),
        ("align", quoted(&o.align)),
        ("display_pixel_aspect_ratio", o.pixel_aspect.to_string()),
        ("active_display_aspect_ratio", (geometry.width as f64 * o.pixel_aspect / geometry.height as f64).to_string()),
        ("original_width",geometry.source_aspect.as_ref().map_or_else(||"null".into(),|a|a.width.to_string())),
        ("original_height",geometry.source_aspect.as_ref().map_or_else(||"null".into(),|a|a.height.to_string())),
        ("original_sample_aspect_ratio",geometry.source_aspect.as_ref().map_or_else(||"null".into(),|a|a.sample_aspect.to_string())),
        ("original_display_aspect_ratio",geometry.source_aspect.as_ref().map_or_else(||"null".into(),|a|a.display_aspect.to_string())),
        ("aspect_rounding_error_relative",geometry.source_aspect.as_ref().map_or_else(||"null".into(),|a|(geometry.width as f64 * o.pixel_aspect / geometry.height as f64 / a.display_aspect - 1.0).to_string())),
        ("quantisation_width", source_w.to_string()),
        ("quantisation_height", source_h.to_string()),
        ("source_fps", o.fps.to_string()),
        ("colour_mode", quoted(o.colour_mode.name())),
        ("temporal_mixing", (o.colour_mode==ColourMode::Temporal).to_string()),
        ("phase_fps", display_hz(&o).to_string()),
        ("display_fps", display_hz(&o).to_string()),
        ("phase_repeat", o.phase_repeat.to_string()),
        ("frame_ticks", if o.colour_mode==ColourMode::Temporal {"null".into()} else {ticks_per_content_frame(&o).to_string()}),
        ("ticks_per_content_frame",ticks_per_content_frame(&o).to_string()),
        ("source_frames", frame_count.to_string()),
        ("phases", phase_frames.len().to_string()),
        ("display_ticks",phase_frames.len().to_string()),
        ("logical_oracle_file",if o.colour_mode==ColourMode::Temporal {"null".into()} else {quoted("oracle.frames")}),
        ("logical_oracle_frames",if o.colour_mode==ColourMode::Temporal {"0".into()} else {frame_count.to_string()}),
        ("phase_order", quoted(&display_order(&o))),
        ("phase_layout", quoted(&o.phase_layout)),
        ("loop_transition", (!o.buffered).to_string()),
        ("initial_frame_bytes", FRAME_BYTES.to_string()),
        (
            "raw_phase_bytes",
            (phase_frames.len() * FRAME_BYTES).to_string(),
        ),
        ("stream_bytes", stream.len().to_string()),
        ("stream_blocks", (stream.len() / 65536).to_string()),
        ("stream_block_bytes", "65536".into()),
        ("record_bytes_total", unpadded_stream_bytes.to_string()),
        (
            "video_bytes_per_second",
            (stream.len() as f64 / duration).to_string(),
        ),
        (
            "video_record_bytes_per_second",
            (unpadded_stream_bytes as f64 / duration).to_string(),
        ),
        ("max_packet_bytes", max_packet.to_string()),
        ("packet_budget", if o.buffered {"null".into()} else {o.packet_budget.to_string()}),
        (
            "fits_packet_buffer",
            if o.buffered {"null".into()} else {(max_packet <= o.packet_budget).to_string()},
        ),
        ("buffered",o.buffered.to_string()),
        ("native_cache_ticks",o.cache_ticks.map_or_else(||"null".into(),|v|v.to_string())),
        ("framebuffer_bytes",if o.buffered {buffered::FRAME_BYTES.to_string()} else {FRAME_BYTES.to_string()}),
        ("buffered_memory_map",if o.buffered {buffered::memory_map_json()} else {"null".into()}),
        ("buffered_chunk_capacity",if o.buffered {buffered::CHUNK_CAPACITY.to_string()} else {"null".into()}),
        ("max_chunk_bytes",buffered_assets.as_ref().map_or_else(||"null".into(),|a|a.max_chunk_bytes.to_string())),
        ("fits_chunk_buffer",buffered_assets.as_ref().map_or_else(||"null".into(),|a|(a.max_chunk_bytes<=buffered::CHUNK_CAPACITY).to_string())),
        ("chunk_count_total",buffered_assets.as_ref().map_or_else(||"null".into(),|a|a.records.iter().map(|r|r.chunks).sum::<usize>().to_string())),
        ("swap_count",buffered_assets.as_ref().map_or_else(||"null".into(),|a|a.records.iter().filter(|r|r.swap).count().to_string())),
        ("empty_tick_count",buffered_assets.as_ref().map_or_else(||"null".into(),|a|a.records.iter().filter(|r|r.chunks==0).count().to_string())),
        ("max_tick_payload_bytes",if o.buffered {max_packet.to_string()} else {"null".into()}),
        ("estimated_tick_cpu_slots_max",buffered_assets.as_ref().map_or_else(||"null".into(),|a|a.max_tick_slots.to_string())),
        ("tick_budget_cpu_slots",if o.buffered {o.tick_budget.to_string()} else {"null".into()}),
        ("fits_tick_budget",buffered_assets.as_ref().map_or_else(||"null".into(),|a|a.fits_tick_budget.to_string())),
        ("initial_compact_file",if o.buffered {quoted("initial.compact")} else {"null".into()}),
        ("initial_frame_both_slots",o.buffered.to_string()),
        ("buffered_wire_format",if o.buffered {quoted("Outer LE16 payload length; flags bit0SWAP; repeated LE16chunklength and terminated span chunk<=1024; final LE16zero. COPY0/FILL1/ALT2 only, absolute destinationminus1/count. EOF holds final front without wrap.")} else {"null".into()}),
        ("buffered_tick_cost_model",if o.buffered {quoted("63*all wire bytes including outer length, flags, chunk lengths and terminators +200/command +COPY15/FILL10/ALT12 per output byte +500/chunk; sequential next-fit partition balanced by binary-searched maximum; native timing authoritative")} else {"null".into()}),
        (
            "audio_encoding",
            quoted("signed PCM 16-bit little-endian stereo interleaved"),
        ),
        ("audio_sample_rate", o.audio_rate.to_string()),
        ("audio_samples", (audio.len() / 4).to_string()),
        ("audio_bytes", audio.len().to_string()),
        ("audio_gain_db", o.audio_gain_db.to_string()),
        ("audio_gain", audio_gain.metadata()),
        (
            "combined_bytes_per_second",
            ((stream.len() + audio.len()) as f64 / duration).to_string(),
        ),
        ("colour_pair_count", pairs.len().to_string()),
        ("colour_pair_luminance_limit", o.luma_limit.to_string()),
        ("colour_pair_rgb_luminance_limit",o.rgb_luma_limit.map_or_else(||"null".into(),|limit|limit.to_string())),
        ("colour_pair_rgb_luminance_criterion",quoted("Additional to Pepto: abs(Ya-Yb)/((Ya+Yb)/2) <= limit; Y=0.2126R+0.7152G+0.0722B in linear-light sRGB; zero-mean delta=0; all 16 base colours retained")),
        (
            "colour_model",
            quoted(match o.colour_mode {
                ColourMode::Hardware if o.dither == "yliluoma" => "16 hardware colours, linear-light two-colour spatial plans, fixed Bayer8 positional dither",
                ColourMode::Hardware if o.dither == "yliluoma-author" => "16 hardware colours, author gamma2.2 multicolour plans selected by CIEDE2000, fixed positional matrix; no temporal mixing",
                ColourMode::Hardware => "nearest OKLab among 16 hardware colours, no temporal mixing",
                ColourMode::Temporal => "linear-light pair average, nearest OKLab",
                ColourMode::Spatial => "linear-light pair average, nearest OKLab, static spatial checker",
            }),
        ),
        (
            "source_rgb_reference",
            quoted("full active image resize in black 320x200 canvas, before quantisation or scale2/scale4 loss"),
        ),
        (
            "mean_oklab_error_scope",
            quoted(if positional.is_some() || author.is_some() {"at quantisation resolution, source versus selected spatial plan mean; excludes contrast penalty, per-pixel spatial error and scale2/scale4 resolution loss"} else {"at quantisation resolution; excludes scale2/scale4 resolution loss"}),
        ),
        ("neutral_guard", o.neutral_guard.to_string()),
        ("spatial_dither", quoted(&o.dither)),
        ("dither_adaptation",positional.as_ref().map_or_else(||"null".into(),|p|format!("{{\"algorithm\":\"Yliluoma-inspired positional two-colour planning; adaptation, not the exact published algorithm\",\"source\":\"https://bisqwit.iki.fi/story/howto/dither/jy/\",\"matrix\":\"fixed Bayer8x8 at physical canvas x/y; no frame index or error propagation\",\"plans\":{},\"cached_rgb_values\":{},\"ratios\":\"1..63/64 for all120 hardware colour pairs plus16 pure colours\",\"mixing\":\"linear-light sRGB\",\"metric\":\"squared OKLab mean error + weight*r*(1-r)*squared OKLab pair contrast\",\"contrast_weight\":{},\"contrast_weight_basis\":\"empirical\",\"pair_orientation\":\"fixed increasing Rec709 linear-light luminance\",\"search\":\"exact4D KD-tree; sqrt(contrast penalty) fourth coordinate, query fourth coordinate0\",\"pair_filters\":\"all hardware pairs; temporal Pepto/rgb-luma filters do not restrict these spatial plans\",\"stability\":\"same RGB and canvas position gives same hardware index; changed RGB may cross thresholds or change plans\"}}",p.candidates.len(),p.cache.len(),p.contrast_weight))),
        ("author_dither_adaptation",author.as_ref().map_or_else(||"null".into(),AuthorDither::metadata)),
        ("threads",o.threads.map_or_else(||quoted("auto"),|n|n.to_string())),
        ("palette_path", quoted(&o.palette.to_string_lossy())),
        ("palette_rgb", format!("[{palette_rgb}]")),
        ("colour_pairs", format!("[{colour_pairs}]")),
        ("colour_pair_luminance_metrics",format!("[{pair_luminance_metrics}]")),
        ("temporal_pixels",temporal_pixels.to_string()),
        ("spatial_mix_pixels",spatial_mix_pixels.to_string()),
        ("temporal_pixel_scope",quoted("logical content frames, active canvas image pixels; excludes display repetitions")),
        (
            "temporal_pixel_fraction",
            (temporal_pixels as f64 / (frame_count * geometry.width * geometry.height) as f64).to_string(),
        ),
        (
            "mean_oklab_error",
            (error_sum / (frame_count * source_w * source_h) as f64).to_string(),
        ),
        ("changed_bytes_total", total.changed.to_string()),
        ("written_bytes_total", total.writes().to_string()),
        ("runs_total", total.runs.to_string()),
        ("flip2_bytes_total", total.flip2.to_string()),
        ("flip2_runs_total", total.flip2_runs.to_string()),
        ("flip_page_bytes_total", (total.flip_pages * 256).to_string()),
        ("flip_pages_total", total.flip_pages.to_string()),
        ("flip_page_runs_total", total.flip_page_runs.to_string()),
        ("flip_bytes_total", total.flip_bytes().to_string()),
        ("flip_runs_total", total.flip_runs().to_string()),
        ("max_written_bytes", max_writes.to_string()),
        ("max_estimated_decoder_cycles", max_cycles.to_string()),
        ("flip_commands",quoted("tag8: no payload, even output count 2..254 and even destination; tag9: no payload, count 1..255 aligned 256-byte pages; both region-bounded, input pairs V,swap(V), output swaps every nibble pair")),
        ("flip_cycle_model",format!("{{\"flip2_header_cycles\":{FLIP2_HEADER_CYCLES},\"flip2_cycles_per_pair\":{FLIP2_PAIR_CYCLES},\"flip_page_header_cycles\":{FLIP_PAGE_HEADER_CYCLES},\"flip_page_cycles_per_page\":{FLIP_PAGE_CYCLES}}}")),
        (
            "cycle_estimate_warning",
            quoted("inner-loop estimate only; native timing determines deadline feasibility"),
        ),
        ("reference_roundtrip_verified", "true".into()),
        ("records", format!("[\n{}\n  ]", records.join(",\n"))),
    ];
    let manifest = format!(
        "{{\n{}\n}}\n",
        fields
            .into_iter()
            .map(|(key, value)| format!("  {}:{value}", quoted(key)))
            .collect::<Vec<_>>()
            .join(",\n")
    );
    write(&o.output.join("manifest.json"), manifest)?;
    println!(
        "{} phase records, {} bytes video ({:.0} B/s), {} bytes audio",
        phase_frames.len(),
        stream.len(),
        stream.len() as f64 / duration,
        audio.len()
    );
    if let Some(assets) = &buffered_assets {
        println!(
            "Buffered chunks max{} /1024 bytes; tick model max{} /{} slots, fits{}",
            assets.max_chunk_bytes, assets.max_tick_slots, o.tick_budget, assets.fits_tick_budget
        );
    } else {
        println!(
        "Largest packet {max_packet} / {} bytes RAM; maximum estimated decoder cycles {max_cycles}",
        o.packet_budget
    );
    }
    if !o.buffered && max_packet > o.packet_budget {
        eprintln!("Packet RAM budget exceeded; inspect manifest before playing this profile.");
        if o.strict_budget {
            return Err("Packet RAM budget exceeded".into());
        }
    }
    if o.native_mux {
        let mux = if let Some(ticks) = o.cache_ticks {
            mux_fixed_tick_blocks(
                &stream,
                &audio,
                phase_frames.len(),
                o.audio_divider,
                o.audio_offset,
                ticks,
            )?
        } else {
            mux_cache_blocks(
                &stream,
                &audio,
                phase_frames.len(),
                o.audio_divider,
                o.audio_offset,
            )?
        };
        write(&o.output.join("stream.mup"), &mux.file)?;
        write(
            &o.output.join("cache-mux.json"),
            mux.metadata(o.audio_offset, o.audio_divider, o.audio_rate),
        )?;
        println!(
            "Native A/V: {} blocks, {} bytes; {} useful video / {} PCM bytes",
            mux.block_records.len(),
            mux.file.len(),
            mux.useful_video_bytes,
            mux.audio_lengths.iter().sum::<usize>()
        );
    }
    Ok(())
}

pub fn legacy_cli() {
    let result = parse_options().and_then(|o| match o {
        Some(o) => run(o),
        None => Ok(()),
    });
    if let Err(e) = result {
        eprintln!("mupic-encoder: {e}");
        std::process::exit(1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn options_with(args: &[&str]) -> Result<Options, String> {
        parse_options_args(
            ["--input", "fixture.mp4", "--output", "fixture-output"]
                .iter()
                .chain(args)
                .map(|s| (*s).to_owned()),
        )?
        .ok_or_else(|| "Missing options".into())
    }
    #[test]
    fn contrast_weight_and_audio_gain_parameters_are_explicit_and_validated() {
        let defaults = options_with(&[]).unwrap();
        assert_eq!(defaults.dither_contrast_weight, 0.1);
        assert_eq!(defaults.audio_gain_db, 0.0);
        let selected =
            options_with(&["--dither-contrast-weight", "0.03", "--audio-gain-db", "-6"]).unwrap();
        assert_eq!(selected.dither_contrast_weight, 0.03);
        assert_eq!(selected.audio_gain_db, -6.0);
        assert!(options_with(&["--dither-contrast-weight", "0"]).is_ok());
        for value in ["-0.1", "NaN", "inf", "-inf", "invalid"] {
            assert!(options_with(&["--dither-contrast-weight", value]).is_err());
        }
        for value in ["NaN", "inf", "-inf", "10000", "invalid"] {
            assert!(options_with(&["--audio-gain-db", value]).is_err());
        }
    }
    #[test]
    fn audio_gain_preserves_zero_db_bytes_and_measures_s16_limits() {
        let mut audio = (i16::MIN..=i16::MAX)
            .flat_map(i16::to_le_bytes)
            .collect::<Vec<_>>();
        let before = audio.clone();
        let measurement = apply_audio_gain(&mut audio, 0.0).unwrap();
        assert_eq!(audio, before);
        assert_eq!(measurement.frames, 32768);
        assert_eq!(measurement.before.peak, [32768, 32767]);
        assert_eq!(measurement.before.full_scale_samples, [1, 1]);
        assert_eq!(measurement.after.peak, measurement.before.peak);
        assert_eq!(measurement.clipped_samples, [0, 0]);
        assert!(apply_audio_gain(&mut [0, 0], 0.0).is_err());
        assert!(apply_audio_gain(&mut [], f64::NAN).is_err());
    }
    #[test]
    fn audio_gain_rounds_and_clamps_without_wrapping_and_counts_real_clipping() {
        let values: [i16; 8] = [16383, -16384, 16384, -16385, 32767, -32768, 1, -1];
        let mut audio = values
            .into_iter()
            .flat_map(i16::to_le_bytes)
            .collect::<Vec<_>>();
        let measurement = apply_audio_gain(&mut audio, 20.0 * 2.0_f64.log10()).unwrap();
        let result = audio
            .chunks_exact(2)
            .map(|bytes| i16::from_le_bytes([bytes[0], bytes[1]]))
            .collect::<Vec<_>>();
        assert_eq!(result, [32766, -32768, 32767, -32768, 32767, -32768, 2, -2]);
        assert_eq!(measurement.clipped_samples, [2, 2]);
        assert_eq!(measurement.after.full_scale_samples, [2, 3]);
        let mut half = [1i16, -1, 3, -3]
            .into_iter()
            .flat_map(i16::to_le_bytes)
            .collect::<Vec<_>>();
        apply_audio_gain(&mut half, 20.0 * 0.5_f64.log10()).unwrap();
        assert_eq!(
            half,
            [1i16, -1, 2, -2]
                .into_iter()
                .flat_map(i16::to_le_bytes)
                .collect::<Vec<_>>()
        );
    }
    #[test]
    fn positional_neutral_guard_applies_to_each_contrast_preset() {
        let palette = read_palette(&options_with(&[]).unwrap().palette).unwrap();
        for weight in [0.03, 0.1, 0.3] {
            let mut dither = PositionalDither::new(&palette, true, weight);
            for level in (0..=255u8).step_by(7) {
                for rgb in [
                    [level; 3],
                    [level, level.saturating_add(4), level.saturating_add(8)],
                ] {
                    let index = dither.plan(rgb);
                    let candidate = dither.candidates[index];
                    assert!(candidate.neutral, "weight {weight}, colour {rgb:?}");
                    for y in 0..8 {
                        for x in 0..8 {
                            assert!([0, 1, 11, 12, 15].contains(&candidate.plan.colour(x, y)));
                        }
                    }
                }
            }
        }
    }
    #[test]
    fn author_dither_cli_is_distinct_and_hardware_only() {
        let options = options_with(&[
            "--colour-mode",
            "hardware",
            "--dither",
            "yliluoma-author",
            "--neutral-guard",
        ])
        .unwrap();
        assert_eq!(options.dither, "yliluoma-author");
        assert!(options.neutral_guard);
        assert_eq!(options.dither_contrast_weight, 0.1);
        assert!(options_with(&["--dither", "yliluoma-author"]).is_err());
        assert!(
            options_with(&["--colour-mode", "spatial", "--dither", "yliluoma-author"]).is_err()
        );
    }

    #[test]
    fn author_thread_count_is_explicit_and_positive() {
        assert_eq!(options_with(&[]).unwrap().threads, None);
        assert_eq!(options_with(&["--threads", "auto"]).unwrap().threads, None);
        assert_eq!(options_with(&["--threads", "4"]).unwrap().threads, Some(4));
        for bad in ["0", "-1", "1.5", "nan", ""] {
            assert!(options_with(&["--threads", bad]).is_err());
        }
    }
    #[test]
    fn author_dither_plans_keep_fixed_hardware_pixels_and_neutral_guard() {
        let palette = read_palette(&options_with(&[]).unwrap().palette).unwrap();
        let mut author = AuthorDither::new(&palette, true).unwrap();
        let plan = author.plan([128; 3]);
        let expected = (0..64)
            .map(|pixel| author.colour(plan, pixel % 8 + 3, pixel / 8 + 15))
            .collect::<Vec<_>>();
        assert!(expected
            .iter()
            .all(|index| [0, 1, 11, 12, 15].contains(index)));
        let mean = author.mean_linear(plan);
        assert!(mean
            .iter()
            .all(|channel| channel.is_finite() && (0.0..=1.0).contains(channel)));
        author.plan([80, 127, 201]);
        let repeated = author.plan([128; 3]);
        assert_eq!(repeated, plan);
        assert_eq!(
            (0..64)
                .map(|pixel| author.colour(repeated, pixel % 8 + 3, pixel / 8 + 15))
                .collect::<Vec<_>>(),
            expected
        );
        let black = author.plan(palette[0]);
        assert!(!author.is_mixed(black));
        assert!((0..64).all(|pixel| author.colour(black, pixel % 8, pixel / 8) == 0));
    }
    #[test]
    fn buffered_options_are_explicitly_isolated_from_legacy_temporal_and_scaling() {
        let options = options_with(&[
            "--buffered",
            "--colour-mode",
            "hardware",
            "--fit-canvas",
            "--pixel-aspect",
            "15:16",
            "--frame-ticks",
            "4",
            "--no-native-mux",
        ])
        .unwrap();
        assert!(options.buffered);
        assert_eq!(options.tick_budget, 490000);
        assert!(options_with(&["--buffered"]).is_err());
        assert!(options_with(&[
            "--buffered",
            "--colour-mode",
            "hardware",
            "--fit-canvas",
            "--scale2"
        ])
        .is_err());
        assert!(options_with(&["--buffered", "--colour-mode", "spatial", "--fit-canvas"]).is_err());
        assert!(options_with(&[
            "--buffered",
            "--colour-mode",
            "hardware",
            "--fit-canvas",
            "--tick-budget",
            "0"
        ])
        .is_err());
    }
    #[test]
    fn fixed_cache_cli_is_buffered_native_only_and_sets_audio_offset_implicitly() {
        let args = [
            "--buffered",
            "--colour-mode",
            "hardware",
            "--fit-canvas",
            "--pixel-aspect",
            "15:16",
            "--frame-ticks",
            "5",
            "--cache-ticks",
            "20",
        ];
        let options = options_with(&args).unwrap();
        assert_eq!(options.cache_ticks, Some(20));
        assert_eq!(options.audio_offset, 131072);
        let explicit = args
            .iter()
            .copied()
            .chain(["--audio-offset", "0x20000"])
            .collect::<Vec<_>>();
        assert_eq!(options_with(&explicit).unwrap().audio_offset, 131072);
        let no_mux = args
            .iter()
            .copied()
            .chain(["--no-native-mux"])
            .collect::<Vec<_>>();
        assert!(options_with(&no_mux).is_err());
        assert!(options_with(&["--cache-ticks", "20"]).is_err());
        assert!(options_with(&[
            "--buffered",
            "--colour-mode",
            "hardware",
            "--fit-canvas",
            "--cache-ticks",
            "21"
        ])
        .is_err());
        assert_eq!(options_with(&[]).unwrap().audio_offset, 32768);
    }
    fn raw_mux_fixture(count: usize, payload_len: usize) -> Vec<u8> {
        let mut raw = Vec::new();
        for index in 0..count {
            if raw.len() % 65536 + payload_len + 2 > 65534 {
                raw.resize((raw.len() / 65536 + 1) * 65536, 0);
            }
            raw.extend((payload_len as u16).to_le_bytes());
            raw.extend(vec![index as u8; payload_len]);
        }
        raw.resize((raw.len() / 65536 + 1) * 65536, 0);
        raw
    }
    #[test]
    fn fixed_cache_blocks_preserve_twenty_tick_boundaries_partial_eof_and_pcm() {
        let raw = raw_mux_fixture(41, 5000);
        let original = records_from_raw(&raw).unwrap();
        let audio = (0..sample_at_tick(41, 281) * 4)
            .map(|i| (i * 29 + i / 251) as u8)
            .collect::<Vec<_>>();
        let mux = mux_fixed_tick_blocks(&raw, &audio, 41, 281, 131072, 20).unwrap();
        assert_eq!(mux.block_records, [20, 20, 1]);
        assert_eq!(mux.file.len(), 3 * 262144);
        assert_eq!(mux.padded_audio_bytes, 0);
        let mut pcm = Vec::new();
        for (index, length) in mux.audio_lengths.iter().enumerate() {
            pcm.extend_from_slice(
                &mux.file[index * 262144 + 131072..index * 262144 + 131072 + length],
            );
            assert_eq!(
                *length,
                (sample_at_tick(((index + 1) * 20).min(41), 281) - sample_at_tick(index * 20, 281))
                    * 4
            );
        }
        assert_eq!(pcm, audio);
        for (packet, offset) in original.iter().zip(&mux.record_offsets) {
            assert_eq!(&mux.file[*offset..offset + packet.len()], packet);
            let cache_address = offset % 262144 + (offset / 262144 % 2) * 262144;
            let bytes = [
                cache_address as u8,
                (cache_address >> 8) as u8,
                (cache_address >> 16) as u8,
            ];
            assert_eq!(
                usize::from(bytes[0]) | usize::from(bytes[1]) << 8 | usize::from(bytes[2]) << 16,
                cache_address
            );
        }
        assert!(mux
            .record_offsets
            .iter()
            .take(20)
            .any(|offset| offset % 262144 >= 65536));
        assert!(mux
            .record_offsets
            .iter()
            .take(20)
            .zip(&original)
            .any(|(offset, packet)| offset / 65536 != (offset + packet.len() - 1) / 65536));
        let exact =
            mux_fixed_tick_blocks(&raw_mux_fixture(40, 3), &audio, 40, 281, 131072, 20).unwrap();
        assert_eq!(exact.block_records, [20, 20]);
        let metadata = mux.metadata(131072, 281, 22085);
        assert!(metadata.contains("\"cache_slot_bytes\":262144"));
        assert!(metadata.contains("\"cache_bytes\":524288"));
        assert!(metadata.contains("\"fixed_ticks_per_block\":20"));
    }
    #[test]
    fn fixed_cache_checks_video_marker_audio_capacity_and_zero_pads_short_pcm() {
        let raw = raw_mux_fixture(20, 3);
        let audio_bytes = sample_at_tick(20, 281) * 4;
        let audio = vec![0x55; audio_bytes];
        assert!(mux_fixed_tick_blocks(&raw, &audio, 20, 281, 104, 20).is_ok());
        assert!(mux_fixed_tick_blocks(&raw, &audio, 20, 281, 100, 20).is_err());
        let audio_offset = 262144 - audio_bytes;
        assert!(mux_fixed_tick_blocks(&raw, &audio, 20, 281, audio_offset, 20).is_ok());
        assert!(mux_fixed_tick_blocks(&raw, &audio, 20, 281, audio_offset + 4, 20).is_err());
        let short = &audio[..audio.len() - 4];
        let mux = mux_fixed_tick_blocks(&raw, short, 20, 281, 131072, 20).unwrap();
        assert_eq!(mux.padded_audio_bytes, 4);
        assert_eq!(
            &mux.file[131072 + short.len()..131072 + audio.len()],
            [0; 4]
        );
        assert!(
            mux_fixed_tick_blocks(&raw, &audio[..audio.len() - 1], 20, 281, 131072, 20).is_err()
        );
        assert!(mux_fixed_tick_blocks(&raw, &audio, 19, 281, 131072, 20).is_err());
        assert!(mux_fixed_tick_blocks(&raw[..65535], &audio, 20, 281, 131072, 20).is_err());
        let legacy = mux_cache_blocks(&raw, &audio, 20, 281, 32768).unwrap();
        assert_eq!(legacy.block_bytes, 65536);
        assert_eq!(legacy.fixed_ticks_per_block, None);
        assert!(!legacy
            .metadata(32768, 281, 22085)
            .contains("fixed_ticks_per_block"));
    }
    #[test]
    fn aspect_fit_honours_source_dar_sar_pixel_aspect_and_both_anchors() {
        let bbb = parse_probe_aspect(
            "width=1280\nheight=720\nsample_aspect_ratio=1:1\ndisplay_aspect_ratio=16:9\n",
        )
        .unwrap();
        for (align, top) in [("start", 0), ("center", 15), ("end", 31)] {
            let g = fitted_geometry(bbb.clone(), ratio("15:16").unwrap(), align, 1).unwrap();
            assert_eq!((g.width, g.height, g.left, g.top), (320, 169, 0, top));
            assert_eq!((g.quant_width, g.quant_height), (320, 169));
            let error = g.width as f64 * 15.0 / 16.0 / g.height as f64 / bbb.display_aspect - 1.0;
            assert!((-0.002..0.0).contains(&error));
        }
        let square_pixels = fitted_geometry(bbb, 1.0, "center", 1).unwrap();
        assert_eq!(
            (square_pixels.width, square_pixels.height, square_pixels.top),
            (320, 180, 10)
        );
        let anamorphic = parse_probe_aspect(
            "width=720\nheight=576\nsample_aspect_ratio=16:15\ndisplay_aspect_ratio=N/A\n",
        )
        .unwrap();
        assert!((anamorphic.display_aspect - 4.0 / 3.0).abs() < 1e-12);
        for (align, left) in [("start", 0), ("center", 26), ("end", 53)] {
            let g = fitted_geometry(anamorphic.clone(), 1.0, align, 1).unwrap();
            assert_eq!((g.width, g.height, g.left, g.top), (267, 200, left, 0));
        }
        assert_eq!(ratio("0.9375"), ratio("15/16"));
        assert!(ratio("1:0").is_none() && ratio("NaN").is_none());
        assert!(parse_probe_aspect("width=0\nheight=720\n").is_err());
        let legacy = geometry(&options_with(&["--scale4"]).unwrap()).unwrap();
        assert_eq!(
            (
                legacy.width,
                legacy.height,
                legacy.top,
                legacy.quant_width,
                legacy.quant_height
            ),
            (320, 180, 8, 80, 45)
        );
    }
    #[test]
    fn hardware_and_spatial_cadence_hold_content_without_ab_alternation() {
        let hardware = options_with(&[
            "--colour-mode",
            "hardware",
            "--frame-ticks",
            "3",
            "--dither",
            "none",
        ])
        .unwrap();
        assert!((hardware.fps - pal_tick_hz() / 3.0).abs() < 1e-12);
        assert!((display_hz(&hardware) - pal_tick_hz()).abs() < 1e-12);
        assert_eq!(display_order(&hardware), "F0,F0,F0,F1,F1,F1,...");
        let palette = read_palette(&hardware.palette).unwrap();
        let colours = colour_pairs(&palette, &hardware);
        assert_eq!(colours.len(), 16);
        let indices = quantise(
            &palette.concat(),
            16,
            1,
            &colours,
            "none",
            false,
            &mut HashMap::new(),
        );
        for (base, index) in indices.iter().enumerate() {
            assert_eq!(
                (colours[*index as usize].a, colours[*index as usize].b),
                (base as u8, base as u8)
            );
        }
        let first = vec![0; FRAME_BYTES];
        let mut next = first.clone();
        next[..128].fill(0x11);
        let mut held = Vec::new();
        append_held(&mut held, &first, ticks_per_content_frame(&hardware));
        append_held(&mut held, &next, ticks_per_content_frame(&hardware));
        assert_eq!(held.len(), 6);
        for (index, pair) in held.windows(2).enumerate() {
            let (packet, _) = encode_packet(&pair[0], &pair[1]);
            if index != 2 {
                assert_eq!(packet, [0, 0, 0, 0]);
            }
            let mut actual = pair[0].clone();
            decode_packet(&mut actual, &packet).unwrap();
            assert_eq!(actual, pair[1]);
        }
        let spatial = options_with(&["--colour-mode", "spatial", "--frame-ticks", "2"]).unwrap();
        assert_eq!(spatial.phase_layout, "checker");
        assert_eq!(display_order(&spatial), "F0,F0,F1,F1,...");
        assert!(colour_pairs(&palette, &spatial).len() > 16);
        assert!(options_with(&["--colour-mode", "hardware"]).is_ok());
        assert!(options_with(&["--colour-mode", "hardware", "--frame-ticks", "0"]).is_err());
        assert!(options_with(&["--colour-mode", "hardware", "--fps", "25"]).is_err());
        assert!(options_with(&["--colour-mode", "hardware", "--phase-repeat", "2"]).is_err());
        assert!(options_with(&["--frame-ticks", "2"]).is_err());
        assert!(options_with(&["--colour-mode", "spatial", "--phase-layout", "uniform"]).is_err());
    }
    #[test]
    fn yliluoma_kd_search_matches_complete_metric_and_preserves_palette_colours() {
        let options = options_with(&["--colour-mode", "hardware", "--dither", "yliluoma"]).unwrap();
        let palette = read_palette(&options.palette).unwrap();
        let mut dither = PositionalDither::new(&palette, false, YLILUOMA_CONTRAST_WEIGHT);
        assert_eq!(dither.candidates.len(), 7576);
        for rgb in [
            [0, 0, 0],
            [255, 255, 255],
            [80, 127, 201],
            [13, 177, 8],
            [189, 49, 107],
            [121, 121, 121],
            [61, 64, 82],
            [130, 99, 44],
        ] {
            let target = lab(rgb.map(linear));
            let query = [target[0], target[1], target[2], 0.0];
            let expected = dither
                .candidates
                .iter()
                .enumerate()
                .min_by(|(a, ca), (b, cb)| {
                    point_distance(query, ca.point)
                        .total_cmp(&point_distance(query, cb.point))
                        .then(a.cmp(b))
                })
                .unwrap()
                .0;
            assert_eq!(dither.plan(rgb), expected);
        }
        for (index, rgb) in palette.iter().enumerate() {
            let chosen = dither.plan(*rgb);
            let plan = dither.candidates[chosen].plan;
            assert_eq!(
                plan,
                MixPlan {
                    a: index as u8,
                    b: index as u8,
                    ratio: 0
                }
            );
        }
        let mut guarded = PositionalDither::new(&palette, true, YLILUOMA_CONTRAST_WEIGHT);
        let plan = guarded.plan([128, 128, 128]);
        assert!(guarded.candidates[plan].neutral);
        assert!(options_with(&["--dither", "yliluoma"]).is_err());
        assert!(options_with(&["--colour-mode", "spatial", "--dither", "yliluoma"]).is_err());
    }
    fn positional_frame(
        dither: &mut PositionalDither,
        frame: &[u8],
        width: usize,
        left: usize,
        top: usize,
    ) -> Vec<u8> {
        let plans = dither.frame_plans(frame);
        plans
            .iter()
            .enumerate()
            .map(|(pos, plan)| {
                dither.candidates[*plan]
                    .plan
                    .colour(pos % width + left, pos / width + top)
            })
            .collect()
    }
    #[test]
    fn yliluoma_is_frame_order_independent_and_pixel_changes_stay_local() {
        let palette = read_palette(&options_with(&[]).unwrap().palette).unwrap();
        let mut frame = (0..128)
            .flat_map(|i| [(i * 13) as u8, (i * 7 + 31) as u8, (i * 29 + 61) as u8])
            .collect::<Vec<_>>();
        frame[37 * 3..38 * 3].fill(0);
        let mut first = PositionalDither::new(&palette, false, YLILUOMA_CONTRAST_WEIGHT);
        let expected = positional_frame(&mut first, &frame, 16, 3, 15);
        let mut other = PositionalDither::new(&palette, false, YLILUOMA_CONTRAST_WEIGHT);
        positional_frame(&mut other, &vec![177; frame.len()], 16, 3, 15);
        assert_eq!(positional_frame(&mut other, &frame, 16, 3, 15), expected);
        assert_eq!(positional_frame(&mut first, &frame, 16, 3, 15), expected);
        frame[37 * 3..38 * 3].fill(255);
        let changed = positional_frame(&mut other, &frame, 16, 3, 15);
        let differences = changed
            .iter()
            .zip(&expected)
            .enumerate()
            .filter_map(|(i, (a, b))| (a != b).then_some(i))
            .collect::<Vec<_>>();
        assert_eq!(differences, [37]);
        assert!(expected.iter().chain(&changed).all(|index| *index < 16));
        let thresholds = (0..64).map(|i| bayer8(i % 8, i / 8)).collect::<Vec<_>>();
        let mut sorted = thresholds.clone();
        sorted.sort_unstable();
        assert_eq!(sorted, (0..64).collect::<Vec<u8>>());
        assert_eq!(&thresholds[..8], &[0, 48, 12, 60, 3, 51, 15, 63]);
        for ratio in [1, 7, 32, 63] {
            let plan = MixPlan { a: 2, b: 8, ratio };
            assert_eq!(
                (0..64)
                    .filter(|i| plan.colour(i % 8 + 3, i / 8 + 15) == 8)
                    .count(),
                ratio as usize
            );
        }
    }
    #[test]
    fn identical_packet_is_only_a_terminator_with_zero_writes() {
        let frame = varied_checker_frame();
        let (packet, stats) = encode_packet(&frame, &frame);
        assert_eq!(packet, [0, 0, 0, 0]);
        assert_eq!(
            (stats.changed, stats.writes(), stats.runs, stats.flip_runs()),
            (0, 0, 0, 0)
        );
    }
    #[test]
    fn packet_roundtrips_literals_fills_alternation_and_region_ends() {
        let before = vec![0u8; FRAME_BYTES];
        let mut after = before.clone();
        after[0..300].fill(0xab);
        for (i, v) in after[450..1000].iter_mut().enumerate() {
            *v = if i & 1 == 0 { 0x12 } else { 0x21 };
        }
        for (i, v) in after[1400..1800].iter_mut().enumerate() {
            *v = (i * 37) as u8;
        }
        after[26620..26628].fill(0x88);
        after[31990..].fill(0x44);
        let (packet, stats) = encode_packet(&before, &after);
        assert!(stats.copied > 0 && stats.filled > 0 && stats.alternating > 0);
        let mut result = before.clone();
        decode_packet(&mut result, &packet).unwrap();
        assert_eq!(result, after);
        let (return_packet, _) = encode_packet(&after, &before);
        decode_packet(&mut result, &return_packet).unwrap();
        assert_eq!(result, before);
    }
    #[test]
    fn decoder_rejects_truncation_bad_tags_and_destinations() {
        let mut frame = vec![0; FRAME_BYTES];
        for bad in [
            vec![],
            vec![0, 0, 0],
            vec![0xff, 0x1f, 2, 0, 1],
            vec![0xff, 0x1f, 1, 10, 0, 0, 0, 0],
            vec![0, 0, 1, 1, 5, 0, 0, 0, 0],
            vec![0xfe, 0x87, 2, 1, 5, 0, 0, 0, 0],
            vec![0, 0, 0, 0, 9],
            vec![0xff, 0x1f, 5, 3, 0x12, 0, 0, 0, 0],
            vec![0xff, 0x1f, 4, 4, 0x12, 0, 0, 0, 0],
        ] {
            assert!(decode_packet(&mut frame, &bad).is_err(), "accepted {bad:?}");
        }
    }
    #[test]
    fn expanded_commands_roundtrip_both_nibbles_in_order() {
        let before = vec![0; FRAME_BYTES];
        let mut after = before.clone();
        for (i, v) in after[0..248].iter_mut().enumerate() {
            let colour = ((i / 4 * 3 + 1) % 16) as u8;
            *v = colour | colour << 4;
        }
        for (i, v) in after[300..552].iter_mut().enumerate() {
            let colour = ((i / 2 * 5 + 3) % 16) as u8;
            *v = colour | colour << 4;
        }
        let (packet, stats) = encode_packet(&before, &after);
        assert!(stats.expanded2 > 0 && stats.expanded4 > 0);
        let mut actual = before;
        decode_packet(&mut actual, &packet).unwrap();
        assert_eq!(actual, after);
        let mut direct = vec![0; FRAME_BYTES];
        decode_packet(&mut direct, &[0xff, 0x1f, 4, 3, 0x82, 0, 0, 0, 0]).unwrap();
        assert_eq!(&direct[..4], &[0x22, 0x22, 0x88, 0x88]);
        decode_packet(&mut direct, &[0xff, 0x1f, 8, 4, 0xf3, 0, 0, 0, 0]).unwrap();
        assert_eq!(
            &direct[..8],
            &[0x33, 0x33, 0x33, 0x33, 0xff, 0xff, 0xff, 0xff]
        );
    }
    #[test]
    fn paired_column_commands_cover_unchanged_prefix_and_eight_row_band() {
        let before = vec![0; FRAME_BYTES];
        let mut after = before.clone();
        for (i, v) in after[..64].iter_mut().enumerate() {
            let colour = (i / 4) as u8;
            *v = colour | colour << 4;
        }
        let first = after[..64].to_vec();
        after[64..128].copy_from_slice(&first);
        after[30720..30724].fill(0x55);
        after[30724..30728].fill(0xcc);
        let last = after[30720..30728].to_vec();
        after[30728..30736].copy_from_slice(&last);
        let (packet, stats) = encode_packet(&before, &after);
        assert!(stats.doubled4 >= 128);
        let mut actual = before;
        decode_packet(&mut actual, &packet).unwrap();
        assert_eq!(actual, after);
        let mut direct = vec![0; FRAME_BYTES];
        decode_packet(&mut direct, &[0xff, 0xb7, 8, 6, 0xc5, 0, 0, 0, 0]).unwrap();
        assert_eq!(
            &direct[30720..30736],
            &[
                0x55, 0x55, 0x55, 0x55, 0xcc, 0xcc, 0xcc, 0xcc, 0x55, 0x55, 0x55, 0x55, 0xcc, 0xcc,
                0xcc, 0xcc
            ]
        );
        assert!(decode_packet(
            &mut direct,
            &[0, 0x20, 64, 5, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]
        )
        .is_err());
    }
    #[test]
    fn checker2_roundtrips_different_pairs_across_framebuffer_regions() {
        let before = vec![0; FRAME_BYTES];
        let mut after = before.clone();
        for (start, length) in [(0, 254), (26600, 24), (26624, 128), (31800, 200)] {
            for i in (0..length).step_by(2) {
                let value = (((i / 2 * 7) % 16) as u8) | ((((i / 2 * 3 + 1) % 16) as u8) << 4);
                after[start + i] = value;
                after[start + i + 1] = value.rotate_left(4);
            }
        }
        let (packet, stats) = encode_packet(&before, &after);
        assert!(stats.checker2 > 0);
        let mut actual = before;
        decode_packet(&mut actual, &packet).unwrap();
        assert_eq!(actual, after);
        let mut direct = vec![0; FRAME_BYTES];
        decode_packet(
            &mut direct,
            &[0xff, 0x1f, 6, 7, 0x1a, 0xf0, 0x37, 0, 0, 0, 0],
        )
        .unwrap();
        assert_eq!(&direct[..6], &[0x1a, 0xa1, 0xf0, 0x0f, 0x37, 0x73]);
        assert!(decode_packet(&mut direct, &[0xff, 0x1f, 3, 7, 0x1a, 0x37, 0, 0, 0, 0]).is_err());
        assert!(decode_packet(&mut direct, &[0xff, 0x1f, 6, 7, 0x1a, 0x37]).is_err());
        assert!(decode_packet(&mut direct, &[0xff, 0x1f, 1, 7, 0x1a, 0, 0, 0, 0]).is_err());
    }
    fn varied_checker_frame() -> Vec<u8> {
        let mut frame = vec![0; FRAME_BYTES];
        for (i, pair) in frame.chunks_exact_mut(2).enumerate() {
            let value = ((i * 7 + 1) % 16) as u8 | (((i * 3 + 2) % 16) as u8) << 4;
            pair[0] = value;
            pair[1] = value.rotate_left(4);
        }
        frame
    }
    #[test]
    fn flip2_has_no_payload_and_coalesces_unchanged_pairs() {
        let mut before = vec![0; FRAME_BYTES];
        // A non-checker pair prevents the aligned page command from covering
        // the following 254-byte span. One interior pair needs no mutation.
        before[..2].copy_from_slice(&[0x12, 0x55]);
        before[2..256].copy_from_slice(&varied_checker_frame()[2..256]);
        before[126..128].fill(0x77);
        let mut after = before.clone();
        for value in &mut after[2..256] {
            *value = value.rotate_left(4);
        }
        let (packet, stats) = encode_packet(&before, &after);
        assert_eq!(packet, [1, 0x20, 254, 8, 0, 0, 0, 0]);
        assert_eq!(
            (stats.flip2, stats.flip2_runs, stats.flip_pages),
            (254, 1, 0)
        );
        assert_eq!(stats.changed, 252);
        assert_eq!(stats.writes(), 254);
        assert_eq!(stats.estimated_cycles(), 190 + 28 * 127 + 80);
        let mut actual = before.clone();
        decode_packet(&mut actual, &packet).unwrap();
        assert_eq!(actual, after);
        decode_packet(&mut actual, &packet).unwrap();
        assert_eq!(actual, before);
    }
    #[test]
    fn flip_pages_roundtrips_both_regions_and_general_changes_fall_back() {
        let mut before = varied_checker_frame();
        before[..2].fill(0x77);
        before[FRAME_BYTES - 2..].fill(0x55);
        let after = before
            .iter()
            .map(|value| value.rotate_left(4))
            .collect::<Vec<_>>();
        let (packet, stats) = encode_packet(&before, &after);
        assert_eq!(packet, [0xff, 0x1f, 104, 9, 0xff, 0xa7, 21, 9, 0, 0, 0, 0]);
        assert_eq!((stats.flip_pages, stats.flip_page_runs), (125, 2));
        assert_eq!(stats.flip_bytes(), FRAME_BYTES);
        assert_eq!(stats.changed, FRAME_BYTES - 4);
        assert_eq!(stats.estimated_cycles(), 180 * 2 + 2980 * 125 + 80);
        let mut actual = before.clone();
        decode_packet(&mut actual, &packet).unwrap();
        assert_eq!(actual, after);
        decode_packet(&mut actual, &packet).unwrap();
        assert_eq!(actual, before);
        let mut general = after;
        general[14] ^= 1;
        let (packet, stats) = encode_packet(&before, &general);
        assert!(stats.flip2 > 0 && stats.flip_pages > 0);
        assert!(stats.flip_bytes() < FRAME_BYTES);
        decode_packet(&mut actual, &packet).unwrap();
        assert_eq!(actual, general);
    }
    #[test]
    fn flip_decoder_rejects_invalid_counts_alignment_boundaries_and_input() {
        let before = varied_checker_frame();
        for bad in [
            vec![0xff, 0x1f, 1, 8, 0, 0, 0, 0],
            vec![0xff, 0x1f, 3, 8, 0, 0, 0, 0],
            vec![0xff, 0x1f, 0, 8, 0, 0, 0, 0],
            vec![0, 0x20, 2, 8, 0, 0, 0, 0],
            vec![1, 0x20, 1, 9, 0, 0, 0, 0],
            vec![0xff, 0x86, 2, 9, 0, 0, 0, 0],
            vec![0xff, 0xbb, 2, 9, 0, 0, 0, 0],
            vec![0xff, 0x1f, 255, 9, 0, 0, 0, 0],
            vec![0xff, 0x1f, 2, 8],
            vec![0xff, 0x1f, 1, 9],
        ] {
            assert!(
                decode_packet(&mut before.clone(), &bad).is_err(),
                "accepted {bad:?}"
            );
        }
        for (tag, count, bytes) in [(8, 254, 254), (9, 1, 256)] {
            for invalid_at in [1, bytes - 1] {
                let mut frame = before.clone();
                frame[invalid_at] ^= 1;
                let invalid_before = frame.clone();
                let packet = [0xff, 0x1f, count, tag, 0, 0, 0, 0];
                assert!(decode_packet(&mut frame, &packet).is_err());
                assert_eq!(frame, invalid_before, "partially applied tag {tag}");
                let target = invalid_before
                    .iter()
                    .map(|v| v.rotate_left(4))
                    .collect::<Vec<_>>();
                assert_eq!(flip_lengths(&invalid_before, &target)[0], invalid_at - 1);
            }
        }
    }
    #[test]
    fn framebuffer_packing_matches_known_coordinates() {
        let mut image = vec![0; WIDTH * HEIGHT];
        image[0] = 3;
        image[1] = 12;
        image[64 * WIDTH + 2] = 7;
        image[128 * WIDTH + 192] = 5;
        image[192 * WIDTH + 318] = 11;
        let bytes = pack(&image);
        assert_eq!(bytes.len(), FRAME_BYTES);
        assert_eq!(bytes[0], 0xc3);
        assert_eq!(bytes[10240 + 64], 7);
        assert_eq!(bytes[20480 + 96 * 64], 5);
        assert_eq!(bytes[30720 + 159 * 8], 11);
    }
    #[test]
    fn temporal_palette_has_123_pairs_and_preserves_black() {
        let palette = read_palette(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../assets/palette.json"),
        )
        .unwrap();
        for (limit, count) in [(0, 23), (4, 55), (8, 87), (16, 123), (32, 136)] {
            let pairs = pairs(&palette, limit, None);
            assert_eq!(pairs.len(), count);
            let black = &pairs[nearest([0, 0, 0], &pairs, false) as usize];
            assert_eq!((black.a, black.b), (0, 0));
        }
    }
    #[test]
    fn relative_rgb_luminance_filter_retains_base_colours_and_expected_safe_pairs() {
        let palette = read_palette(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../assets/palette.json"),
        )
        .unwrap();
        for (limit, count) in [(0.0, 16), (0.01, 17), (0.05, 19)] {
            let filtered = pairs(&palette, 0, Some(limit));
            assert_eq!(filtered.len(), count);
            for base in 0..16 {
                assert!(filtered.iter().any(|p| p.a == base && p.b == base));
            }
            for pair in &filtered {
                assert!(pair.relative_luminance_delta <= limit);
                assert_eq!(PEPTO[pair.a as usize], PEPTO[pair.b as usize]);
                if pair.a == pair.b {
                    assert_eq!(pair.relative_luminance_delta, 0.0);
                }
            }
        }
        assert_eq!(pairs(&palette, 0, None).len(), 23);
        let black = &pairs(&palette, 0, Some(0.0))[0];
        assert_eq!(black.luminance, [0.0, 0.0]);
        assert_eq!(black.relative_luminance_delta, 0.0);
    }
    #[test]
    fn repeated_content_frames_keep_balanced_ab_order_and_native_pal_timing() {
        for repeat in 1..=8 {
            let mut phases = Vec::new();
            append_ab(&mut phases, &0u8, &1u8, repeat);
            append_ab(&mut phases, &2u8, &3u8, repeat);
            assert_eq!(phases.len(), 2 * 2 * repeat);
            for (frame, chunk) in phases.chunks_exact(2 * repeat).enumerate() {
                assert_eq!(
                    chunk.iter().filter(|p| **p == frame as u8 * 2).count(),
                    repeat
                );
                assert_eq!(
                    chunk.iter().filter(|p| **p == frame as u8 * 2 + 1).count(),
                    repeat
                );
                assert!(chunk
                    .chunks_exact(2)
                    .all(|ab| ab == [frame as u8 * 2, frame as u8 * 2 + 1]));
            }
        }
        assert_eq!(phase_order(2), "A0,B0,A0,B0,A1,B1,A1,B1,...");
        let fps = 12.5311355185;
        assert!((phase_fps(fps, 2) - 50.124542074).abs() < 1e-12);
        assert!(validate_native_phase_rate(fps, 2, true).is_ok());
        assert!(validate_native_phase_rate(25.0, 2, true).is_err());
        assert!(validate_native_phase_rate(25.0, 2, false).is_ok());
        assert!(validate_native_phase_rate(25.0, 1, true).is_ok());
        let logical_frames = 20;
        let phases = logical_frames * 2 * 2;
        assert!((logical_frames as f64 / fps - phases as f64 / phase_fps(fps, 2)).abs() < 1e-12);
    }
    #[test]
    fn native_mux_preserves_records_pcm_and_zero_boundaries() {
        let mut raw = Vec::new();
        let mut original = Vec::new();
        for i in 0..200usize {
            let size = 1998;
            let mut record = (size as u16).to_le_bytes().to_vec();
            record.extend(vec![i as u8; size - 4]);
            record.extend([0; 4]);
            if raw.len() % CACHE_BLOCK_BYTES + record.len() + 2 > CACHE_BLOCK_BYTES {
                raw.resize((raw.len() / CACHE_BLOCK_BYTES + 1) * CACHE_BLOCK_BYTES, 0);
            }
            raw.extend(&record);
            original.push(record);
        }
        raw.resize((raw.len() / CACHE_BLOCK_BYTES + 1) * CACHE_BLOCK_BYTES, 0);
        let samples = sample_at_tick(200, 281);
        let pcm: Vec<u8> = (0..samples * 4).map(|i| (i * 37) as u8).collect();
        let mux = mux_cache_blocks(&raw, &pcm, 200, 281, 32768).unwrap();
        assert!(mux.block_records.len() > 2);
        assert_eq!(mux.file.len(), mux.block_records.len() * 65536);
        assert_eq!(mux.record_offsets.len(), 200);
        assert_eq!(mux.audio_lengths.iter().sum::<usize>(), pcm.len());
        let mut audio = Vec::new();
        let mut record = 0;
        for (block_index, (&count, &length)) in
            mux.block_records.iter().zip(&mux.audio_lengths).enumerate()
        {
            let block = &mux.file[block_index * 65536..(block_index + 1) * 65536];
            let mut end = 0;
            for _ in 0..count {
                let expected = &original[record];
                assert_eq!(&block[end..end + expected.len()], expected);
                assert_eq!(mux.record_offsets[record], block_index * 65536 + end);
                end += expected.len();
                record += 1;
            }
            assert!(end + 2 <= 32768);
            assert_eq!(&block[end..end + 2], &[0, 0]);
            assert_eq!(length % 4, 0);
            audio.extend_from_slice(&block[32768..32768 + length]);
        }
        assert_eq!(record, 200);
        assert_eq!(audio, pcm);
        assert_eq!(mux.padded_audio_bytes, 0);
        assert!(mux_cache_blocks(&raw, &pcm, 199, 281, 32768).is_err());
        assert!(mux_cache_blocks(&raw, &pcm, 200, 281, 1000).is_err());
    }
    #[test]
    fn native_mux_pads_short_pcm_and_rejects_crossing_records() {
        let mut raw = vec![0; 65536];
        raw[..2].copy_from_slice(&4u16.to_le_bytes());
        let mux = mux_cache_blocks(&raw, &[1, 2, 3, 4], 1, 281, 32768).unwrap();
        assert_eq!(&mux.file[32768..32772], &[1, 2, 3, 4]);
        assert_eq!(mux.padded_audio_bytes, (sample_at_tick(1, 281) - 1) * 4);
        let mut crossing = vec![0; 65536];
        crossing[..2].copy_from_slice(&65535u16.to_le_bytes());
        assert!(records_from_raw(&crossing).is_err());
        assert!(records_from_raw(&crossing[..65535]).is_err());
    }
    #[test]
    fn jjn_quantisation_preserves_black_without_edge_overrun() {
        let palette = read_palette(
            &PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../assets/palette.json"),
        )
        .unwrap();
        let pairs = pairs(&palette, 16, None);
        let mut cache = HashMap::new();
        for (w, h) in [(1, 1), (1, 5), (5, 1), (3, 4)] {
            let data = vec![0; w * h * 3];
            let indices = quantise(&data, w, h, &pairs, "jjn", false, &mut cache);
            assert!(indices
                .iter()
                .all(|v| pairs[*v as usize].a == 0 && pairs[*v as usize].b == 0));
        }
    }
}

#[cfg(all(feature = "gpu", not(target_arch = "wasm32")))]
pub mod gpu;
mod core;
mod wasm;
pub use core::*;
