use rmcp::schemars;

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ListFilesParams {
    /// Device name or ID (omit for local)
    #[serde(default)]
    pub device: Option<String>,
    /// Directory label (e.g. "documents", "photos")
    pub directory: String,
    /// Relative path within the directory (default: root)
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct SearchFilesParams {
    /// Glob pattern to search for (e.g. "*.rs", "**/*.txt")
    pub pattern: String,
    /// Device name or ID (omit for local)
    #[serde(default)]
    pub device: Option<String>,
    /// Directory label to search within (omit to search all)
    #[serde(default)]
    pub directory: Option<String>,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct ReadFileParams {
    /// Device name or ID (omit for local)
    #[serde(default)]
    pub device: Option<String>,
    /// Directory label
    pub directory: String,
    /// Relative path to the file
    pub path: String,
}

#[derive(Debug, serde::Deserialize, schemars::JsonSchema)]
pub struct FileInfoParams {
    /// Device name or ID (omit for local)
    #[serde(default)]
    pub device: Option<String>,
    /// Directory label
    pub directory: String,
    /// Relative path to the file
    pub path: String,
}
