use std::env;
use std::fs;
use std::path::PathBuf;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AppSettings {
    pub query: String,
    pub publisher_filter: String,
    pub uninstall_filter: String,
    pub page_size: usize,
    pub include_system_components: bool,
    pub show_no_remove: bool,
    pub show_details: bool,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            query: String::new(),
            publisher_filter: String::new(),
            uninstall_filter: "selectable".to_owned(),
            page_size: 50,
            include_system_components: false,
            show_no_remove: true,
            show_details: false,
        }
    }
}

impl AppSettings {
    pub fn load() -> Self {
        let Ok(contents) = fs::read_to_string(settings_path()) else {
            return Self::default();
        };

        let mut settings = Self::default();
        for line in contents.lines() {
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };

            match key {
                "query" => settings.query = value.to_owned(),
                "publisher_filter" => settings.publisher_filter = value.to_owned(),
                "uninstall_filter" => settings.uninstall_filter = value.to_owned(),
                "page_size" => settings.page_size = parse_page_size(value),
                "include_system_components" => {
                    settings.include_system_components = parse_bool(value);
                }
                "show_no_remove" => settings.show_no_remove = parse_bool(value),
                "show_details" => settings.show_details = parse_bool(value),
                _ => {}
            }
        }

        settings
    }

    pub fn save(&self) -> Result<(), String> {
        let path = settings_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }

        let contents = format!(
            "query={}\npublisher_filter={}\nuninstall_filter={}\npage_size={}\ninclude_system_components={}\nshow_no_remove={}\nshow_details={}\n",
            self.query.replace('\r', " ").replace('\n', " "),
            self.publisher_filter.replace('\r', " ").replace('\n', " "),
            self.uninstall_filter,
            self.page_size,
            self.include_system_components,
            self.show_no_remove,
            self.show_details
        );
        fs::write(path, contents).map_err(|error| error.to_string())
    }
}

fn settings_path() -> PathBuf {
    env::var_os("APPDATA")
        .or_else(|| env::var_os("LOCALAPPDATA"))
        .map(PathBuf::from)
        .unwrap_or_else(|| env::current_dir().unwrap_or_else(|_| PathBuf::from(".")))
        .join("WinAppDeleteController")
        .join("settings.ini")
}

fn parse_bool(value: &str) -> bool {
    matches!(value.trim(), "true" | "1" | "yes" | "on")
}

fn parse_page_size(value: &str) -> usize {
    match value.trim().parse::<usize>().ok() {
        Some(25 | 50 | 100 | 200) => value.trim().parse().unwrap_or(50),
        _ => 50,
    }
}
