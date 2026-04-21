use std::io;

#[derive(Debug)]
pub enum PxtoneError {
  /// I/O error
  Io(io::Error),
  /// Unknown file format
  UnknownFormat,
  /// File format version is too new
  NewFormat,
  /// File is corrupted
  BrokenFile,
  /// Unsupported format
  Unsupported(&'static str),
  /// Ogg Vorbis decode error
  OggVorbis(String),
  /// PCM conversion error
  PcmConvert,
  /// Invalid code
  InvalidCode,
  /// Invalid data
  InvalidData,
  /// Anti-operation (editing forbidden)
  AntiOperation,
  /// Beat clock denied
  DenyBeatClock,
  /// Woice table is full
  WoiceFull,
  /// Too many events
  TooMuchEvent,
  /// x3x tuning error
  X3xTuning,
  /// x1x format is ignored
  X1xIgnore,
  /// Initialization error
  Init,
  /// General error
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
