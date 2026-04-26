use std::io;

/// Error type returned by pxtone operations.
#[derive(Debug)]
pub enum PxtoneError {
  /// An I/O error occurred while reading the file.
  Io(io::Error),
  /// The file is not a recognized pxtone format.
  UnknownFormat,
  /// The file was created by a newer version and cannot be decoded.
  NewFormat,
  /// The file is corrupted or truncated.
  BrokenFile,
  /// The format is valid but not supported by this decoder.
  Unsupported(&'static str),
  /// The embedded Ogg Vorbis audio could not be decoded.
  OggVorbis(String),
  /// Audio format conversion failed.
  PcmConvert,
  /// A required chunk header was not found.
  InvalidCode,
  /// A field value is out of the expected range.
  InvalidData,
  /// The requested operation is not permitted for this file.
  AntiOperation,
  /// The beat clock value is not accepted.
  DenyBeatClock,
  /// The instrument table is full; no more instruments can be added.
  WoiceFull,
  /// The event list has reached its maximum capacity.
  TooMuchEvent,
  /// The tuning data is incompatible with this format version.
  X3xTuning,
  /// This file uses a legacy format (version 1.x) that is not supported.
  X1xIgnore,
  /// The service was not properly initialized before use.
  Init,
  /// An unspecified error occurred.
  Other(&'static str),
}

impl std::fmt::Display for PxtoneError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      PxtoneError::Io(e) => write!(f, "I/O error: {e}"),
      PxtoneError::UnknownFormat => write!(f, "Unknown format"),
      PxtoneError::NewFormat => write!(f, "Format version too new"),
      PxtoneError::BrokenFile => write!(f, "Broken file"),
      PxtoneError::Unsupported(s) => write!(f, "Unsupported: {s}"),
      PxtoneError::OggVorbis(s) => write!(f, "Ogg Vorbis error: {s}"),
      PxtoneError::PcmConvert => write!(f, "PCM conversion error"),
      PxtoneError::InvalidCode => write!(f, "Invalid code"),
      PxtoneError::InvalidData => write!(f, "Invalid data"),
      PxtoneError::AntiOperation => write!(f, "Anti-operation (edit forbidden)"),
      PxtoneError::DenyBeatClock => write!(f, "Deny beat clock"),
      PxtoneError::WoiceFull => write!(f, "Woice table full"),
      PxtoneError::TooMuchEvent => write!(f, "Too many events"),
      PxtoneError::X3xTuning => write!(f, "x3x tuning error"),
      PxtoneError::X1xIgnore => write!(f, "x1x format ignored"),
      PxtoneError::Init => write!(f, "Initialization error"),
      PxtoneError::Other(s) => write!(f, "{s}"),
    }
  }
}

impl std::error::Error for PxtoneError {
  fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
    match self {
      PxtoneError::Io(e) => Some(e),
      _ => None,
    }
  }
}

impl From<io::Error> for PxtoneError {
  fn from(e: io::Error) -> Self {
    PxtoneError::Io(e)
  }
}
