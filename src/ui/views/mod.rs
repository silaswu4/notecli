pub mod channels;
pub mod piano;
pub mod playlist;
pub mod mixer;
pub mod browser;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Channels,
    Piano,
    Playlist,
    Mixer,
    Browser,
}

impl View {
    pub fn label(&self) -> &'static str {
        match self {
            View::Channels => "channels",
            View::Piano => "piano",
            View::Playlist => "playlist",
            View::Mixer => "mixer",
            View::Browser => "browser",
        }
    }

    pub fn from_key(k: char) -> Option<Self> {
        match k {
            '1' => Some(View::Channels),
            '2' => Some(View::Piano),
            '3' => Some(View::Playlist),
            '4' => Some(View::Mixer),
            '5' => Some(View::Browser),
            _ => None,
        }
    }
}
