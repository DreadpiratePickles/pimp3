use std::fmt;

/// Everything that can go wrong decoding an MP3 stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DecodeError {
    /// The bytes did not contain a recognisable MPEG audio stream.
    NotMpegAudio,
    /// The container was readable but held no audio track.
    NoAudioTrack,
    /// The stream declares parameters this build cannot handle.
    UnsupportedStream { detail: String },
    /// A frame was damaged. Decoding can usually continue past it.
    CorruptFrame { detail: String },
    /// A seek target outside the stream, or a stream that cannot seek.
    Seek { detail: String },
    /// Underlying I/O over the in-memory buffer failed.
    Io { detail: String },
}

impl fmt::Display for DecodeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NotMpegAudio => write!(f, "input is not an MPEG audio stream"),
            Self::NoAudioTrack => write!(f, "stream contains no audio track"),
            Self::UnsupportedStream { detail } => write!(f, "unsupported stream: {detail}"),
            Self::CorruptFrame { detail } => write!(f, "corrupt frame: {detail}"),
            Self::Seek { detail } => write!(f, "seek failed: {detail}"),
            Self::Io { detail } => write!(f, "i/o error: {detail}"),
        }
    }
}

impl std::error::Error for DecodeError {}

pub type Result<T> = std::result::Result<T, DecodeError>;
