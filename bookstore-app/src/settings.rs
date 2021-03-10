use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use unicase::UniCase;

#[derive(Copy, Clone)]
pub enum Color {
    Black,
    Red,
    Green,
    Yellow,
    Blue,
    Magenta,
    Cyan,
    Gray,
    DarkGray,
    LightRed,
    LightGreen,
    LightYellow,
    LightBlue,
    LightMagenta,
    LightCyan,
    White,
}

/// Provides terminal UI settings.
pub struct Settings {
    pub interface_style: InterfaceStyle,
    pub columns: Vec<String>,
    pub sort_settings: SortSettings,
    pub navigation_settings: NavigationSettings,
    pub database_settings: DatabaseSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            interface_style: InterfaceStyle::default(),
            columns: vec![String::from("Title"), String::from("Authors")],
            sort_settings: SortSettings::default(),
            navigation_settings: NavigationSettings::default(),
            database_settings: Default::default(),
        }
    }
}

#[derive(Clone, Copy)]
pub struct InterfaceStyle {
    pub selected_fg: Color,
    pub selected_bg: Color,
    pub edit_fg: Color,
    pub edit_bg: Color,
    pub cursor_fg: Color,
    pub cursor_bg: Color,
}

impl Default for InterfaceStyle {
    fn default() -> Self {
        InterfaceStyle {
            selected_fg: Color::White,
            selected_bg: Color::Blue,
            edit_fg: Color::White,
            edit_bg: Color::LightBlue,
            cursor_fg: Color::Black,
            cursor_bg: Color::White,
        }
    }
}

// TODO: Consider removing sort settings from settings? Functionality is somewhat
//  replicated by IndexMap
#[derive(Debug, Clone)]
pub struct SortSettings {
    pub columns: Box<[(UniCase<String>, bool)]>,
    pub is_sorted: bool,
}

impl Default for SortSettings {
    fn default() -> Self {
        SortSettings {
            columns: vec![].into_boxed_slice(),
            is_sorted: false,
        }
    }
}

#[derive(Debug, Copy, Clone)]
pub struct NavigationSettings {
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

#[derive(Debug, Clone)]
pub struct DatabaseSettings {
    pub path: PathBuf,
}

impl Default for DatabaseSettings {
    fn default() -> Self {
        DatabaseSettings {
            path: dirs::data_local_dir().map_or_else(
                || PathBuf::from("."),
                |mut p| {
                    p.push("bookstore/bookstore.db");
                    p
                },
            ),
        }
    }
}

fn str_to_color_or<S: AsRef<str>>(s: S, default: Color) -> Color {
    match s.as_ref().to_ascii_lowercase().as_str() {
        "black" => Color::Black,
        "red" => Color::Red,
        "green" => Color::Green,
        "yellow" => Color::Yellow,
        "blue" => Color::Blue,
        "magenta" => Color::Magenta,
        "cyan" => Color::Cyan,
        "gray" => Color::Gray,
        "darkgray" => Color::DarkGray,
        "lightred" => Color::LightRed,
        "lightgreen" => Color::LightGreen,
        "lightyellow" => Color::LightYellow,
        "lightblue" => Color::LightBlue,
        "lightmagenta" => Color::LightMagenta,
        "lightcyan" => Color::LightCyan,
        "white" => Color::White,
        _ => default,
    }
}

fn color_to_string(c: Color) -> String {
    match c {
        Color::Black => "Black",
        Color::Red => "Red",
        Color::Green => "Green",
        Color::Yellow => "Yellow",
        Color::Blue => "Blue",
        Color::Magenta => "Magenta",
        Color::Cyan => "Cyan",
        Color::Gray => "Gray",
        Color::DarkGray => "DarkGray",
        Color::LightRed => "LightRed",
        Color::LightGreen => "LightGreen",
        Color::LightYellow => "LightYellow",
        Color::LightBlue => "LightBlue",
        Color::LightMagenta => "LightMagenta",
        Color::LightCyan => "LightCyan",
        Color::White => "White",
    }
    .to_string()
}

#[derive(Debug, Serialize, Deserialize)]
struct TomlSettings {
    colors: Option<TomlColors>,
    layout: Option<TomlColumns>,
    sorting: Option<TomlSort>,
    navigation: Option<TomlNavigation>,
    database: Option<TomlDatabase>,
}

#[derive(Debug, Serialize, Deserialize)]
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
            // TODO: Add cursor styling to TOML
            ..Default::default()
        }
    }
}

