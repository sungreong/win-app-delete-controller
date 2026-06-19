#[derive(Clone, Debug)]
pub struct InstalledApp {
    pub id: String,
    pub display_name: String,
    pub publisher: Option<String>,
    pub version: Option<String>,
    pub install_location: Option<String>,
    pub estimated_size_kb: Option<u32>,
    pub install_date: Option<String>,
    pub uninstall_string: String,
    pub registry_path: String,
    pub source_hive: String,
    pub is_system_component: bool,
    pub no_remove: bool,
    search_blob: String,
}

impl InstalledApp {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        id: String,
        display_name: String,
        publisher: Option<String>,
        version: Option<String>,
        install_location: Option<String>,
        estimated_size_kb: Option<u32>,
        install_date: Option<String>,
        uninstall_string: String,
        registry_path: String,
        source_hive: String,
        is_system_component: bool,
        no_remove: bool,
    ) -> Self {
        let search_blob = [
            display_name.as_str(),
            publisher.as_deref().unwrap_or_default(),
            version.as_deref().unwrap_or_default(),
            install_location.as_deref().unwrap_or_default(),
            install_date.as_deref().unwrap_or_default(),
            &estimated_size_kb
                .map(|size| size.to_string())
                .unwrap_or_default(),
            registry_path.as_str(),
        ]
        .join(" ")
        .to_lowercase();

        Self {
            id,
            display_name,
            publisher,
            version,
            install_location,
            estimated_size_kb,
            install_date,
            uninstall_string,
            registry_path,
            source_hive,
            is_system_component,
            no_remove,
            search_blob,
        }
    }

    pub fn matches_terms(&self, terms: &[String]) -> bool {
        terms.iter().all(|term| self.search_blob.contains(term))
    }

    pub fn publisher_text(&self) -> &str {
        self.publisher.as_deref().unwrap_or("-")
    }

    pub fn version_text(&self) -> &str {
        self.version.as_deref().unwrap_or("-")
    }

    pub fn location_text(&self) -> &str {
        self.install_location.as_deref().unwrap_or("-")
    }

    pub fn size_text(&self) -> String {
        match self.estimated_size_kb {
            Some(size_kb) if size_kb >= 1024 * 1024 => {
                format!("{:.1} GB", size_kb as f32 / 1024.0 / 1024.0)
            }
            Some(size_kb) if size_kb >= 1024 => {
                format!("{:.0} MB", size_kb as f32 / 1024.0)
            }
            Some(size_kb) => format!("{size_kb} KB"),
            None => "-".to_owned(),
        }
    }

    pub fn install_date_text(&self) -> String {
        let Some(raw) = self.install_date.as_deref() else {
            return "-".to_owned();
        };

        if raw.len() == 8 && raw.chars().all(|ch| ch.is_ascii_digit()) {
            format!("{}-{}-{}", &raw[0..4], &raw[4..6], &raw[6..8])
        } else {
            raw.to_owned()
        }
    }

    pub fn info_text(&self) -> String {
        let mut parts = Vec::new();
        let size = self.size_text();
        if size != "-" {
            parts.push(size);
        }

        let date = self.install_date_text();
        if date != "-" {
            parts.push(date);
        }

        if self.is_system_component {
            parts.push("숨김".to_owned());
        }

        if self.no_remove {
            parts.push("제거 제한".to_owned());
        }

        if parts.is_empty() {
            "-".to_owned()
        } else {
            parts.join(" / ")
        }
    }
}
