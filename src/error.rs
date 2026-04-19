use std::io;

#[derive(Debug)]
pub enum PxtoneError {
    /// I/O エラー
    Io(io::Error),
    /// ファイルフォーマットが不明
    UnknownFormat,
    /// ファイルフォーマットが新しすぎる
    NewFormat,
    /// ファイルが壊れている
    BrokenFile,
    /// サポートされていないフォーマット
    Unsupported(&'static str),
    /// Ogg Vorbis デコードエラー
    OggVorbis(String),
    /// PCM 変換エラー
    PcmConvert,
    /// 不正なコード
    InvalidCode,
    /// 不正なデータ
    InvalidData,
    /// アンチオペレーション (編集禁止)
    AntiOperation,
    /// ビートクロック拒否
    DenyBeatClock,
    /// ウォイスが満杯
    WoiceFull,
    /// イベント数超過
    TooMuchEvent,
    /// x3x チューニングエラー
    X3xTuning,
    /// x1x フォーマットは無視
    X1xIgnore,
    /// 初期化エラー
    Init,
    /// 一般的なエラー
    Other(&'static str),
}

impl std::fmt::Display for PxtoneError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PxtoneError::Io(e)               => write!(f, "I/O error: {e}"),
            PxtoneError::UnknownFormat       => write!(f, "Unknown format"),
            PxtoneError::NewFormat           => write!(f, "Format version too new"),
            PxtoneError::BrokenFile          => write!(f, "Broken file"),
            PxtoneError::Unsupported(s)      => write!(f, "Unsupported: {s}"),
            PxtoneError::OggVorbis(s)        => write!(f, "Ogg Vorbis error: {s}"),
            PxtoneError::PcmConvert          => write!(f, "PCM conversion error"),
            PxtoneError::InvalidCode         => write!(f, "Invalid code"),
            PxtoneError::InvalidData         => write!(f, "Invalid data"),
            PxtoneError::AntiOperation       => write!(f, "Anti-operation (edit forbidden)"),
            PxtoneError::DenyBeatClock       => write!(f, "Deny beat clock"),
            PxtoneError::WoiceFull           => write!(f, "Woice table full"),
            PxtoneError::TooMuchEvent        => write!(f, "Too many events"),
            PxtoneError::X3xTuning           => write!(f, "x3x tuning error"),
            PxtoneError::X1xIgnore           => write!(f, "x1x format ignored"),
            PxtoneError::Init                => write!(f, "Initialization error"),
            PxtoneError::Other(s)            => write!(f, "{s}"),
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
