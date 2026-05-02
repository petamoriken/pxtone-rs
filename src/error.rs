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
  OggVorbis(lewton::VorbisError),
  /// Audio format conversion failed.
  PcmConvert,
  /// A required chunk header was not found.
  InvalidCode,
  /// The requested operation is not permitted for this file.
  AntiOperation,
  /// The instrument table is full; no more instruments can be added.
  WoiceFull,
  /// The service was not properly initialized before use.
  Init,
}

impl std::fmt::Display for PxtoneError {
  fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
    match self {
      PxtoneError::Io(e) => write!(f, "I/O error: {e}"),
      PxtoneError::UnknownFormat => write!(f, "Unknown format"),
      PxtoneError::NewFormat => write!(f, "Format version too new"),
      PxtoneError::BrokenFile => write!(f, "Broken file"),
      PxtoneError::Unsupported(s) => write!(f, "Unsupported: {s}"),
      PxtoneError::OggVorbis(e) => write!(f, "Ogg Vorbis error: {e}"),
      PxtoneError::PcmConvert => write!(f, "PCM conversion error"),
      PxtoneError::InvalidCode => write!(f, "Invalid code"),
      PxtoneError::AntiOperation => write!(f, "Anti-operation (edit forbidden)"),
      PxtoneError::WoiceFull => write!(f, "Woice table full"),
      PxtoneError::Init => write!(f, "Initialization error"),
    }
  }
}

impl std::error::Error for PxtoneError {
  fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
    match self {
      PxtoneError::Io(e) => Some(e),
      PxtoneError::OggVorbis(e) => Some(e),
      _ => None,
    }
  }
}

impl From<io::Error> for PxtoneError {
  fn from(e: io::Error) -> Self {
    PxtoneError::Io(e)
  }
}
