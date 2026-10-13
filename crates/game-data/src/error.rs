use std::path::{Path, PathBuf};
use std::sync::Arc;

// The kinds of files a replay is drawn from
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    Rec,
    Lev,
    Lgr,
    ElmaRes,
    AcrossRes,
}

// Why something couldn't be loaded from a game folder
#[derive(Debug, Clone, thiserror::Error)]
pub enum Error {
    // A file that isn't what it should be. `path` is where it is, or what it is when it
    // came from memory, like "the replay".
    #[error("{}: {source}", path.display())]
    Format {
        kind: FileKind,
        path: PathBuf,
        source: elma_formats::Error,
    },
    #[error("cannot read {}: {source}", path.display())]
    Io {
        path: PathBuf,
        source: Arc<std::io::Error>,
    },
    // A file of `kind` that isn't there, described by `what`
    #[error("cannot find {what}")]
    NotFound { kind: FileKind, what: String },
    // A replay's level isn't in lev/: the file name an Elma replay asks for, or None for
    // Across, which doesn't store it
    #[error("{}", match .0 {
        Some(name) => format!("cannot find the level for this replay: {name}"),
        None => "cannot find the level of this replay".into(),
    })]
    MissingLevel(Option<String>),
    // The level was edited after the replay was recorded
    #[error("{level} has changed since {replay} was recorded")]
    LevelChanged { level: String, replay: String },
    // A ghost of another level
    #[error("{ghost} is not a replay of {level}")]
    NotSameLevel { ghost: String, level: String },
    // An Across replay given a level it doesn't start on
    #[error("{} wasn't recorded on {}: the bike doesn't start on its start", replay.display(), level.display())]
    WrongLevel { replay: PathBuf, level: PathBuf },
    #[error("{level} has topology errors")]
    TopologyErrors { level: String },
    // The folder is the other game's
    #[error("this is an {folder} folder, not an {wanted} one")]
    WrongGame {
        folder: &'static str,
        wanted: &'static str,
    },
}

impl Error {
    // The kind of file to blame, if there is one
    pub fn file_kind(&self) -> Option<FileKind> {
        match self {
            Error::Format { kind, .. } | Error::NotFound { kind, .. } => Some(*kind),
            Error::MissingLevel(_) | Error::WrongLevel { .. } | Error::TopologyErrors { .. } => {
                Some(FileKind::Lev)
            }
            Error::LevelChanged { .. } | Error::NotSameLevel { .. } => Some(FileKind::Rec),
            Error::Io { .. } | Error::WrongGame { .. } => None,
        }
    }
}

// Maps a format error of the file of `kind` at `path`
pub(crate) fn format(
    kind: FileKind,
    path: impl Into<PathBuf>,
) -> impl FnOnce(elma_formats::Error) -> Error {
    let path = path.into();
    move |source| Error::Format { kind, path, source }
}

pub(crate) fn read(path: &Path) -> Result<Vec<u8>, Error> {
    std::fs::read(path).map_err(|e| Error::Io {
        path: path.to_path_buf(),
        source: Arc::new(e),
    })
}

pub(crate) fn not_found(kind: FileKind, what: impl Into<String>) -> Error {
    Error::NotFound {
        kind,
        what: what.into(),
    }
}