impl From<InterfaceStyle> for TomlColors {
    fn from(is: InterfaceStyle) -> Self {
        TomlColors {
            selected_fg: Some(color_to_string(is.selected_fg)),
            selected_bg: Some(color_to_string(is.selected_bg)),
            edit_fg: Some(color_to_string(is.edit_fg)),
            edit_bg: Some(color_to_string(is.edit_bg)),
        }
    }
}

impl TomlColors {
    pub fn selected_bg(&self) -> Color {
        if let Some(color) = &self.selected_bg {
            str_to_color_or(color, Color::LightBlue)
        } else {
            Color::LightBlue
        }
    }

    pub fn selected_fg(&self) -> Color {
        if let Some(color) = &self.selected_fg {
            str_to_color_or(color, Color::White)
        } else {
            Color::White
        }
    }

    pub fn edit_bg(&self) -> Color {
        if let Some(color) = &self.edit_bg {
            str_to_color_or(color, Color::Blue)
        } else {
            Color::Blue
        }
    }

    pub fn edit_fg(&self) -> Color {
        if let Some(color) = &self.edit_fg {
            str_to_color_or(color, Color::White)
        } else {
            Color::White
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
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
            vec![String::from("Title"), String::from("Authors")]
        }
    }
}

impl From<Vec<String>> for TomlColumns {
    fn from(vs: Vec<String>) -> Self {
        TomlColumns { columns: Some(vs) }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct TomlSort {
    columns: Option<Vec<(String, Option<bool>)>>,
}

impl Default for TomlSort {
    fn default() -> Self {
        TomlSort { columns: None }
    }
}

impl From<TomlSort> for SortSettings {
    fn from(t: TomlSort) -> Self {
        let columns = t.columns.unwrap_or_default();
        let columns: Vec<_> = columns
            .into_iter()
            .map(|(s, r)| (UniCase::new(s), r.unwrap_or(false)))
            .collect();
        SortSettings {
            is_sorted: columns.is_empty(),
            columns: columns.into_boxed_slice(),
        }
    }
}

impl From<SortSettings> for TomlSort {
    fn from(s: SortSettings) -> Self {
        TomlSort {
            columns: Some(
                s.columns
                    .into_vec()
                    .into_iter()
                    .map(|(c, r)| (c.into_inner(), Some(r)))
                    .collect(),
            ),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
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

impl From<NavigationSettings> for TomlNavigation {
    fn from(n: NavigationSettings) -> Self {
        TomlNavigation {
            scroll: Some(n.scroll),
            inverted: Some(n.inverted),
        }
    }
}

#[derive(Debug, Serialize, Deserialize)]
struct TomlDatabase {
    file: Option<PathBuf>,
}

impl Default for TomlDatabase {
    fn default() -> Self {
        TomlDatabase { file: None }
    }
}

impl From<TomlDatabase> for DatabaseSettings {
    fn from(t: TomlDatabase) -> Self {
        DatabaseSettings {
            path: t.file.unwrap_or_else(|| Self::default().path),
        }
    }
}

impl From<DatabaseSettings> for TomlDatabase {
    fn from(n: DatabaseSettings) -> Self {
        TomlDatabase { file: Some(n.path) }
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
    pub fn open<P: AsRef<Path>>(file: P) -> Result<Self, std::io::Error> {
        let f = std::fs::read_to_string(file.as_ref())?;
        let value: TomlSettings = toml::from_str(f.as_str())?;
        Ok(Settings {
            interface_style: value.colors.unwrap_or_default().into(),
            columns: value.layout.unwrap_or_default().into(),
            sort_settings: value.sorting.unwrap_or_default().into(),
            navigation_settings: value.navigation.unwrap_or_default().into(),
            database_settings: value.database.unwrap_or_default().into(),
        })
    }

    pub fn write<P: AsRef<Path>>(&self, file: P) -> Result<(), std::io::Error> {
        let value = TomlSettings {
            colors: Some(self.interface_style.clone().into()),
            layout: Some(self.columns.clone().into()),
            sorting: Some(self.sort_settings.clone().into()),
            navigation: Some(self.navigation_settings.clone().into()),
            database: Some(self.database_settings.clone().into()),
        };
        std::fs::write(
            file,
            toml::to_string(&value)
                .expect("Unknown error when serializing settings.")
                .as_bytes(),
        )
    }
}
