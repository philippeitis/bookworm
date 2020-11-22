use std::path::Path;

use tui::style::Color;

use serde::Deserialize;

use unicase::UniCase;

/// Provides terminal UI settings.
pub(crate) struct Settings {
    pub interface_style: InterfaceStyle,
    pub columns: Vec<String>,
    pub sort_settings: SortSettings,
    pub navigation_settings: NavigationSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            interface_style: InterfaceStyle::default(),
            columns: vec!["Title".to_string(), "Authors".to_string()],
            sort_settings: SortSettings::default(),
            navigation_settings: NavigationSettings::default(),
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct InterfaceStyle {
    pub selected_fg: Color,
    pub selected_bg: Color,
    pub edit_fg: Color,
    pub edit_bg: Color,
}

impl Default for InterfaceStyle {
    fn default() -> Self {
        InterfaceStyle {
            selected_fg: Color::White,
            selected_bg: Color::LightBlue,
            edit_fg: Color::White,
            edit_bg: Color::Blue,
        }
    }
}

#[derive(Debug)]
pub(crate) struct SortSettings {
    pub column: UniCase<String>,
    pub is_sorted: bool,
    pub reverse: bool,
}

impl Default for SortSettings {
    fn default() -> Self {
        SortSettings {
            column: UniCase::new("".to_string()),
            is_sorted: false,
            reverse: false,
        }
    }
}

#[derive(Debug)]
pub(crate) struct NavigationSettings {
    pub scroll: usize,
    pub inverted: bool,
}

impl Default for NavigationSettings {
    fn default() -> Self {
        NavigationSettings {
            scroll: 0,
            inverted: false,
        }
    }
}

fn str_to_color<S: AsRef<str>>(s: S) -> Option<Color> {
    match s.as_ref().to_ascii_lowercase().as_str() {
        "black" => Some(Color::Black),
        "red" => Some(Color::Red),
        "green" => Some(Color::Green),
        "yellow" => Some(Color::Yellow),
        "blue" => Some(Color::Blue),
        "magenta" => Some(Color::Magenta),
        "cyan" => Some(Color::Cyan),
        "gray" => Some(Color::Gray),
        "darkgray" => Some(Color::DarkGray),
        "lightred" => Some(Color::LightRed),
        "lightgreen" => Some(Color::LightGreen),
        "lightyellow" => Some(Color::LightYellow),
        "lightblue" => Some(Color::LightBlue),
        "lightmagenta" => Some(Color::LightMagenta),
        "lightcyan" => Some(Color::LightCyan),
        "white" => Some(Color::White),
        _ => None,
    }
}

#[derive(Debug, Deserialize)]
struct TomlSettings {
    colors: Option<TomlColors>,
    layout: Option<TomlColumns>,
    sorting: Option<TomlSort>,
    navigation: Option<TomlNavigation>,
}

#[derive(Debug, Deserialize)]
struct TomlColors {
    selected_fg: Option<String>,
    selected_bg: Option<String>,
    edit_fg: Option<String>,
    edit_bg: Option<String>,
}

impl Default for TomlColors {
    fn default() -> Self {
        TomlColors {
            selected_fg: None,
            selected_bg: None,
            edit_fg: None,
            edit_bg: None,
        }
    }
}

impl From<TomlColors> for InterfaceStyle {
    fn from(t: TomlColors) -> Self {
        InterfaceStyle {
            selected_fg: t.selected_fg(),
            selected_bg: t.selected_bg(),
            edit_fg: t.edit_fg(),
            edit_bg: t.edit_bg(),
        }
    }
}

impl TomlColors {
    fn selected_bg(&self) -> Color {
        if let Some(color) = &self.selected_bg {
            str_to_color(color).unwrap_or(Color::LightBlue)
        } else {
            Color::LightBlue
        }
    }

    fn selected_fg(&self) -> Color {
        if let Some(color) = &self.selected_fg {
            str_to_color(color).unwrap_or(Color::White)
        } else {
            Color::White
        }
    }

    fn edit_bg(&self) -> Color {
        if let Some(color) = &self.edit_bg {
            str_to_color(color).unwrap_or(Color::Blue)
        } else {
            Color::Blue
        }
    }

    fn edit_fg(&self) -> Color {
        if let Some(color) = &self.edit_fg {
            str_to_color(color).unwrap_or(Color::White)
        } else {
            Color::White
        }
    }
}

#[derive(Debug, Deserialize)]
struct TomlColumns {
    columns: Option<Vec<String>>,
}

impl Default for TomlColumns {
    fn default() -> Self {
        TomlColumns { columns: None }
    }
}

impl From<TomlColumns> for Vec<String> {
    fn from(t: TomlColumns) -> Self {
        if let Some(s) = t.columns {
            s
        } else {
            vec!["Title".to_string(), "Authors".to_string()]
        }
    }
}

#[derive(Debug, Deserialize)]
struct TomlSort {
    column: Option<String>,
    reverse: Option<bool>,
}

impl Default for TomlSort {
    fn default() -> Self {
        TomlSort {
            column: None,
            reverse: None,
        }
    }
}

impl From<TomlSort> for SortSettings {
    fn from(t: TomlSort) -> Self {
        SortSettings {
            column: UniCase::new(t.column.clone().unwrap_or_else(|| "".to_string())),
            is_sorted: t.column.is_none(),
            reverse: t.reverse.unwrap_or(false),
        }
    }
}

#[derive(Debug, Deserialize)]
struct TomlNavigation {
    scroll: Option<usize>,
    inverted: Option<bool>,
}

impl Default for TomlNavigation {
    fn default() -> Self {
        TomlNavigation {
            scroll: Some(5),
            inverted: Some(cfg!(macos)),
        }
    }
}

impl From<TomlNavigation> for NavigationSettings {
    fn from(t: TomlNavigation) -> Self {
        NavigationSettings {
            scroll: t.scroll.unwrap_or(5),
            inverted: t.inverted.unwrap_or(cfg!(macos)),
        }
    }
}

impl Settings {
    /// Opens the settings at the provided location, and fills in missing settings from default
    /// values.
    ///
    /// # Arguments
    ///
    /// * ` file ` - The path to the settings.
    ///
    /// # Error
    /// Errors if reading the file or parsing the settings fails.
    pub(crate) fn open<P: AsRef<Path>>(file: P) -> Result<Self, std::io::Error> {
        let f = std::fs::read_to_string(file.as_ref())?;
        let value: TomlSettings = toml::from_str(f.as_str())?;
        Ok(Settings {
            interface_style: value.colors.unwrap_or_default().into(),
            columns: value.layout.unwrap_or_default().into(),
            sort_settings: value.sorting.unwrap_or_default().into(),
            navigation_settings: value.navigation.unwrap_or_default().into(),
        })
    }
}
