//! `pimp3` — decode an MP3 to 32-bit float WAV, or report what is in it.

mod wav;

use pimp3_core::Mp3Decoder;
use std::io::{Read, Write};
use std::path::PathBuf;
use std::process::ExitCode;

const USAGE: &str = "\
pimp3 — MP3 decoder

USAGE:
    pimp3 [options] [input.mp3]

ARGS:
    input               MP3 to decode. Reads stdin when omitted or '-'.

OPTIONS:
    -o, --output <file> Write 32-bit float WAV here. Writes stdout when omitted.
    -i, --info          Print stream parameters and exit.
        --seek <secs>   Discard everything before this position.
        --duration <s>  Stop after this many seconds of audio.
    -h, --help          Show this message.
";

struct Args {
    input: Option<PathBuf>,
    output: Option<PathBuf>,
    info: bool,
    seek_seconds: Option<f64>,
    duration_seconds: Option<f64>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        input: None,
        output: None,
        info: false,
        seek_seconds: None,
        duration_seconds: None,
    };
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        let mut number = |name: &str| -> Result<f64, String> {
            it.next()
                .ok_or_else(|| format!("{name} needs a value"))?
                .parse()
                .map_err(|_| format!("{name} needs a number"))
        };
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            "-i" | "--info" => args.info = true,
            "-o" | "--output" => {
                args.output = Some(PathBuf::from(it.next().ok_or("--output needs a path")?))
            }
            "--seek" => args.seek_seconds = Some(number("--seek")?),
            "--duration" => args.duration_seconds = Some(number("--duration")?),
            "-" => args.input = None,
            other if other.starts_with('-') => return Err(format!("unknown option '{other}'")),
            other => args.input = Some(PathBuf::from(other)),
        }
    }
    Ok(args)
}

fn read_input(path: &Option<PathBuf>) -> Result<Vec<u8>, String> {
    match path {
        Some(p) => std::fs::read(p).map_err(|e| format!("cannot read {}: {e}", p.display())),
        None => {
            let mut buf = Vec::new();
            std::io::stdin()
                .read_to_end(&mut buf)
                .map_err(|e| format!("cannot read stdin: {e}"))?;
            Ok(buf)
        }
    }
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    let input = read_input(&args.input)?;
    let mut decoder = Mp3Decoder::new(input).map_err(|e| e.to_string())?;
    let info = decoder.info();

    if args.info {
        println!("sample rate : {} Hz", info.sample_rate_hz);
        println!("channels    : {}", info.channel_count);
        match info.duration_seconds() {
            Some(seconds) => println!("duration    : {seconds:.3} s"),
            None => println!("duration    : unknown (no length header)"),
        }
        return Ok(());
    }

    if let Some(seconds) = args.seek_seconds {
        decoder.seek(seconds).map_err(|e| e.to_string())?;
    }

    let frame_limit = args
        .duration_seconds
        .map(|seconds| (seconds * f64::from(info.sample_rate_hz)) as usize)
        .unwrap_or(usize::MAX);

    let mut samples = Vec::new();
    let channels = usize::from(info.channel_count).max(1);
    while let Some(chunk) = decoder.decode_next().map_err(|e| e.to_string())? {
        samples.extend_from_slice(&chunk.samples);
        if samples.len() / channels >= frame_limit {
            samples.truncate(frame_limit * channels);
            break;
        }
    }

    if decoder.dropped_frames() > 0 {
        eprintln!(
            "pimp3: lost {} frame(s) of audio to stream damage",
            decoder.dropped_frames()
        );
    }

    let wav = wav::encode(&samples, info.sample_rate_hz, info.channel_count);
    match &args.output {
        Some(path) => std::fs::write(path, &wav)
            .map_err(|e| format!("cannot write {}: {e}", path.display()))?,
        None => std::io::stdout()
            .write_all(&wav)
            .map_err(|e| format!("cannot write stdout: {e}"))?,
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("pimp3: {message}");
            ExitCode::FAILURE
        }
    }
}
