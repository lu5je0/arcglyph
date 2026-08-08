#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Desktop {
    Kde,
    Gnome,
    Other,
}

pub fn detect() -> Desktop {
    let v = std::env::var("XDG_CURRENT_DESKTOP")
        .unwrap_or_default()
        .to_ascii_lowercase();
    if v.contains("gnome") {
        Desktop::Gnome
    } else if v.contains("kde") {
        Desktop::Kde
    } else {
        Desktop::Other
    }
}
