pub mod tools;
pub mod types;

use rmcp::{
    handler::server::{router::tool::ToolRouter, wrapper::Parameters},
    model::*,
    tool, tool_handler, tool_router,
    transport::stdio,
    ErrorData as McpError, ServerHandler, ServiceExt,
};

use crate::config::Config;
use crate::db::DbPool;

use types::*;

#[derive(Clone)]
pub struct SalitaMcp {
    pub config: Config,
    pub pool: DbPool,
    tool_router: ToolRouter<Self>,
}

#[tool_router]
impl SalitaMcp {
    pub fn new(config: Config, pool: DbPool) -> Self {
        Self {
            config,
            pool,
            tool_router: Self::tool_router(),
        }
    }

    #[tool(description = "List all devices in the mesh network with their online/offline status")]
    fn list_devices(&self) -> Result<CallToolResult, McpError> {
        self.list_devices_impl()
    }

    #[tool(
        description = "List files in a directory on any device. Specify device name/ID for remote, omit for local."
    )]
    fn list_files(
        &self,
        Parameters(params): Parameters<ListFilesParams>,
    ) -> Result<CallToolResult, McpError> {
        self.list_files_impl(params)
    }

    #[tool(
        description = "Search for files matching a glob pattern across devices. Supports patterns like '*.rs', '**/*.txt'."
    )]
    fn search_files(
        &self,
        Parameters(params): Parameters<SearchFilesParams>,
    ) -> Result<CallToolResult, McpError> {
        self.search_files_impl(params)
    }

    #[tool(
        description = "Read the content of a file from any device. Returns text content or binary file info."
    )]
    fn read_file(
        &self,
        Parameters(params): Parameters<ReadFileParams>,
    ) -> Result<CallToolResult, McpError> {
        self.read_file_impl(params)
    }

    #[tool(description = "Get metadata about a file (size, type, modified time) from any device.")]
    fn file_info(
        &self,
        Parameters(params): Parameters<FileInfoParams>,
    ) -> Result<CallToolResult, McpError> {
        self.file_info_impl(params)
    }
}

#[tool_handler]
impl ServerHandler for SalitaMcp {
    fn get_info(&self) -> ServerInfo {
        ServerInfo {
            instructions: Some(
                "Salita mesh file system. Browse and read files across all devices in your home network. \
                 Use list_devices to see available devices, then list_files/search_files/read_file/file_info \
                 to access files. Omit 'device' parameter for local files."
                    .into(),
            ),
            capabilities: ServerCapabilities::builder().enable_tools().build(),
            ..Default::default()
        }
    }
}

pub async fn run_mcp(config: Config, pool: DbPool) -> anyhow::Result<()> {
    let server = SalitaMcp::new(config, pool);
    let service = server.serve(stdio()).await?;
    service.waiting().await?;
    Ok(())
}
