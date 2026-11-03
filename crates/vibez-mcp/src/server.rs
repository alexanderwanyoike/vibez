//! MCP tool surface: translates protocol calls into Vibez project operations.

use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{Implementation, ServerCapabilities, ServerConfig};
use rmcp::{tool, tool_handler, tool_router, Json, ServerHandler};
use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::device_params::{self, Device, ParameterInfo};

const INSTRUCTIONS: &str = "Builds Vibez projects: create a project, add tracks with native \
instruments and effects, place audio and note clips, then save a .vzp or bounce a WAV.";

#[derive(Clone)]
pub struct VibezServer {
    tool_router: ToolRouter<Self>,
}

impl Default for VibezServer {
    fn default() -> Self {
        Self::new()
    }
}

#[tool_router]
impl VibezServer {
    pub fn new() -> Self {
        Self {
            tool_router: Self::tool_router(),
        }
    }

    #[tool(
        description = "List a native device's parameters with their ranges and defaults. \
Use these names when setting instrument or effect parameters."
    )]
    async fn list_device_parameters(
        &self,
        Parameters(request): Parameters<DeviceRequest>,
    ) -> Json<DeviceParameters> {
        Json(DeviceParameters {
            device: request.device,
            parameters: device_params::parameter_info(request.device),
        })
    }
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DeviceRequest {
    pub device: Device,
}

#[derive(Debug, Serialize, JsonSchema)]
pub struct DeviceParameters {
    pub device: Device,
    pub parameters: Vec<ParameterInfo>,
}

#[tool_handler(router = self.tool_router)]
impl ServerHandler for VibezServer {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("vibez-mcp", env!("CARGO_PKG_VERSION")))
            .with_instructions(INSTRUCTIONS)
    }
}
